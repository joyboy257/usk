//! `usk convert` — preview the harness adapter output for a local skill.
//!
//! This is a dry-run: it parses `skill.yaml` from the given directory,
//! runs the named harness's `HarnessAdapter::convert` against `--out`,
//! and prints a summary of what was written. It does NOT touch
//! `config.installed` — that's the `usk install` flow's job.

use std::path::Path;
use usk_harness_core::adapter::HarnessAdapter;

/// Run a harness conversion and print a summary.
///
/// `source` must be a skill directory containing `skill.yaml`. `out` is
/// created if it doesn't exist. `harness` is one of the keys in
/// `usk_harness_core::discovery::KNOWN_HARNESSES`.
pub fn convert(source: &Path, harness: &str, out: &Path) -> Result<(), String> {
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
        return Err(format!(
            "no skill.yaml found in '{}'",
            source.display()
        ));
    }
    let skill = usk_core::parser::parse_skill_yaml(&yaml_path)
        .map_err(|e| format!("failed to parse skill.yaml: {}", e))?;
    if let Err(e) = usk_core::validation::validate(&skill, source) {
        return Err(format!("validation failed: {}", e));
    }

    if !usk_harness_core::discovery::is_known(harness) {
        let known: Vec<String> = usk_harness_core::discovery::KNOWN_HARNESSES
            .iter()
            .map(|h| h.key.to_string())
            .collect();
        return Err(format!(
            "unknown harness '{}'. Known harnesses: {}",
            harness,
            known.join(", ")
        ));
    }

    // Build the adapter. We dispatch on the harness key — same pattern
    // as the install flow in `main.rs` — so the `convert` function here
    // works without depending on private adapter state in `commands`.
    let summary = match harness {
        "claude-code" => {
            let adapter = usk_harness_claude::converter::ClaudeCodeAdapter;
            adapter
                .convert(&skill, source, out)
                .map_err(|e: usk_harness_core::error::HarnessError| e.to_string())?;
            summarize_output(out)?
        }
        "codex-cli" => {
            let adapter = usk_harness_codex::converter::CodexCliAdapter;
            adapter
                .convert(&skill, source, out)
                .map_err(|e: usk_harness_core::error::HarnessError| e.to_string())?;
            summarize_output(out)?
        }
        // `is_known` already filtered these out, but keep the catch-all
        // for future harness additions.
        other => {
            return Err(format!(
                "harness '{}' is recognized but has no built-in converter in this build",
                other
            ))
        }
    };

    println!("Converted '{}' to {}:", skill.name, harness);
    for line in summary {
        println!("  {}", line);
    }
    println!("Output: {}", out.display());
    Ok(())
}

/// Walk `out` and produce a short human summary of what was written.
fn summarize_output(out: &Path) -> Result<Vec<String>, String> {
    let mut lines: Vec<String> = Vec::new();
    if !out.exists() {
        lines.push("(no output produced)".to_string());
        return Ok(lines);
    }
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    collect_top_level(out, &mut files, &mut dirs)?;
    // Stable order for deterministic output (and stable tests).
    files.sort();
    dirs.sort();
    for f in files {
        let rel = f
            .strip_prefix(out)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        let size = f.metadata().map(|m| m.len()).unwrap_or(0);
        lines.push(format!("{} ({})", rel, crate::inspect::format_size(size)));
    }
    for d in dirs {
        let rel = d
            .strip_prefix(out)
            .unwrap_or(&d)
            .to_string_lossy()
            .replace('\\', "/");
        let (count, bytes) = crate::inspect::summarize_dir(&d);
        lines.push(format!(
            "{}/ ({} files, {})",
            rel.trim_end_matches('/'),
            count,
            crate::inspect::format_size(bytes)
        ));
    }
    if lines.is_empty() {
        lines.push("(no output produced)".to_string());
    }
    Ok(lines)
}

/// Collect top-level files and directories in `root`.
fn collect_top_level(
    root: &Path,
    files: &mut Vec<std::path::PathBuf>,
    dirs: &mut Vec<std::path::PathBuf>,
) -> Result<(), String> {
    let rd = std::fs::read_dir(root).map_err(|e| format!("read_dir: {}", e))?;
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            dirs.push(p);
        } else {
            files.push(p);
        }
    }
    Ok(())
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
        fs::write(dir.join("SKILL.md"), "# tmp skill\n\nbody.").unwrap();
    }

    #[test]
    fn test_convert_writes_files_to_out() {
        let src = tempfile::tempdir().unwrap();
        write_minimal_skill(src.path());

        let dst = tempfile::tempdir().unwrap();
        let out_dir = dst.path().join("preview");
        // Don't pre-create; the function should handle creation.
        let result = convert(src.path(), "claude-code", &out_dir);
        assert!(result.is_ok(), "convert failed: {:?}", result.err());
        // claude-code writes SKILL.md.
        assert!(out_dir.join("SKILL.md").exists());
    }

    #[test]
    fn test_convert_creates_out_dir() {
        let src = tempfile::tempdir().unwrap();
        write_minimal_skill(src.path());
        let dst = tempfile::tempdir().unwrap();
        let nested = dst.path().join("a").join("b").join("c");
        let result = convert(src.path(), "claude-code", &nested);
        assert!(result.is_ok());
        assert!(nested.exists());
    }

    #[test]
    fn test_convert_unknown_harness_errors() {
        let src = tempfile::tempdir().unwrap();
        write_minimal_skill(src.path());
        let dst = tempfile::tempdir().unwrap();
        let result = convert(src.path(), "bogus-harness", dst.path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("unknown harness"), "got: {}", err);
    }

    #[test]
    fn test_convert_no_skill_yaml_errors() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let result = convert(src.path(), "claude-code", dst.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no skill.yaml"));
    }

    #[test]
    fn test_convert_to_codex_writes_agent_yaml() {
        let src = tempfile::tempdir().unwrap();
        write_minimal_skill(src.path());
        let dst = tempfile::tempdir().unwrap();
        let out_dir = dst.path().join("preview");
        let result = convert(src.path(), "codex-cli", &out_dir);
        assert!(result.is_ok(), "convert failed: {:?}", result.err());
        assert!(out_dir.join("agent.yaml").exists());
    }

    // Make `HashMap` import not flagged as dead — it's here in case
    // the file gets a refactor that needs it.
    #[allow(dead_code)]
    fn _h(_h: HashMap<String, String>) {}
}
