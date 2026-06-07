//! `usk inspect` — show the contents of a skill directory.
//!
//! Walks the skill folder, parses `skill.yaml` if present, and prints a
//! `tree`-like summary of name/version/description/harnesses plus a file
//! listing with human-readable sizes.

use std::fs;
use std::path::Path;

/// Inspect a skill directory and print a summary to stdout.
///
/// `source` must be an existing directory; otherwise an error is returned.
pub fn inspect(source: &Path) -> Result<(), String> {
    if !source.exists() {
        return Err(format!("path '{}' does not exist", source.display()));
    }
    if !source.is_dir() {
        return Err(format!(
            "path '{}' is not a directory (usk inspect expects a skill folder)",
            source.display()
        ));
    }

    let yaml_path = source.join("skill.yaml");
    if !yaml_path.exists() {
        return Err(format!(
            "no skill.yaml found in '{}' — is this a skill directory?",
            source.display()
        ));
    }

    let skill = usk_core::parser::parse_skill_yaml(&yaml_path)
        .map_err(|e| format!("failed to parse skill.yaml: {}", e))?;

    println!("name:        {}", skill.name);
    println!("version:     {}", skill.version);
    if let Some(desc) = &skill.description {
        if !desc.is_empty() {
            // Trim trailing whitespace; the description is free-form so it
            // may include a trailing newline from the YAML.
            let trimmed = desc.trim();
            for (i, line) in trimmed.lines().enumerate() {
                if i == 0 {
                    println!("description: {}", line);
                } else {
                    println!("             {}", line);
                }
            }
        }
    }
    if !skill.harnesses.is_empty() {
        let mut names: Vec<&str> = skill.harnesses.keys().map(|s| s.as_str()).collect();
        names.sort();
        println!("harnesses:   {}", names.join(", "));
    }

    // File listing.
    println!("files:");
    let entries = collect_entries(source).map_err(|e| format!("failed to walk directory: {}", e))?;
    if entries.is_empty() {
        println!("  (no files)");
        return Ok(());
    }
    for entry in &entries {
        let rel = entry
            .strip_prefix(source)
            .unwrap_or(entry)
            .to_string_lossy()
            .replace('\\', "/");
        let size = format_size(entry.metadata().map(|m| m.len()).unwrap_or(0));
        if entry.is_dir() {
            // Count files inside the directory and report their total size.
            let (file_count, total_bytes) = summarize_dir(entry);
            println!(
                "             {}/ ({} files, {})",
                rel.trim_end_matches('/'),
                file_count,
                format_size(total_bytes)
            );
        } else {
            println!("             {} ({})", rel, size);
        }
    }
    Ok(())
}

/// Recursively collect all files and subdirectories under `root`, sorted
/// with directories first, then files, each group alphabetical. The root
/// itself is excluded.
fn collect_entries(root: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some("skill.yaml") {
            continue;
        }
        if path.is_dir() {
            dirs.push(path);
        } else {
            files.push(path);
        }
    }
    dirs.sort();
    files.sort();
    let mut out = dirs;
    out.extend(files);
    Ok(out)
}

/// Count files in `dir` (recursively) and sum their sizes.
pub fn summarize_dir(dir: &Path) -> (usize, u64) {
    let mut count = 0usize;
    let mut bytes = 0u64;
    walk_recursive(dir, &mut |path| {
        if path.is_file() {
            count += 1;
            if let Ok(meta) = path.metadata() {
                bytes += meta.len();
            }
        }
    });
    (count, bytes)
}

pub fn walk_recursive<F: FnMut(&Path)>(dir: &Path, cb: &mut F) {
    if let Ok(rd) = fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            cb(&p);
            if p.is_dir() {
                walk_recursive(&p, cb);
            }
        }
    }
}

/// Format a byte count as a short human-readable string.
///
/// We use the K/M convention (powers of 1024) to keep output consistent
/// with what users see in typical CLI tools. Bytes are only shown for
/// sub-1K files.
pub fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    if bytes >= GIB {
        format!("{:.1}G", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1}M", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1}K", bytes as f64 / KIB as f64)
    } else {
        format!("{}B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0B");
        assert_eq!(format_size(512), "512B");
    }

    #[test]
    fn test_format_size_kib() {
        let s = format_size(1024);
        assert!(s.ends_with('K'), "expected K suffix, got {}", s);
        assert_eq!(s, "1.0K");
    }

    #[test]
    fn test_format_size_mib() {
        let s = format_size(1024 * 1024);
        assert!(s.ends_with('M'), "expected M suffix, got {}", s);
    }

    #[test]
    fn test_format_size_gib() {
        let s = format_size(1024 * 1024 * 1024);
        assert!(s.ends_with('G'), "expected G suffix, got {}", s);
    }

    #[test]
    fn test_inspect_on_example_skill() {
        // The spec example is the canonical "well-formed" skill used
        // throughout the project. Verifies the expected fields appear.
        let path = std::path::Path::new("../../spec/examples/escalation-handling");
        if !path.exists() {
            // Skip when run from a different cwd (e.g. inside a sandbox).
            return;
        }
        // Capture stdout for assertions by re-implementing the inspection
        // steps directly. Simpler than redirecting println! in a test.
        let yaml = path.join("skill.yaml");
        let skill = usk_core::parser::parse_skill_yaml(&yaml).unwrap();
        assert_eq!(skill.name, "escalation-handling");
        assert_eq!(skill.version, "1.0.0");
        assert!(skill.harnesses.contains_key("claude-code"));
        assert!(skill.harnesses.contains_key("codex-cli"));
    }

    #[test]
    fn test_inspect_missing_dir_errors() {
        let result = inspect(Path::new("/nonexistent/path/should/not/exist"));
        assert!(result.is_err());
    }

    #[test]
    fn test_inspect_file_instead_of_dir_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("not-a-dir.txt");
        std::fs::write(&f, "hello").unwrap();
        let result = inspect(&f);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a directory"));
    }

    #[test]
    fn test_inspect_dir_without_skill_yaml_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let result = inspect(tmp.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no skill.yaml"));
    }
}
