//! Local-first install flow: install a skill from a directory path or
//! a URL (HTTP archive or git clone) without going through the
//! registry server.
//!
//! This module owns the tri-modal install dispatch:
//! - local path: read `skill.yaml` from the directory, validate, run
//!   the harness adapter's `convert`, persist to `config.installed`.
//! - URL: download or clone to a tempdir, then run the local-path
//!   flow against the extracted directory.
//!
//! The same convert+persist step is shared by all three modes (path,
//! URL, registry); only the source-of-truth changes.

use std::path::{Path, PathBuf};
use std::process::Command;

use usk_core::config::{Config, InstalledSkill};
use usk_core::lockfile::{LockEntry, Lockfile};
use usk_core::parser::parse_skill_yaml;
use usk_core::resolver::{ResolvedSkill, Resolver};
use usk_core::validation::validate;
use usk_harness_core::adapter::HarnessAdapter;
use usk_harness_core::error::HarnessError;
use usk_harness_core::paths;

/// Predicate: does this `name` look like a local filesystem path?
///
/// Local install is triggered when the name starts with `./`, `/`,
/// `~`, or `../`. Bare names (e.g. `escalation-handling`) flow
/// through the registry, which is the existing behavior.
pub fn is_local_path(name: &str) -> bool {
    name.starts_with("./")
        || name.starts_with("../")
        || name.starts_with('/')
        || name.starts_with('~')
}

/// Predicate: does this `name` look like a URL we know how to fetch?
pub fn is_url(name: &str) -> bool {
    name.starts_with("http://")
        || name.starts_with("https://")
        || name.starts_with("git@")
        || name.starts_with("git://")
        || name.starts_with("ssh://")
}

/// Install a skill from a local directory.
///
/// The directory must contain a `skill.yaml`. The skill is validated
/// (semver, entry file exists) and then the named harness adapter's
/// `convert` is invoked, writing to the harness-native install path
/// (or the `--target` override, if provided).
pub fn install_from_path(
    source: &Path,
    harness: &str,
    config: &Config,
) -> Result<PathBuf, String> {
    install_from_path_with_target(source, harness, config, None)
}

/// Same as [`install_from_path`] but with an explicit target
/// override. Used by the `usk install --target` flag and by tests
/// that want to direct the install to a non-default location.
pub fn install_from_path_with_target(
    source: &Path,
    harness: &str,
    config: &Config,
    target_override: Option<&Path>,
) -> Result<PathBuf, String> {
    if !source.exists() {
        return Err(format!("source path does not exist: {}", source.display()));
    }
    if !source.is_dir() {
        return Err(format!(
            "source path is not a directory: {}",
            source.display()
        ));
    }

    let skill_yaml = source.join("skill.yaml");
    if !skill_yaml.exists() {
        return Err(format!(
            "no skill.yaml found at {}; this does not look like a skill",
            source.display()
        ));
    }

    let skill = parse_skill_yaml(&skill_yaml).map_err(|e| {
        format!(
            "failed to parse {}: {}",
            skill_yaml.display(),
            e
        )
    })?;

    validate(&skill, source).map_err(|e| format!("validation failed: {}", e))?;

    if !paths::is_valid_skill_name(&skill.name) {
        return Err(format!(
            "skill name '{}' contains path-traversal or illegal characters",
            skill.name
        ));
    }

    let target = match target_override {
        Some(p) => p.to_path_buf(),
        None => resolve_target(harness, &skill.name, config)?,
    };
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "failed to create install root {}: {}",
                parent.display(),
                e
            )
        })?;
    }

    // U4 store + projection model. When the user did NOT pass
    // `--target`, the harness-native path is a *projection* — a
    // symlink at `target` that points at the USK-managed store
    // entry. The store at `~/.usk/store/<harness>/<name>/` is the
    // source of truth; `usk disable` removes the projection without
    // touching the store, and `usk enable` re-creates the symlink.
    //
    // When the user DID pass `--target`, the legacy behavior stands
    // — write directly to the explicit path. The store is opt-in
    // and the legacy behavior is preserved for ad-hoc targets.
    if target_override.is_none() {
        install_via_store(harness, &skill, source, &target)?;
    } else {
        install_direct(source, &target)?;
        run_convert(harness, &skill, source, &target)?;
    }

    persist_install(config, &skill, harness, &target)?;

    Ok(target)
}

/// Install the skill into the USK store and create a projection
/// (symlink) at the harness-native path. The store entry is
/// `~/.usk/store/<harness>/<name>/`; the symlink is at
/// `~/.claude/skills/<name>/` (or the equivalent for the harness).
fn install_via_store(
    harness: &str,
    skill: &usk_core::schema::Skill,
    source: &Path,
    projection: &Path,
) -> Result<(), String> {
    use usk_core::store::{Store, StoreEntry};

    let store_entry = Store::entry_path(harness, &skill.name);
    if let Some(parent) = store_entry.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create store root {}: {}", parent.display(), e))?;
    }
    if store_entry.exists() {
        std::fs::remove_dir_all(&store_entry)
            .map_err(|e| format!("clear old store entry: {}", e))?;
    }
    std::fs::create_dir_all(&store_entry)
        .map_err(|e| format!("create store entry {}: {}", store_entry.display(), e))?;

    // Copy raw source into the store so the convert step sees the
    // same layout a registry install would see.
    copy_dir_contents(source, &store_entry)?;
    run_convert(harness, skill, source, &store_entry)?;

    // The projection at the harness-native path: create the parent
    // (already done by caller), then symlink.
    if projection.exists() || projection.is_symlink() {
        // remove_file works for both symlinks and regular files; for
        // a directory, we need remove_dir_all. Use a single
        // conditional: a symlink is never a directory.
        if projection.is_symlink() || projection.is_file() {
            std::fs::remove_file(projection).ok();
        } else {
            std::fs::remove_dir_all(projection).ok();
        }
    }
    std::os::unix::fs::symlink(&store_entry, projection).map_err(|e| {
        format!(
            "create projection {} -> {}: {}",
            projection.display(),
            store_entry.display(),
            e
        )
    })?;

    // Record the install in the store index.
    let mut store = Store::load();
    store.add(StoreEntry {
        harness: harness.to_string(),
        name: skill.name.clone(),
        version: skill.version.clone(),
        store_path: store_entry,
        enabled: true,
    });
    if let Err(e) = store.save() {
        eprintln!("warning: store index write failed: {}", e);
    }

    Ok(())
}

/// Legacy direct-to-target install (used when `--target` is passed).
/// Writes the source files to `target` and runs the adapter
/// against the same path. No store, no projection.
fn install_direct(source: &Path, target: &Path) -> Result<(), String> {
    if target.exists() {
        std::fs::remove_dir_all(target)
            .map_err(|e| format!("failed to clear existing install at {}: {}", target.display(), e))?;
    }
    std::fs::create_dir_all(target)
        .map_err(|e| format!("failed to create install dir {}: {}", target.display(), e))?;
    copy_dir_contents(source, target)?;
    Ok(())
}

/// Copy the contents of `src` into `dst` (which must already exist).
/// Used by `install_from_path` to seed the harness-native install
/// location with the raw source files before the adapter runs.
fn copy_dir_contents(src: &Path, dst: &Path) -> Result<(), String> {
    copy_dir_contents_recursive(src, dst)
}

fn copy_dir_contents_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(src)
        .map_err(|e| format!("read_dir {}: {}", src.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {}", e))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("file_type: {}", e))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_dir() {
            std::fs::create_dir_all(&to)
                .map_err(|e| format!("create dir {}: {}", to.display(), e))?;
            copy_dir_contents_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| format!("copy {} -> {}: {}", from.display(), to.display(), e))?;
        }
    }
    Ok(())
}

/// Install a skill from a URL (HTTP archive or git URL).
///
/// For `https://`/`http://` URLs, the body is downloaded to a temp
/// file. If it looks like a tar.gz / tar.bz2 / tar.xz / zip archive
/// (by extension), it's extracted. Otherwise the body is treated as a
/// raw `skill.yaml`.
///
/// For `git@...` / `git://...` / `ssh://...` URLs, the repo is
/// shallow-cloned into a tempdir and treated as a local path.
///
/// The tempdir is cleaned up before returning; the final install
/// lives at the harness-native path.
pub fn install_from_url(
    url: &str,
    harness: &str,
    config: &Config,
) -> Result<PathBuf, String> {
    if is_git_url(url) {
        let tmp = tempfile::tempdir().map_err(|e| format!("tempdir: {}", e))?;
        clone_git(url, tmp.path())?;
        let final_path = install_from_path(tmp.path(), harness, config)?;
        // tmp is dropped here, removing the clone root. The final
        // install at `final_path` is independent of the tempdir.
        return Ok(final_path);
    }

    // HTTP fetch.
    let tmp = tempfile::tempdir().map_err(|e| format!("tempdir: {}", e))?;
    let body = fetch_http(url)?;
    std::fs::write(tmp.path().join("downloaded"), &body)
        .map_err(|e| format!("write downloaded body: {}", e))?;

    let lower = url.to_lowercase();
    if lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || lower.ends_with(".tar.bz2")
        || lower.ends_with(".tar.xz")
    {
        let archive_path = tmp.path().join("archive.bin");
        std::fs::write(&archive_path, &body)
            .map_err(|e| format!("write archive: {}", e))?;
        let extract_dir = tmp.path().join("extracted");
        std::fs::create_dir_all(&extract_dir)
            .map_err(|e| format!("create extract dir: {}", e))?;
        extract_tar(&archive_path, &extract_dir)?;

        // Archives commonly have a single top-level directory; detect
        // that and pass its contents as the source root.
        let source_root = locate_archive_root(&extract_dir).unwrap_or(extract_dir);
        install_from_path(&source_root, harness, config)
    } else {
        // Treat the body as a single skill.yaml in an ad-hoc dir.
        let adhoc = tmp.path().join("skill");
        std::fs::create_dir_all(&adhoc).map_err(|e| format!("mkdir: {}", e))?;
        std::fs::write(adhoc.join("skill.yaml"), &body)
            .map_err(|e| format!("write skill.yaml: {}", e))?;
        install_from_path(&adhoc, harness, config)
    }
}

// ---- U11: skill collections (R13) --------------------------------
//
// A "collection" is a directory whose immediate subdirectories are
// each a self-contained skill (each with its own `skill.yaml`). The
// install flow treats it as a bulk install: walk the children, install
// each in turn, and collect the results. One failure does not abort
// the others; we report all errors at the end.

/// Detect whether `source` looks like a skill collection.
///
/// A directory is a collection when it contains **two or more**
/// subdirectories, each of which has a `skill.yaml` at its root. We
/// require at least two so a single-skill install path
/// (`install_from_path`) still works for normal cases. A single skill
/// under a directory is *not* a collection — that's exactly what
/// `install_from_path` is for.
pub fn is_collection(source: &Path) -> bool {
    if !source.is_dir() {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(source) else {
        return false;
    };
    let mut skill_subdirs = 0usize;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() && p.join("skill.yaml").is_file() {
            skill_subdirs += 1;
        }
    }
    skill_subdirs >= 2
}

/// Install every skill under `source` as a collection.
///
/// Each subdirectory containing a `skill.yaml` is installed via
/// [`install_from_path`]. Errors on individual skills are collected
/// and reported (to stderr and the returned `Err` only if EVERY
/// install failed). The first failing error is also surfaced.
///
/// We deliberately do NOT short-circuit on the first failure: a
/// collection with 10 skills where 1 is broken should still install
/// the other 9, since the user is likely bulk-installing.
pub fn install_collection(
    source: &Path,
    harness: &str,
    config: &Config,
) -> Result<Vec<PathBuf>, String> {
    if !source.is_dir() {
        return Err(format!(
            "collection source '{}' is not a directory",
            source.display()
        ));
    }

    let mut paths: Vec<PathBuf> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    let entries = std::fs::read_dir(source)
        .map_err(|e| format!("read_dir on collection '{}': {}", source.display(), e))?;

    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        // Skip subdirs without a skill.yaml — they're probably
        // README.md folders, examples, etc.
        if !p.join("skill.yaml").is_file() {
            continue;
        }

        match install_from_path(&p, harness, config) {
            Ok(installed) => paths.push(installed),
            Err(e) => errors.push(format!("{}: {}", p.display(), e)),
        }
    }

    if paths.is_empty() && !errors.is_empty() {
        return Err(format!(
            "no skills were installed from '{}' ({} error(s)):\n  - {}",
            source.display(),
            errors.len(),
            errors.join("\n  - ")
        ));
    }

    if !errors.is_empty() {
        eprintln!(
            "Installed {} skill(s) from '{}'; {} error(s):\n  - {}",
            paths.len(),
            source.display(),
            errors.len(),
            errors.join("\n  - ")
        );
    } else {
        eprintln!(
            "Installed {} skill(s) from '{}'",
            paths.len(),
            source.display()
        );
    }

    Ok(paths)
}

// ---- U12: lockfile integration (R14) ------------------------------
//
// After a successful install, we record the (name, version, harness,
// install_path) tuple to `usk.lock` so future `usk install --locked`
// invocations can reproduce the exact install. The lockfile lives in
// the project root (cwd) by default; if cwd is not writable, callers
// can pass an explicit path.

/// Default per-project lockfile path: `<cwd>/usk.lock`.
///
/// Callers that need a different location (e.g. the global registry)
/// can use [`record_to_lockfile_at`].
pub fn default_lockfile_path() -> PathBuf {
    std::env::current_dir()
        .map(|d| d.join("usk.lock"))
        .unwrap_or_else(|_| PathBuf::from("usk.lock"))
}

/// Build a `LockEntry` from the values a successful install just
/// produced. Kept as a small helper so call sites don't have to
/// remember the field order.
pub fn entry_from_install(
    name: &str,
    version: &str,
    harness: &str,
    install_path: &Path,
) -> LockEntry {
    LockEntry {
        name: name.to_string(),
        version: version.to_string(),
        harness: harness.to_string(),
        install_path: install_path.to_path_buf(),
    }
}

/// Record an installed skill to the per-project lockfile.
///
/// On any lockfile error, we warn and continue — a lockfile write
/// failure must not roll back an otherwise-successful install. The
/// skill is already on disk; the lockfile is bookkeeping.
pub fn record_to_lockfile(entry: &LockEntry) -> Result<(), String> {
    let path = default_lockfile_path();
    record_to_lockfile_at(entry, &path)
}

/// Same as [`record_to_lockfile`] but with an explicit path. Used by
/// the global install flow (writes to `~/.usk/installed.lock`) and by
/// tests.
pub fn record_to_lockfile_at(entry: &LockEntry, path: &Path) -> Result<(), String> {
    let mut lf = Lockfile::load(path);
    lf.add_entry(entry.clone());
    lf.save(path).map_err(|e| {
        format!(
            "failed to write lockfile at {}: {}",
            path.display(),
            e
        )
    })
}

/// Read the lockfile at `path` and return its entries.
pub fn load_lockfile_at(path: &Path) -> Lockfile {
    Lockfile::load(path)
}

// ---- U10: `requires` integration (R12) -----------------------------
//
// These wrappers combine the resolver (U10 in usk-core) with the
// install flow (Tier 1). The resolver returns skills in install
// order; we install the start skill from the local source. Non-start
// dependencies require a registry fetch (or local source), which is
// out of scope for the v1 local-first flow — we surface a clear
// warning if any non-start skills are listed.

/// Resolve a skill's `requires` graph and install the start skill.
///
/// `lookup` and `requires` are closures that the caller wires up to
/// its data source — typically a `RegistryIndex` in v1, or a
/// synthetic graph in tests. The returned list of paths is the
/// `[start_skill_install_path]` (currently v1 only installs the
/// local source; non-start deps are reported as warnings).
pub fn install_with_requires<L, R>(
    source: &Path,
    harness: &str,
    config: &Config,
    lookup: L,
    requires: R,
) -> Result<Vec<PathBuf>, String>
where
    L: Fn(&str) -> Option<usk_core::schema::SkillMeta>,
    R: Fn(&str, &str) -> Vec<String>,
{
    let skill_yaml = source.join("skill.yaml");
    if !skill_yaml.exists() {
        return Err(format!(
            "no skill.yaml at {}; cannot resolve requires",
            source.display()
        ));
    }
    let start_skill = parse_skill_yaml(&skill_yaml)
        .map_err(|e| format!("parse skill.yaml: {}", e))?;
    let start_meta = usk_core::schema::SkillMeta::from(&start_skill);

    let mut resolver = Resolver::new();
    let order = resolver
        .resolve(&start_meta.name, lookup, requires)
        .map_err(|e| format!("dependency resolution failed: {}", e))?;

    // Warn for any non-start skill the resolver produced. In v1
    // we have no local source for those (they'd need a registry or
    // to be in cwd for `install_collection` to find them).
    for ResolvedSkill { name, version } in &order {
        if name == &start_meta.name {
            continue;
        }
        eprintln!(
            "warning: dependency '{}' v{} has no local source; \
             install via the registry or place it under the project root",
            name, version
        );
    }

    let path = install_from_path(source, harness, config)?;
    Ok(vec![path])
}

// ---- internals ---------------------------------------------------

/// Resolve the target install path for a (harness, skill_name) pair.
///
/// Priority: harness-known path (U1) → existing `config.install_dir`
/// staging location (fallback for unknown harnesses).
fn resolve_target(
    harness: &str,
    skill_name: &str,
    config: &Config,
) -> Result<PathBuf, String> {
    if let Some(p) = paths::resolve_install_path(harness, skill_name) {
        return Ok(p);
    }
    // Fall back to the legacy staging location. We still pass through
    // the path-traversal check, since we splice the skill name in.
    if !paths::is_valid_skill_name(skill_name) {
        return Err(format!(
            "skill name '{}' is not safe for filesystem paths",
            skill_name
        ));
    }
    Ok(config.install_dir.join(harness).join(skill_name))
}

/// Run the harness adapter's `convert` step. Mirrors the registry
/// install path's behavior of writing into a per-skill directory.
fn run_convert(
    harness: &str,
    skill: &usk_core::schema::Skill,
    source: &Path,
    target: &Path,
) -> Result<(), String> {
    let result: Result<(), HarnessError> = match harness {
        "claude-code" => {
            let adapter = usk_harness_claude::converter::ClaudeCodeAdapter;
            adapter.convert(skill, source, target)
        }
        "codex-cli" => {
            let adapter = usk_harness_codex::converter::CodexCliAdapter;
            adapter.convert(skill, source, target)
        }
        other => {
            return Err(format!(
                "unknown harness '{}'; use `usk harness add` to register",
                other
            ));
        }
    };
    result.map_err(|e| format!("harness conversion failed: {}", e))
}

/// Persist the install to `config.installed` and write back to disk.
fn persist_install(
    config: &Config,
    skill: &usk_core::schema::Skill,
    harness: &str,
    target: &Path,
) -> Result<(), String> {
    let mut updated = config.clone();
    updated.installed.insert(
        skill.name.clone(),
        InstalledSkill {
            version: skill.version.clone(),
            harness: harness.to_string(),
            install_path: target.to_path_buf(),
        },
    );
    updated
        .save()
        .map_err(|e| format!("failed to save config: {}", e))?;
    Ok(())
}

fn is_git_url(url: &str) -> bool {
    url.starts_with("git@")
        || url.starts_with("git://")
        || url.starts_with("ssh://")
        || url.ends_with(".git")
}

/// Synchronous HTTP GET. We use the async reqwest client via a
/// one-shot tokio runtime, since `usk-cli` doesn't have the
/// `reqwest` `blocking` feature enabled and adding a new workspace
/// dependency is out of scope for this tier. The cost is one
/// runtime spawn per URL install, which is acceptable for a
/// non-hot-path command.
fn fetch_http(url: &str) -> Result<Vec<u8>, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("build tokio runtime: {}", e))?;
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| format!("build http client: {}", e))?;
        let resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("http GET {} failed: {}", url, e))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("http GET {} returned status {}", url, status));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("read response body: {}", e))?;
        Ok(bytes.to_vec())
    })
}

/// Shell out to `git clone --depth 1`. We use `git2` only for
/// in-process cloning when available, but in this sync code path the
/// simplest correct thing is to shell out — the workspace already
/// declares `git2`, but `usk-cli` itself does not depend on it, and
/// adding a dependency is out of scope for this tier.
fn clone_git(url: &str, dest: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(url)
        .arg(dest)
        .output()
        .map_err(|e| {
            format!(
                "failed to invoke git (is it on PATH?): {}",
                e
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git clone failed: {}", stderr.trim()));
    }
    Ok(())
}

/// Extract a `.tar.{gz,bz2,xz}` archive into `dest`. We use the
/// `tar` and `flate2` crates already in the workspace.
fn extract_tar(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| format!("open archive: {}", e))?;

    // Decide on a decoder by peeking at the first two bytes (gzip
    // magic: 0x1f 0x8b). For .tar.bz2 / .tar.xz we still need a
    // decoder; we attempt gzip first, then fall back to an
    // uncompressed tar reader.
    let mut reader: Box<dyn std::io::Read> = match peek_magic_gzip(archive) {
        true => Box::new(flate2::read::GzDecoder::new(file)),
        false => Box::new(file),
    };

    let mut archive = tar::Archive::new(&mut reader);
    archive
        .unpack(dest)
        .map_err(|e| format!("extract tar: {}", e))?;
    Ok(())
}

fn peek_magic_gzip(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    use std::io::Read;
    let mut buf = [0u8; 2];
    if f.read(&mut buf).is_err() {
        return false;
    }
    buf[0] == 0x1f && buf[1] == 0x8b
}

/// If the extracted dir contains exactly one subdirectory and no
/// `skill.yaml` at the root, treat that subdirectory as the archive
/// root. Otherwise return `None` (caller should use the dir as-is).
fn locate_archive_root(extract_dir: &Path) -> Option<PathBuf> {
    let skill_yaml = extract_dir.join("skill.yaml");
    if skill_yaml.exists() {
        return None;
    }
    let entries: Vec<PathBuf> = std::fs::read_dir(extract_dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    if entries.len() == 1 {
        let candidate = &entries[0];
        if candidate.join("skill.yaml").exists() {
            return Some(candidate.clone());
        }
    }
    None
}

// ---- tests -------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::collections::HashMap;
    use tempfile::tempdir;

    fn write_skill(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let yaml = format!(
            r#"name: "{name}"
version: "1.0.0"
description: "test"
author: "tester"
license: MIT
tags: []
harnesses: {{}}
requires: []
entry: SKILL.md
"#,
            name = name
        );
        std::fs::write(dir.join("skill.yaml"), yaml).unwrap();
        std::fs::write(dir.join("SKILL.md"), format!("# {}\n", name)).unwrap();
    }

    /// Run `f` with a set of env vars temporarily set, restoring
    /// (or removing) the prior values on the way out. `vars` is a
    /// slice of `(name, Some(value))` to set or `(name, None)` to
    /// remove.
    fn with_env<F: FnOnce()>(vars: &[(&str, Option<&str>)], f: F) {
        struct Restore(Vec<(String, Option<String>)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (k, v) in self.0.drain(..) {
                    match v {
                        Some(val) => std::env::set_var(&k, val),
                        None => std::env::remove_var(&k),
                    }
                }
            }
        }
        let mut prior: Vec<(String, Option<String>)> = Vec::new();
        for (k, v) in vars {
            prior.push((k.to_string(), std::env::var(k).ok()));
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
        let _restore = Restore(prior);
        f();
    }

    fn test_config(install_dir: &Path) -> Config {
        let mut harnesses = HashMap::new();
        harnesses.insert(
            "claude-code".to_string(),
            "usk-harness-claude".to_string(),
        );
        Config {
            registry_url: "http://localhost:0".to_string(),
            install_dir: install_dir.to_path_buf(),
            harnesses,
            installed: HashMap::new(),
        }
    }

    #[test]
    fn is_local_path_predicate() {
        assert!(is_local_path("./here"));
        assert!(is_local_path("../up"));
        assert!(is_local_path("/abs/path"));
        assert!(is_local_path("~/home"));
        assert!(!is_local_path("escalation-handling"));
        assert!(!is_local_path("https://example.com/x"));
    }

    #[test]
    fn is_url_predicate() {
        assert!(is_url("https://example.com/x.tar.gz"));
        assert!(is_url("http://example.com/x"));
        assert!(is_url("git@github.com:foo/bar.git"));
        assert!(is_url("git://github.com/foo/bar"));
        assert!(!is_url("./local"));
        assert!(!is_url("escalation-handling"));
    }

    #[test]
    #[serial]
    fn install_from_path_writes_to_harness_native_location() {
        let home = tempdir().expect("home");
        let install_dir = tempdir().expect("install dir");
        let source = tempdir().expect("source");
        write_skill(source.path(), "escalation-handling");

        std::env::set_var("HOME", home.path());
        let config = test_config(install_dir.path());
        let result = install_from_path(source.path(), "claude-code", &config);
        std::env::remove_var("HOME");

        let target = result.expect("install_from_path should succeed");
        let expected = home
            .path()
            .join(".claude")
            .join("skills")
            .join("escalation-handling");
        assert_eq!(target, expected, "target should be harness-native");
        assert!(
            target.join("SKILL.md").exists(),
            "SKILL.md should be written at the harness-native path"
        );
        assert!(
            target.join("skill.yaml").exists(),
            "skill.yaml should be written at the harness-native path"
        );
    }

    #[test]
    #[serial]
    fn install_from_path_persists_to_config() {
        let home = tempdir().expect("home");
        let install_dir = tempdir().expect("install dir");
        let source = tempdir().expect("source");
        write_skill(source.path(), "my-skill");

        // Use USK_CONFIG_DIR so the install's `config.save()` and the
        // post-install `Config::load()` agree on the same path
        // regardless of the parent process's HOME. Previously the
        // test set `HOME` and then removed it before reloading,
        // which made `Config::load()` fall back to the real `$HOME`
        // and miss the just-written config.
        let config_dir = tempdir().expect("config dir");
        with_env(&[
            ("USK_CONFIG_DIR", Some(config_dir.path().to_str().unwrap())),
            ("HOME", Some(home.path().to_str().unwrap())),
        ], || {
            let config = test_config(install_dir.path());
            install_from_path(source.path(), "claude-code", &config)
                .expect("install should succeed");
            let reloaded = Config::load();
            let entry = reloaded
                .installed
                .get("my-skill")
                .expect("my-skill should be in config.installed");
            assert_eq!(entry.harness, "claude-code");
            assert_eq!(entry.version, "1.0.0");
            assert!(entry.install_path.ends_with("my-skill"));
        });
    }

    #[test]
    #[serial]
    fn install_from_path_rejects_traversal_in_skill_name() {
        let home = tempdir().expect("home");
        let install_dir = tempdir().expect("install dir");
        let source = tempdir().expect("source");

        // Skill name contains ".." — the validator should reject it.
        write_skill(source.path(), "evil/../escape");

        std::env::set_var("HOME", home.path());
        let config = test_config(install_dir.path());
        let result = install_from_path(source.path(), "claude-code", &config);
        std::env::remove_var("HOME");

        assert!(result.is_err(), "traversal must be rejected");
        let err = result.unwrap_err();
        assert!(
            err.contains("path-traversal") || err.contains("not safe"),
            "error should mention traversal: {}",
            err
        );
    }

    #[test]
    #[serial]
    fn install_from_path_target_override_used() {
        let home = tempdir().expect("home");
        let install_dir = tempdir().expect("install dir");
        let source = tempdir().expect("source");
        write_skill(source.path(), "my-skill");

        std::env::set_var("HOME", home.path());
        let config = test_config(install_dir.path());
        let override_target = home.path().join("custom").join("my-skill");

        let result = install_from_path_with_target(
            source.path(),
            "claude-code",
            &config,
            Some(&override_target),
        );
        std::env::remove_var("HOME");

        let target = result.expect("install with override should succeed");
        assert_eq!(
            target, override_target,
            "target should match the override"
        );
        assert!(
            target.join("SKILL.md").exists(),
            "SKILL.md should be written to the override path"
        );

        // Harness-native path should NOT have been touched.
        let native = home.path().join(".claude").join("skills").join("my-skill");
        assert!(
            !native.exists(),
            "default harness-native path should not be used when --target is set"
        );
    }

    #[test]
    #[serial]
    fn install_from_path_errors_when_skill_yaml_missing() {
        let home = tempdir().expect("home");
        let install_dir = tempdir().expect("install dir");
        let source = tempdir().expect("source");
        // Don't write skill.yaml.

        std::env::set_var("HOME", home.path());
        let config = test_config(install_dir.path());
        let result = install_from_path(source.path(), "claude-code", &config);
        std::env::remove_var("HOME");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no skill.yaml"));
    }

    #[test]
    #[serial]
    fn install_from_path_errors_on_unknown_harness() {
        let home = tempdir().expect("home");
        let install_dir = tempdir().expect("install dir");
        let source = tempdir().expect("source");
        write_skill(source.path(), "my-skill");

        std::env::set_var("HOME", home.path());
        let config = test_config(install_dir.path());
        let result = install_from_path(source.path(), "mystery-harness", &config);
        std::env::remove_var("HOME");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown harness"));
    }

    #[test]
    fn peek_magic_gzip_detects_gzip() {
        let dir = tempdir().unwrap();
        let gz = dir.path().join("x.tar.gz");
        std::fs::write(&gz, [0x1f, 0x8b, 0x08]).unwrap();
        assert!(peek_magic_gzip(&gz));
        let plain = dir.path().join("x.txt");
        std::fs::write(&plain, b"hello").unwrap();
        assert!(!peek_magic_gzip(&plain));
    }

    #[test]
    fn locate_archive_root_finds_single_subdir() {
        let dir = tempdir().unwrap();
        let inner = dir.path().join("my-skill-1.0");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("skill.yaml"), "name: my-skill\nversion: 1.0.0\n").unwrap();
        let root = locate_archive_root(dir.path()).expect("locate");
        assert_eq!(root, inner);
    }

    #[test]
    fn locate_archive_root_returns_none_when_skill_yaml_present() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("skill.yaml"), "name: x\n").unwrap();
        assert!(locate_archive_root(dir.path()).is_none());
    }

    // ---- U11 (collections) tests ----

    #[test]
    fn is_collection_predicate() {
        let dir = tempdir().unwrap();

        // Empty dir: not a collection.
        assert!(!is_collection(dir.path()));

        // Single skill subdir: not a collection (>=2 required).
        write_skill(&dir.path().join("alpha"), "alpha");
        assert!(!is_collection(dir.path()), "single skill is not a collection");

        // Two skill subdirs: IS a collection.
        write_skill(&dir.path().join("beta"), "beta");
        assert!(is_collection(dir.path()), "two skills should be a collection");

        // Non-skill subdirs are ignored.
        std::fs::create_dir(dir.path().join("README")).unwrap();
        assert!(is_collection(dir.path()));
    }

    #[test]
    fn is_collection_returns_false_for_non_dir() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("a-file.txt");
        std::fs::write(&f, "x").unwrap();
        assert!(!is_collection(&f));
    }

    #[test]
    #[serial]
    fn install_collection_installs_all_subdirs() {
        let home = tempdir().expect("home");
        let install_dir = tempdir().expect("install dir");
        let source = tempdir().expect("source");

        write_skill(&source.path().join("alpha"), "alpha");
        write_skill(&source.path().join("beta"), "beta");
        write_skill(&source.path().join("gamma"), "gamma");

        std::env::set_var("HOME", home.path());
        let config = test_config(install_dir.path());
        let result = install_collection(source.path(), "claude-code", &config);
        std::env::remove_var("HOME");

        let paths = result.expect("collection install should succeed");
        assert_eq!(paths.len(), 3, "expected 3 installs, got {}", paths.len());

        // Each skill should now be present in the harness-native tree.
        for name in ["alpha", "beta", "gamma"] {
            let p = home.path().join(".claude").join("skills").join(name);
            assert!(p.exists(), "expected {} at {:?}", name, p);
        }
    }

    #[test]
    #[serial]
    fn install_collection_skips_dirs_without_skill_yaml() {
        let home = tempdir().expect("home");
        let install_dir = tempdir().expect("install dir");
        let source = tempdir().expect("source");

        write_skill(&source.path().join("alpha"), "alpha");
        // A dir without skill.yaml — should be skipped silently.
        std::fs::create_dir(source.path().join("docs")).unwrap();
        write_skill(&source.path().join("beta"), "beta");

        std::env::set_var("HOME", home.path());
        let config = test_config(install_dir.path());
        let result = install_collection(source.path(), "claude-code", &config);
        std::env::remove_var("HOME");

        let paths = result.expect("install should succeed");
        assert_eq!(paths.len(), 2, "non-skill dirs should be skipped");
    }

    // ---- U12 (lockfile) tests ----

    #[test]
    fn record_to_lockfile_writes_and_reads_back() {
        let dir = tempdir().unwrap();
        let lock_path = dir.path().join("usk.lock");

        let entry = entry_from_install(
            "demo-skill",
            "1.2.3",
            "claude-code",
            &dir.path().join("install/demo-skill"),
        );
        record_to_lockfile_at(&entry, &lock_path).expect("record should succeed");

        let loaded = load_lockfile_at(&lock_path);
        assert_eq!(loaded.skill.len(), 1);
        assert_eq!(loaded.skill[0].name, "demo-skill");
        assert_eq!(loaded.skill[0].version, "1.2.3");
        assert_eq!(loaded.skill[0].harness, "claude-code");
    }

    #[test]
    fn record_to_lockfile_overwrites_same_name() {
        let dir = tempdir().unwrap();
        let lock_path = dir.path().join("usk.lock");

        let e1 = entry_from_install("foo", "1.0.0", "claude-code", &dir.path().join("foo"));
        let e2 = entry_from_install("foo", "1.1.0", "claude-code", &dir.path().join("foo"));
        record_to_lockfile_at(&e1, &lock_path).unwrap();
        record_to_lockfile_at(&e2, &lock_path).unwrap();

        let loaded = load_lockfile_at(&lock_path);
        assert_eq!(loaded.skill.len(), 1, "duplicate name should be overwritten, not appended");
        assert_eq!(loaded.skill[0].version, "1.1.0");
    }

    #[test]
    fn lockfile_load_returns_empty_when_file_missing() {
        let dir = tempdir().unwrap();
        let lock_path = dir.path().join("does-not-exist.lock");
        let loaded = load_lockfile_at(&lock_path);
        assert!(loaded.skill.is_empty());
    }

    // ---- U10 (requires) integration test ----

    /// Integration test: install a local skill whose `requires` lists
    /// another local skill. We use an in-memory resolver graph so the
    /// test does not require a registry.
    #[test]
    #[serial]
    fn install_with_requires_resolves_local_graph() {
        let home = tempdir().expect("home");
        let install_dir = tempdir().expect("install dir");
        let source = tempdir().expect("source");
        write_skill(source.path(), "top-skill");

        // A graph where "top-skill" requires "dep-skill" (which has no deps).
        let dep_meta = usk_core::schema::SkillMeta {
            name: "dep-skill".to_string(),
            version: "0.5.0".to_string(),
            description: None,
            author: None,
            tags: vec![],
            harnesses: vec![],
        };
        let top_meta = usk_core::schema::SkillMeta {
            name: "top-skill".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            tags: vec![],
            harnesses: vec![],
        };
        let lookup = move |name: &str| -> Option<usk_core::schema::SkillMeta> {
            match name {
                "dep-skill" => Some(dep_meta.clone()),
                "top-skill" => Some(top_meta.clone()),
                _ => None,
            }
        };
        let requires = |name: &str, _ver: &str| -> Vec<String> {
            match name {
                "top-skill" => vec!["dep-skill".to_string()],
                _ => vec![],
            }
        };

        std::env::set_var("HOME", home.path());
        let config = test_config(install_dir.path());
        let result = install_with_requires(
            source.path(),
            "claude-code",
            &config,
            lookup,
            requires,
        );
        std::env::remove_var("HOME");

        // v1 only installs the local start skill; the non-start dep
        // is reported via stderr (we don't assert on that here).
        let paths = result.expect("install_with_requires should succeed");
        assert_eq!(paths.len(), 1, "start skill should be installed");
        let expected = home.path().join(".claude").join("skills").join("top-skill");
        assert_eq!(paths[0], expected);
    }

    #[test]
    fn install_with_requires_errors_on_missing_dep() {
        let home = tempdir().expect("home");
        let install_dir = tempdir().expect("install dir");
        let source = tempdir().expect("source");
        write_skill(source.path(), "needs-missing");

        let top_meta = usk_core::schema::SkillMeta {
            name: "needs-missing".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            tags: vec![],
            harnesses: vec![],
        };
        let lookup = move |name: &str| -> Option<usk_core::schema::SkillMeta> {
            if name == "needs-missing" { Some(top_meta.clone()) } else { None }
        };
        let requires = |_name: &str, _ver: &str| -> Vec<String> {
            vec!["missing-skill".to_string()]
        };

        std::env::set_var("HOME", home.path());
        let config = test_config(install_dir.path());
        let result = install_with_requires(
            source.path(),
            "claude-code",
            &config,
            lookup,
            requires,
        );
        std::env::remove_var("HOME");

        assert!(result.is_err(), "missing dep should be reported");
        let err = result.unwrap_err();
        assert!(
            err.contains("missing-skill") || err.contains("not found") || err.contains("resolution"),
            "error should mention the missing dep: {}",
            err
        );
    }

    /// End-to-end: install `spec/examples/escalation-handling` and
    /// verify the SKILL.md lands at the harness-native path. This is
    /// the "consumer journey" test for U2.
    #[test]
    #[serial]
    fn install_escalation_handling_example_into_claude_skills_dir() {
        // Resolve the example relative to the workspace root.
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
        let example_dir = workspace_root.join("spec/examples/escalation-handling");
        assert!(
            example_dir.exists(),
            "example skill not found at {}",
            example_dir.display()
        );

        let home = tempdir().expect("home");
        let install_dir = tempdir().expect("install dir");

        std::env::set_var("HOME", home.path());
        let config = test_config(install_dir.path());
        let result = install_from_path(&example_dir, "claude-code", &config);
        std::env::remove_var("HOME");

        let target = result.expect("install from example skill should succeed");
        let expected = home
            .path()
            .join(".claude")
            .join("skills")
            .join("escalation-handling");
        assert_eq!(
            target, expected,
            "skills should land at $HOME/.claude/skills/escalation-handling"
        );
        assert!(target.join("SKILL.md").exists());
        assert!(target.join("skill.yaml").exists());
    }
}
