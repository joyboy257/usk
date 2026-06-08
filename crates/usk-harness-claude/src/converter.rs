use std::fs;
use std::path::{Path, PathBuf};
use usk_core::schema::Skill;
use usk_harness_core::adapter::HarnessAdapter;
use usk_harness_core::error::Result;

pub struct ClaudeCodeAdapter;

impl HarnessAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &str {
        "claude-code"
    }

    fn install_root(&self) -> Option<PathBuf> {
        usk_harness_core::paths::install_root("claude-code")
    }

    fn convert(&self, skill: &Skill, source_dir: &Path, output_dir: &Path) -> Result<()> {
        // Create output directory
        fs::create_dir_all(output_dir)?;

        // Copy SKILL.md (the main instruction document)
        let entry_path = source_dir.join(&skill.entry);
        let source_body = if entry_path.exists() {
            fs::read_to_string(&entry_path)?
        } else {
            String::new()
        };

        let skilly_content = format!(
            "# {name}\n\n{description}\n\n{body}",
            name = skill.name,
            description = skill.description.as_deref().unwrap_or(""),
            body = source_body
        );
        fs::write(output_dir.join("SKILL.md"), skilly_content)?;

        // Copy supporting directories
        for dir_name in &["examples", "templates", "scripts", "references", "instructions"] {
            let src = source_dir.join(dir_name);
            if src.exists() {
                copy_dir_all(&src, &output_dir.join(dir_name))?;
            }
        }

        Ok(())
    }

    fn supported_versions(&self) -> &[semver::VersionReq] {
        static VERSIONS: once_cell::sync::Lazy<Vec<semver::VersionReq>> =
            once_cell::sync::Lazy::new(|| {
                vec![semver::VersionReq::parse(">=0.1").unwrap()]
            });
        &VERSIONS
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    fn test_skill() -> (Skill, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let skill = Skill {
            name: "test-skill".to_string(),
            version: "1.0.0".to_string(),
            description: Some("A test skill".to_string()),
            author: Some("tester".to_string()),
            license: Some("MIT".to_string()),
            tags: vec!["test".to_string()],
            harnesses: HashMap::new(),
            requires: vec![],
            entry: "SKILL.md".to_string(),
            config: None,
        };
        fs::write(dir.path().join("SKILL.md"), "# Instructions\n\nDo the thing.").unwrap();
        (skill, dir)
    }

    #[test]
    fn test_convert_basic_skill() {
        let (skill, src_dir) = test_skill();
        let dst_dir = tempfile::tempdir().unwrap();

        let adapter = ClaudeCodeAdapter;
        adapter.convert(&skill, src_dir.path(), dst_dir.path()).unwrap();

        assert!(dst_dir.path().join("SKILL.md").exists());
        let content = fs::read_to_string(dst_dir.path().join("SKILL.md")).unwrap();
        assert!(content.contains("test-skill"));
        assert!(content.contains("A test skill"));
        assert!(content.contains("Do the thing."));
    }

    #[test]
    fn test_convert_creates_output_dir() {
        let (skill, src_dir) = test_skill();
        let tmp = tempfile::tempdir().unwrap();
        let dst_dir = tmp.path().join("nested").join("output");

        let adapter = ClaudeCodeAdapter;
        adapter.convert(&skill, src_dir.path(), &dst_dir).unwrap();
        assert!(dst_dir.join("SKILL.md").exists());
    }

    #[test]
    fn test_convert_with_minimal_skill() {
        let dir = tempfile::tempdir().unwrap();
        let skill = Skill {
            name: "minimal".to_string(),
            version: "0.1.0".to_string(),
            description: None,
            author: None,
            license: None,
            tags: vec![],
            harnesses: HashMap::new(),
            requires: vec![],
            entry: "SKILL.md".to_string(),
            config: None,
        };
        fs::write(dir.path().join("SKILL.md"), "do it").unwrap();
        let dst = tempfile::tempdir().unwrap();

        let adapter = ClaudeCodeAdapter;
        adapter.convert(&skill, dir.path(), dst.path()).unwrap();
        assert!(dst.path().join("SKILL.md").exists());
    }

    #[test]
    fn test_convert_copies_supporting_dirs() {
        let (skill, src_dir) = test_skill();
        let examples_dir = src_dir.path().join("examples");
        fs::create_dir_all(&examples_dir).unwrap();
        fs::write(examples_dir.join("example1.md"), "# Example 1").unwrap();

        let dst_dir = tempfile::tempdir().unwrap();
        let adapter = ClaudeCodeAdapter;
        adapter.convert(&skill, src_dir.path(), dst_dir.path()).unwrap();

        assert!(dst_dir.path().join("examples").join("example1.md").exists());
    }
}
