//! `usk doctor` — diagnose USK setup.
//!
//! Walks the candidate install paths for each known harness (via
//! `usk_harness_core::paths::install_root`), checks for write access,
//! lists skills present in each harness's view, and cross-references
//! them with `Config.installed` and the project lockfile. No LLM, no
//! subprocess — just filesystem probes.
//!
//! The output is a human-readable, scannable report with a final
//! summary. The function returns `Ok(())` after printing, even if
//! issues were found; it returns `Err` only for IO errors that make
//! the probe impossible (e.g., a permissions failure reading `HOME`).
//!
//! ## Output shape
//!
//! ```text
//! USK Doctor
//! ==========
//!
//! Harnesses:
//!   ✓ claude-code: ~/.claude/skills/ exists, writable
//!   ✓ codex-cli:   ~/.codex/agents/ exists, writable
//!
//! Installed skills (from config):
//!   ✓ escalation-handling v1.0.0 @ claude-code — projection present
//!
//! Lockfile (./usk.lock):
//!   2 entries — all on disk
//!
//! All checks passed. 1 skills installed, 1 enabled, all paths writable.
//! ```
//!
//! ## Implementation note
//!
//! The function is split into `build_report` (pure data, no printing)
//! and `print_report` (stdout). The public `doctor` is a thin wrapper
//! that does both; tests exercise `build_report` so they can assert on
//! the structured sections without capturing stdout.

use std::path::{Path, PathBuf};

use usk_core::config::Config;
use usk_core::lockfile::{LockEntry, Lockfile};
use usk_harness_core::paths;

/// Status markers used in the report lines.
const OK: &str = "✓";
const BAD: &str = "✗";
const WARN: &str = "⚠";

/// Run the diagnostic probe and print a human-readable report to
/// stdout. Returns `Ok(())` after printing — even when issues are
/// found. Returns `Err` only when the probe itself cannot run (e.g.,
/// `HOME` cannot be read).
pub fn doctor(config: &Config, lockfile: Option<&Lockfile>) -> Result<(), String> {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| "HOME is not set; cannot run doctor".to_string())?;
    let report = build_report(config, lockfile, &home)?;
    print_report(&report);
    Ok(())
}

fn build_report(
    config: &Config,
    lockfile: Option<&Lockfile>,
    home: &Path,
) -> Result<DoctorReport, String> {
    let harnesses = check_harnesses(config, home);
    let skills = check_installed_skills(config);
    let lockfile = lockfile.map(check_lockfile);
    let config_installed = config.installed.len();
    let config_on_disk = config
        .installed
        .values()
        .filter(|info| info.install_path.exists())
        .count();
    Ok(DoctorReport {
        harnesses,
        skills,
        lockfile,
        config_installed,
        config_on_disk,
    })
}

fn check_harnesses(config: &Config, home: &Path) -> Section {
    let mut lines: Vec<String> = Vec::new();
    let mut issues = 0usize;
    if config.harnesses.is_empty() {
        lines.push(format!(
            "{} no harnesses registered. Run `usk harness add <name>` to start.",
            WARN
        ));
        issues += 1;
    } else {
        let mut keys: Vec<&String> = config.harnesses.keys().collect();
        keys.sort();
        for key in keys {
            let (line, is_issue) = check_one_harness(key, home);
            if is_issue {
                issues += 1;
            }
            lines.push(line);
        }
    }
    Section {
        lines,
        issue_count: issues,
    }
}

fn check_one_harness(key: &str, home: &Path) -> (String, bool) {
    let root = match paths::install_root_in(home, key) {
        Some(p) => p,
        None => {
            return (
                format!(
                    "{} {}: no install path known (unknown harness key)",
                    BAD, key
                ),
                true,
            );
        }
    };
    if !root.exists() {
        return (
            format!(
                "{} {}: {} does not exist (will be created on first install)",
                WARN,
                key,
                display_path(&root)
            ),
            false,
        );
    }
    if !root.is_dir() {
        return (
            format!(
                "{} {}: {} exists but is not a directory",
                BAD,
                key,
                display_path(&root)
            ),
            true,
        );
    }
    match probe_writable(&root) {
        Ok(()) => (
            format!("{} {}: {} exists, writable", OK, key, display_path(&root)),
            false,
        ),
        Err(e) => (
            format!(
                "{} {}: {} exists, not writable ({})",
                BAD,
                key,
                display_path(&root),
                e
            ),
            true,
        ),
    }
}

/// One section of the report (e.g. "Harnesses" or "Installed skills").
#[derive(Debug, Default)]
struct Section {
    /// Pre-formatted report lines, ready to print.
    lines: Vec<String>,
    /// Count of hard issues (printed `✗` or unknown-harness errors).
    issue_count: usize,
}

/// Lockfile section. Carries the resolved path so the header can show
/// it (the lockfile path depends on `cwd` and `HOME`).
#[derive(Debug)]
struct LockfileSection {
    path: PathBuf,
    lines: Vec<String>,
    issue_count: usize,
}

/// The full doctor report. Held in one struct so the public print
/// function is a single call and tests can introspect all sections.
#[derive(Debug)]
struct DoctorReport {
    harnesses: Section,
    skills: Section,
    lockfile: Option<LockfileSection>,
    /// Number of entries in `Config.installed`.
    config_installed: usize,
    /// Number of `Config.installed` entries whose `install_path`
    /// currently exists on disk.
    config_on_disk: usize,
}

fn print_report(r: &DoctorReport) {
    println!("USK Doctor");
    println!("==========");
    println!();
    print_section("Harnesses:", &r.harnesses);
    print_section("Installed skills (from config):", &r.skills);
    match &r.lockfile {
        Some(lf) => {
            println!("Lockfile ({}):", display_path(&lf.path));
            for line in &lf.lines {
                println!("  {}", line);
            }
            println!();
        }
        None => {
            println!("Lockfile:");
            println!("  (not loaded)");
            println!();
        }
    }
    print_summary(r);
}

fn print_section(header: &str, section: &Section) {
    println!("{}", header);
    for line in &section.lines {
        println!("  {}", line);
    }
    println!();
}

fn print_summary(r: &DoctorReport) {
    let lf_issues = r.lockfile.as_ref().map(|l| l.issue_count).unwrap_or(0);
    let total = r.harnesses.issue_count + r.skills.issue_count + lf_issues;
    if total == 0 {
        if r.config_installed == 0 {
            println!("All checks passed. 0 skills installed, all harness paths writable.");
        } else {
            println!(
                "All checks passed. {} skills installed, {} enabled, all paths writable.",
                r.config_installed, r.config_on_disk
            );
        }
    } else {
        println!("Summary: {} issue(s) found.", total);
    }
}

fn check_installed_skills(config: &Config) -> Section {
    let mut lines: Vec<String> = Vec::new();
    let mut issues = 0usize;
    if config.installed.is_empty() {
        lines.push(format!(
            "{} no skills installed. Run `usk install <path>` to install your first skill.",
            WARN
        ));
    } else {
        let mut names: Vec<&String> = config.installed.keys().collect();
        names.sort();
        for name in names {
            let info = &config.installed[name];
            if info.install_path.exists() {
                lines.push(format!(
                    "{} {} v{} @ {} — projection present",
                    OK, name, info.version, info.harness
                ));
            } else {
                issues += 1;
                lines.push(format!(
                    "{} {} v{} @ {} — projection missing at {}",
                    BAD,
                    name,
                    info.version,
                    info.harness,
                    display_path(&info.install_path)
                ));
            }
        }
    }
    Section {
        lines,
        issue_count: issues,
    }
}

fn check_lockfile(lf: &Lockfile) -> LockfileSection {
    let path = default_lockfile_path();
    let mut lines: Vec<String> = Vec::new();
    let mut issues = 0usize;
    if lf.skill.is_empty() {
        lines.push("(empty)".to_string());
    } else {
        let mut missing: Vec<&LockEntry> = Vec::new();
        for entry in &lf.skill {
            if !entry.install_path.exists() {
                missing.push(entry);
                issues += 1;
            }
        }
        if issues == 0 {
            lines.push(format!(
                "{} {} — all on disk",
                lf.skill.len(),
                pluralize(lf.skill.len())
            ));
        } else {
            lines.push(format!(
                "{} {} — {} missing on disk",
                lf.skill.len(),
                pluralize(lf.skill.len()),
                missing.len()
            ));
            for entry in missing {
                lines.push(format!(
                    "  - {}: missing at {}",
                    entry.name,
                    display_path(&entry.install_path)
                ));
            }
        }
    }
    LockfileSection {
        path,
        lines,
        issue_count: issues,
    }
}

fn default_lockfile_path() -> PathBuf {
    std::env::current_dir()
        .map(|d| d.join("usk.lock"))
        .unwrap_or_else(|_| PathBuf::from("usk.lock"))
}

fn pluralize(n: usize) -> &'static str {
    if n == 1 {
        "entry"
    } else {
        "entries"
    }
}

/// Test whether `dir` is writable by creating a probe file and
/// removing it. Permission errors surface here; full-disk errors are
/// also possible.
fn probe_writable(dir: &Path) -> std::io::Result<()> {
    let probe = dir.join(".usk-doctor-write-probe");
    std::fs::write(&probe, b"usk doctor: write probe")?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// Display a path. If `HOME` is set and the path is under it, render
/// the path with a leading `~` so reports read naturally.
fn display_path(p: &Path) -> String {
    let s = p.display().to_string();
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() && s.starts_with(&home) {
            return format!("~{}", &s[home.len()..]);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use usk_core::config::InstalledSkill;

    fn default_config() -> Config {
        let mut harnesses = HashMap::new();
        harnesses.insert("claude-code".to_string(), "usk-harness-claude".to_string());
        harnesses.insert("codex-cli".to_string(), "usk-harness-codex".to_string());
        Config {
            registry_url: String::new(),
            install_dir: PathBuf::from("/tmp/usk-install"),
            harnesses,
            installed: HashMap::new(),
        }
    }

    fn harness_lines(r: &DoctorReport) -> Vec<&str> {
        r.harnesses.lines.iter().map(String::as_str).collect()
    }

    fn skill_lines(r: &DoctorReport) -> Vec<&str> {
        r.skills.lines.iter().map(String::as_str).collect()
    }

    /// Happy path: a fresh install where every known harness's install
    /// root already exists and is writable, and no skills are yet
    /// installed. Doctor should report 0 issues.
    #[test]
    fn doctor_reports_clean_install() {
        let home = tempfile::tempdir().expect("tempdir");
        // Pre-create both harness install roots so they are present
        // and writable.
        std::fs::create_dir_all(home.path().join(".claude").join("skills")).unwrap();
        std::fs::create_dir_all(home.path().join(".codex").join("agents")).unwrap();

        let config = default_config();
        let r =
            build_report(&config, None, home.path()).expect("report should build");

        let lines = harness_lines(&r);
        assert_eq!(
            lines.len(),
            2,
            "expected 2 harness lines, got: {:?}",
            lines
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("claude-code") && l.contains("writable")),
            "expected claude-code writable line, got: {:?}",
            lines
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("codex-cli") && l.contains("writable")),
            "expected codex-cli writable line, got: {:?}",
            lines
        );
        assert_eq!(
            r.harnesses.issue_count, 0,
            "clean install should have 0 harness issues"
        );
        assert_eq!(r.skills.issue_count, 0);
    }

    /// Edge case: the harness install root does not exist. Doctor
    /// should report it as `⚠` (will be created on first install),
    /// not as a hard issue.
    #[test]
    fn doctor_reports_missing_path() {
        let home = tempfile::tempdir().expect("tempdir");
        // No harness install root is created.

        let config = default_config();
        let r =
            build_report(&config, None, home.path()).expect("report should build");

        let lines = harness_lines(&r);
        assert!(
            lines.iter().any(|l| l.contains("does not exist")
                && l.contains("will be created on first install")),
            "expected missing-path warning, got: {:?}",
            lines
        );
        assert_eq!(
            r.harnesses.issue_count, 0,
            "missing path is a warning, not a hard issue"
        );
    }

    /// Edge case: the config has a skill whose `install_path` is gone.
    /// Doctor should mark it as a hard issue.
    #[test]
    fn doctor_reports_dangling_skill() {
        let home = tempfile::tempdir().expect("tempdir");
        let mut config = default_config();
        let install_path = home.path().join(".claude").join("skills").join("stale-skill");
        // Intentionally NOT creating the install path: doctor should
        // report the skill's projection as missing.
        config.installed.insert(
            "stale-skill".to_string(),
            InstalledSkill {
                version: "0.1.0".to_string(),
                harness: "claude-code".to_string(),
                install_path,
            },
        );
        let r =
            build_report(&config, None, home.path()).expect("report should build");

        let lines = skill_lines(&r);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("stale-skill") && l.contains("missing")),
            "expected dangling-skill line, got: {:?}",
            lines
        );
        assert_eq!(r.skills.issue_count, 1);
    }

    /// Edge case: the user has no harnesses registered. Doctor should
    /// say so explicitly.
    #[test]
    fn doctor_reports_no_harnesses() {
        let home = tempfile::tempdir().expect("tempdir");
        let config = Config {
            registry_url: String::new(),
            install_dir: PathBuf::from("/tmp/usk-install"),
            harnesses: HashMap::new(),
            installed: HashMap::new(),
        };
        let r =
            build_report(&config, None, home.path()).expect("report should build");

        let lines = harness_lines(&r);
        assert!(
            lines.iter().any(|l| l.contains("no harnesses registered")),
            "expected no-harnesses message, got: {:?}",
            lines
        );
    }
}
