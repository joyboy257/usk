use std::collections::HashMap;
use std::fs;
use std::path::Path;
use usk_core::schema::Skill;
use usk_harness_core::adapter::HarnessAdapter;
use usk_harness_core::error::Result;

#[derive(serde::Serialize)]
struct CodexAgent {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    instructions: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    examples: Vec<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config: Option<CodexConfig>,
}

#[derive(serde::Serialize)]
struct CodexConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<u64>,
}

pub struct CodexCliAdapter;

impl HarnessAdapter for CodexCliAdapter {
    fn name(&self) -> &str {
        "codex-cli"
    }

    fn convert(&self, skill: &Skill, source_dir: &Path, output_dir: &Path) -> Result<()> {
        fs::create_dir_all(output_dir)?;

        // Read instructions
        let instructions = if source_dir.join(&skill.entry).exists() {
            fs::read_to_string(source_dir.join(&skill.entry))?
        } else {
            String::new()
        };

        // Read examples
        let mut examples = Vec::new();
        let examples_dir = source_dir.join("examples");
        if examples_dir.exists() {
            for entry in fs::read_dir(&examples_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    let content = fs::read_to_string(entry.path())?;
                    let mut example = HashMap::new();
                    example.insert(
                        entry.path().file_stem().unwrap().to_string_lossy().to_string(),
                        content,
                    );
                    examples.push(example);
                }
            }
        }

        // Collect scripts as tool names
        let mut tools = Vec::new();
        let scripts_dir = source_dir.join("scripts");
        if scripts_dir.exists() {
            for entry in fs::read_dir(&scripts_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    if let Some(name) = entry.path().file_stem() {
                        tools.push(name.to_string_lossy().to_string());
                    }
                }
            }
        }

        let agent = CodexAgent {
            name: skill.name.clone(),
            description: skill.description.clone(),
            instructions,
            examples,
            tools,
            config: skill.config.as_ref().map(|c| CodexConfig {
                temperature: c.temperature,
                timeout: c.timeout,
            }),
        };

        let yaml = serde_yaml::to_string(&agent)?;
        fs::write(output_dir.join("agent.yaml"), yaml)?;

        // Copy scripts to output
        if scripts_dir.exists() {
            let dst_scripts = output_dir.join("scripts");
            fs::create_dir_all(&dst_scripts)?;
            for entry in fs::read_dir(&scripts_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    fs::copy(entry.path(), dst_scripts.join(entry.file_name()))?;
                }
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
        fs::write(dir.path().join("SKILL.md"), "Do the work.").unwrap();
        (skill, dir)
    }

    #[test]
    fn test_convert_basic() {
        let (skill, src_dir) = test_skill();
        let dst_dir = tempfile::tempdir().unwrap();

        let adapter = CodexCliAdapter;
        adapter.convert(&skill, src_dir.path(), dst_dir.path()).unwrap();

        let agent_path = dst_dir.path().join("agent.yaml");
        assert!(agent_path.exists());
        let content = fs::read_to_string(agent_path).unwrap();
        assert!(content.contains("name: test-skill"));
        assert!(content.contains("Do the work."));
    }

    #[test]
    fn test_convert_with_examples() {
        let (skill, src_dir) = test_skill();
        let examples_dir = src_dir.path().join("examples");
        fs::create_dir_all(&examples_dir).unwrap();
        fs::write(examples_dir.join("example1.md"), "Input: X -> Output: Y").unwrap();

        let dst_dir = tempfile::tempdir().unwrap();
        let adapter = CodexCliAdapter;
        adapter.convert(&skill, src_dir.path(), dst_dir.path()).unwrap();

        let content = fs::read_to_string(dst_dir.path().join("agent.yaml")).unwrap();
        assert!(content.contains("examples:"));
    }

    #[test]
    fn test_convert_with_scripts() {
        let (skill, src_dir) = test_skill();
        let scripts_dir = src_dir.path().join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        fs::write(scripts_dir.join("lookup.sh"), "echo looking up").unwrap();

        let dst_dir = tempfile::tempdir().unwrap();
        let adapter = CodexCliAdapter;
        adapter.convert(&skill, src_dir.path(), dst_dir.path()).unwrap();

        let content = fs::read_to_string(dst_dir.path().join("agent.yaml")).unwrap();
        assert!(content.contains("lookup"));
        assert!(dst_dir.path().join("scripts").join("lookup.sh").exists());
    }

    #[test]
    fn test_convert_without_examples() {
        let (skill, src_dir) = test_skill();
        let dst_dir = tempfile::tempdir().unwrap();

        let adapter = CodexCliAdapter;
        adapter.convert(&skill, src_dir.path(), dst_dir.path()).unwrap();

        let content = fs::read_to_string(dst_dir.path().join("agent.yaml")).unwrap();
        // Should have no examples key (or empty: [])
        // serde skips serialization for empty Vec with skip_serializing_if
        assert!(!content.contains("examples:"));
    }

    #[test]
    fn test_convert_with_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut harnesses = HashMap::new();
        harnesses.insert("codex-cli".to_string(), ">=0.1".to_string());
        let skill = Skill {
            name: "configured".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            license: None,
            tags: vec![],
            harnesses,
            requires: vec![],
            entry: "SKILL.md".to_string(),
            config: Some(usk_core::schema::SkillConfig {
                timeout: Some(500),
                temperature: Some(0.2),
            }),
        };
        fs::write(dir.path().join("SKILL.md"), "work").unwrap();

        let dst_dir = tempfile::tempdir().unwrap();
        let adapter = CodexCliAdapter;
        adapter.convert(&skill, dir.path(), dst_dir.path()).unwrap();

        let content = fs::read_to_string(dst_dir.path().join("agent.yaml")).unwrap();
        assert!(content.contains("temperature: 0.2"));
        assert!(content.contains("timeout: 500"));
    }
}
