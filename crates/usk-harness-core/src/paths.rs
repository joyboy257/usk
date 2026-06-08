//! Default install paths for each known harness.
//!
//! When a consumer runs `usk install <name> --harness <h>`, the skill
//! should land in the directory the harness actually reads from
//! (e.g. `~/.claude/skills/<name>/` for Claude Code). The map below is
//! the source of truth for those paths; it lives in code (not in
//! `Config`) so it stays versioned with the harness adapter crate and
//! updates with each release.
//!
//! Callers that need an override can pass `--target <path>` and skip
//! this lookup.
//!
//! ## Verification sources
//!
//! - **`claude-code` → `~/.claude/skills/<name>/`** — verified 2026-06-08
//!   against a live install on macOS (130+ skills present at
//!   `~/.claude/skills/`). Claude Code's docs describe this as the
//!   canonical skill-read directory. See
//!   <https://docs.claude.com/en/docs/claude-code/skills>.
//! - **`codex-cli` → `~/.codex/agents/<name>.yaml`** — UNVERIFIED. On
//!   the development machine used for v0.2 verification, the
//!   `~/.codex/agents/` directory did not exist; Codex CLI's actual
//!   skill layout is uncertain. Treated as best-effort pending docs
//!   research. If a user runs `usk install --harness codex-cli` and
//!   the file does not appear in Codex's UI, the user should fall
//!   back to `--target` and report the path the harness actually
//!   reads from so this constant can be corrected.

use std::path::{Path, PathBuf};

/// Default install path for a given harness key and skill name.
///
/// Returns `None` if the harness key is unknown. The path is resolved
/// against the value of the `HOME` environment variable at call time;
/// callers can override with a `--target` flag.
pub fn default_install_path(harness_key: &str, skill_name: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let home = PathBuf::from(home);
    match harness_key {
        "claude-code" => Some(home.join(".claude").join("skills").join(skill_name)),
        "codex-cli" => Some(
            home.join(".codex")
                .join("agents")
                .join(format!("{}.yaml", skill_name)),
        ),
        _ => None,
    }
}

/// Validate that a skill name is safe to use as a directory component.
///
/// Rejects names containing path traversal characters (`..`), path
/// separators (`/`, `\`), null bytes, or names that resolve to `.` /
/// `..`. This guards `default_install_path` (and any caller that
/// splices the name into a filesystem path) from being tricked into
/// writing outside the install root.
pub fn is_valid_skill_name(skill_name: &str) -> bool {
    if skill_name.is_empty() {
        return false;
    }
    if skill_name == "." || skill_name == ".." {
        return false;
    }
    if skill_name.contains('/') || skill_name.contains('\\') {
        return false;
    }
    if skill_name.contains("..") {
        return false;
    }
    if skill_name.contains('\0') {
        return false;
    }
    true
}

/// Convenience helper: resolve the harness-native install path and
/// validate the skill name in one call. Returns `None` if either check
/// fails.
pub fn resolve_install_path(harness_key: &str, skill_name: &str) -> Option<PathBuf> {
    if !is_valid_skill_name(skill_name) {
        return None;
    }
    default_install_path(harness_key, skill_name)
}

/// Return the install path's parent directory, for callers that want
/// to create the install root before invoking the harness adapter.
pub fn install_root(harness_key: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    install_root_in(&PathBuf::from(home), harness_key)
}

/// Same as [`install_root`] but with an explicit `home` directory.
/// Lets tests and code paths that already have a `Path` in hand
/// avoid races on the process-global `HOME` env var.
pub fn install_root_in(home: &Path, harness_key: &str) -> Option<PathBuf> {
    match harness_key {
        "claude-code" => Some(home.join(".claude").join("skills")),
        "codex-cli" => Some(home.join(".codex").join("agents")),
        _ => None,
    }
}

/// Return `true` if `path` looks like a harness install path produced
/// by this module. Used by tests and inspection helpers.
pub fn is_under_install_root(harness_key: &str, path: &Path) -> bool {
    match install_root(harness_key) {
        Some(root) => path.starts_with(root),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn default_install_path_for_claude_code() {
        let p = default_install_path("claude-code", "foo").expect("claude-code is known");
        assert!(p.ends_with(".claude/skills/foo"), "got {:?}", p);
    }

    #[test]
    fn default_install_path_for_codex_cli() {
        let p = default_install_path("codex-cli", "bar").expect("codex-cli is known");
        assert!(p.ends_with(".codex/agents/bar.yaml"), "got {:?}", p);
    }

    #[test]
    #[serial]
    fn default_install_path_resolves_against_home() {
        // Use a stable synthetic HOME to assert the prefix exactly.
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("HOME", tmp.path());

        let p = default_install_path("claude-code", "foo").expect("claude-code is known");
        assert_eq!(p, tmp.path().join(".claude").join("skills").join("foo"));

        std::env::remove_var("HOME");
    }

    #[test]
    fn default_install_path_unknown_harness_is_none() {
        assert!(default_install_path("bogus", "foo").is_none());
        assert!(default_install_path("", "foo").is_none());
    }

    #[test]
    #[serial]
    fn default_install_path_without_home_is_none() {
        let prev = std::env::var("HOME").ok();
        std::env::remove_var("HOME");
        let result = default_install_path("claude-code", "foo");
        if let Some(value) = prev {
            std::env::set_var("HOME", value);
        }
        assert!(result.is_none(), "expected None when HOME is unset, got {:?}", result);
    }

    #[test]
    fn is_valid_skill_name_accepts_normal_names() {
        assert!(is_valid_skill_name("escalation-handling"));
        assert!(is_valid_skill_name("my_skill"));
        assert!(is_valid_skill_name("a"));
        assert!(is_valid_skill_name("v1.0.0-skill"));
    }

    #[test]
    fn is_valid_skill_name_rejects_path_traversal() {
        assert!(!is_valid_skill_name("../foo"));
        assert!(!is_valid_skill_name("foo/../bar"));
        assert!(!is_valid_skill_name(".."));
        assert!(!is_valid_skill_name("."));
        assert!(!is_valid_skill_name("foo/bar"));
        assert!(!is_valid_skill_name("foo\\bar"));
        assert!(!is_valid_skill_name(""));
        assert!(!is_valid_skill_name("foo\0bar"));
    }

    #[test]
    #[serial]
    fn resolve_install_path_rejects_traversal() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("HOME", tmp.path());

        assert!(resolve_install_path("claude-code", "../escape").is_none());
        assert!(resolve_install_path("claude-code", "good-name").is_some());

        std::env::remove_var("HOME");
    }

    #[test]
    #[serial]
    fn install_root_returns_parent_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("HOME", tmp.path());

        let root = install_root("claude-code").expect("known harness");
        assert_eq!(root, tmp.path().join(".claude").join("skills"));

        let root = install_root("codex-cli").expect("known harness");
        assert_eq!(root, tmp.path().join(".codex").join("agents"));

        std::env::remove_var("HOME");
    }

    #[test]
    #[serial]
    fn is_under_install_root_detects_membership() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("HOME", tmp.path());

        let in_root = tmp.path().join(".claude").join("skills").join("foo");
        assert!(is_under_install_root("claude-code", &in_root));
        assert!(!is_under_install_root("claude-code", &tmp.path().join("other")));

        std::env::remove_var("HOME");
    }
}
