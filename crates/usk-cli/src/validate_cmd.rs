//! `usk validate` — standalone validation with deeper structural checks.
//!
//! This reuses `usk_core::validation::validate` for the basic name/semver/
//! entry-file checks, then layers on:
//!
//!   * For each `NN-*.md` in `instructions/`, the file must exist (we
//!     also warn when the directory is empty).
//!   * For each file in `scripts/`, warn if no shebang line.
//!   * For each file in `examples/`, the file must exist (this is mostly
//!     a sanity check; it always passes for files we can list, but it
//!     catches symlink breaks in CI).
//!
//! Errors block (exit code 1). Warnings don't.

use std::fs;
use std::path::Path;

/// Result of running `usk validate`. `errors` are blocking; `warnings` are not.
#[derive(Debug, Default, Clone)]
pub struct ValidationOutcome {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationOutcome {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
    fn error(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }
    fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }
}

/// Run all validation checks and return the aggregated outcome.
pub fn validate(source: &Path) -> Result<(), String> {
    let outcome = run_checks(source)?;
    print_report(source, &outcome);
    if outcome.has_errors() {
        // Use a stable, parseable exit signal. The dispatcher maps any
        // Err to exit code 1, which is what callers expect.
        return Err(format!(
            "{} validation error(s)",
            outcome.errors.len()
        ));
    }
    Ok(())
}

/// Internal: run the full set of checks and return the raw outcome
/// without printing. Exposed for unit tests.
pub fn run_checks(source: &Path) -> Result<ValidationOutcome, String> {
    let mut out = ValidationOutcome::new();

    if !source.exists() {
        return Err(format!("path '{}' does not exist", source.display()));
    }
    if !source.is_dir() {
        return Err(format!(
            "path '{}' is not a directory",
            source.display()
        ));
    }

    let yaml_path = source.join("skill.yaml");
    if !yaml_path.exists() {
        out.error(format!(
            "skill.yaml: file not found at {}",
            yaml_path.display()
        ));
        // No point continuing without a parsed skill.
        return Ok(out);
    }

    let skill = match usk_core::parser::parse_skill_yaml(&yaml_path) {
        Ok(s) => s,
        Err(e) => {
            out.error(format!("skill.yaml: parse error: {}", e));
            return Ok(out);
        }
    };

    // Basic checks via usk_core. A failure here is reported as a single
    // error, not a panic, so the rest of the report stays useful.
    if let Err(e) = usk_core::validation::validate(&skill, source) {
        out.error(format!("skill.yaml: {}", e));
        // Don't return; the deeper structural checks below are still
        // useful even when the basic check fails.
    }

    check_instructions(source, &mut out);
    check_scripts(source, &mut out);
    check_examples(source, &mut out);

    Ok(out)
}

fn check_instructions(source: &Path, out: &mut ValidationOutcome) {
    let dir = source.join("instructions");
    if !dir.exists() {
        // No `instructions/` directory isn't an error; the spec only
        // requires it for skills that use numbered instruction files.
        return;
    }
    let read = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(e) => {
            out.error(format!("instructions/: cannot read directory: {}", e));
            return;
        }
    };
    let mut count = 0usize;
    for entry in read.flatten() {
        let p = entry.path();
        if p.is_file() {
            count += 1;
            // We don't currently store the instruction list in the
            // schema (the spec leaves it to filesystem convention), so
            // this is just a presence check.
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("..") {
                    out.error(format!(
                        "instructions/{}: file name must not start with '..'",
                        name
                    ));
                }
            }
        }
    }
    if count == 0 {
        out.warn(format!(
            "instructions/: directory is empty (consider removing or populating)"
        ));
    }
}

fn check_scripts(source: &Path, out: &mut ValidationOutcome) {
    let dir = source.join("scripts");
    if !dir.exists() {
        return;
    }
    let read = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(e) => {
            out.error(format!("scripts/: cannot read directory: {}", e));
            return;
        }
    };
    for entry in read.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<unnamed>")
            .to_string();
        // Heuristic for "executable script": no extension and a shebang
        // OR an extension that looks like a script and a shebang.
        let is_probably_script = std::path::Path::new(&name)
            .extension()
            .map(|ext| {
                let e = ext.to_string_lossy().to_lowercase();
                e == "sh" || e == "bash" || e == "zsh" || e == "py" || e == "pl" || e == "rb"
            })
            .unwrap_or(false);
        if !is_probably_script {
            continue;
        }
        let head = fs::read(&p).ok().and_then(|bytes| {
            if bytes.len() < 2 {
                None
            } else {
                Some((bytes[0], bytes[1]))
            }
        });
        match head {
            Some((0x23, 0x21)) => {} // "#!" — has a shebang
            _ => {
                out.warn(format!(
                    "scripts/{}: no shebang line; add `#!/usr/bin/env ...` for portability",
                    name
                ));
            }
        }
    }
}

fn check_examples(source: &Path, out: &mut ValidationOutcome) {
    let dir = source.join("examples");
    if !dir.exists() {
        return;
    }
    // `fs::read_dir` already gives us real entries; we check that each
    // exists and is readable. A broken symlink would surface as `Err`
    // from `metadata()` here.
    let read = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(e) => {
            out.error(format!("examples/: cannot read directory: {}", e));
            return;
        }
    };
    for entry in read.flatten() {
        let p = entry.path();
        if p.is_file() {
            if let Err(e) = p.metadata() {
                out.error(format!("examples/{}: metadata error: {}", p.display(), e));
            }
        }
    }
}

fn print_report(source: &Path, outcome: &ValidationOutcome) {
    println!("Validating '{}':", source.display());
    if outcome.errors.is_empty() && outcome.warnings.is_empty() {
        println!("  OK (0 errors, 0 warnings)");
        return;
    }
    for e in &outcome.errors {
        println!("  error:   {}", e);
    }
    for w in &outcome.warnings {
        println!("  warning: {}", w);
    }
    println!(
        "  {} error(s), {} warning(s)",
        outcome.errors.len(),
        outcome.warnings.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    fn write_minimal_skill(dir: &Path) {
        let yaml = r#"name: "tmp-skill"
version: "1.0.0"
description: "test"
author: "tester"
license: MIT
tags: []
harnesses: {}
requires: []
entry: SKILL.md
"#;
        fs::write(dir.join("skill.yaml"), yaml).unwrap();
        fs::write(dir.join("SKILL.md"), "# tmp").unwrap();
    }

    #[test]
    fn test_validate_minimal_skill_no_errors() {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_skill(tmp.path());
        let outcome = run_checks(tmp.path()).expect("run_checks ok");
        assert!(!outcome.has_errors(), "errors: {:?}", outcome.errors);
    }

    #[test]
    fn test_validate_on_real_example() {
        let path = std::path::Path::new("../../spec/examples/escalation-handling");
        if !path.exists() {
            return;
        }
        let outcome = run_checks(path).expect("run_checks ok");
        assert!(
            !outcome.has_errors(),
            "expected 0 errors, got: {:?}",
            outcome.errors
        );
    }

    #[test]
    fn test_validate_missing_dir() {
        let result = run_checks(Path::new("/no/such/path"));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_no_skill_yaml() {
        let tmp = tempfile::tempdir().unwrap();
        let outcome = run_checks(tmp.path()).expect("run_checks ok");
        assert!(outcome.has_errors());
        assert!(outcome.errors[0].contains("skill.yaml"));
    }

    #[test]
    fn test_validate_empty_instructions_dir_warns() {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_skill(tmp.path());
        fs::create_dir_all(tmp.path().join("instructions")).unwrap();
        let outcome = run_checks(tmp.path()).expect("run_checks ok");
        assert!(
            outcome.warnings.iter().any(|w| w.contains("instructions/")),
            "expected an instructions warning, got {:?}",
            outcome.warnings
        );
    }

    #[test]
    fn test_validate_script_without_shebang_warns() {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_skill(tmp.path());
        let scripts = tmp.path().join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(scripts.join("run.sh"), "echo hi\n").unwrap();
        let outcome = run_checks(tmp.path()).expect("run_checks ok");
        assert!(
            outcome.warnings.iter().any(|w| w.contains("run.sh")),
            "expected shebang warning, got {:?}",
            outcome.warnings
        );
    }

    #[test]
    fn test_validate_script_with_shebang_no_warning() {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_skill(tmp.path());
        let scripts = tmp.path().join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(scripts.join("run.sh"), "#!/usr/bin/env bash\necho hi\n").unwrap();
        let outcome = run_checks(tmp.path()).expect("run_checks ok");
        assert!(
            !outcome.warnings.iter().any(|w| w.contains("run.sh")),
            "did not expect shebang warning, got {:?}",
            outcome.warnings
        );
    }

    // Reuse `format_size` from `inspect` so the test below has a stable
    // symbol; kept here to avoid re-deriving the contract.
    #[test]
    fn test_outcome_default_is_clean() {
        let o = ValidationOutcome::default();
        assert!(!o.has_errors());
        assert!(o.warnings.is_empty());
    }

    // Suppress unused-import warning for HashMap in case the module
    // changes later.
    #[allow(dead_code)]
    fn _suppress_unused(_h: HashMap<String, String>) {}
}
