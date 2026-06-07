use crate::error::Result;
use crate::schema::Skill;
use std::fs;
use std::path::Path;

pub fn parse_skill_yaml(path: &Path) -> Result<Skill> {
    let content = fs::read_to_string(path)?;
    let skill: Skill = serde_yaml::from_str(&content)?;
    Ok(skill)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_parse_valid_yaml() {
        let dir = temp_dir();
        let path = dir.path().join("skill.yaml");
        fs::write(&path, r#"
name: test-skill
version: 1.0.0
description: a test skill
author: tester
license: MIT
tags: [test, example]
harnesses:
  claude-code: ">=0.1"
requires:
  - some-dependency
entry: SKILL.md
config:
  timeout: 300
  temperature: 0.3
"#).unwrap();

        let skill = parse_skill_yaml(&path).unwrap();
        assert_eq!(skill.name, "test-skill");
        assert_eq!(skill.version, "1.0.0");
        assert_eq!(skill.tags, vec!["test", "example"]);
        assert_eq!(skill.harnesses.get("claude-code").unwrap(), ">=0.1");
        assert_eq!(skill.requires, vec!["some-dependency"]);
        assert!(skill.config.is_some());
        assert_eq!(skill.config.as_ref().unwrap().timeout, Some(300));
    }

    #[test]
    fn test_parse_minimal_yaml() {
        let dir = temp_dir();
        let path = dir.path().join("skill.yaml");
        fs::write(&path, r#"
name: minimal-skill
version: 0.1.0
"#).unwrap();

        let skill = parse_skill_yaml(&path).unwrap();
        assert_eq!(skill.name, "minimal-skill");
        assert_eq!(skill.description, None);
        assert_eq!(skill.entry, "SKILL.md");
        assert!(skill.harnesses.is_empty());
        assert!(skill.requires.is_empty());
        assert!(skill.config.is_none());
    }

    #[test]
    fn test_parse_invalid_yaml() {
        let dir = temp_dir();
        let path = dir.path().join("skill.yaml");
        fs::write(&path, "name: [invalid").unwrap();
        assert!(parse_skill_yaml(&path).is_err());
    }

    #[test]
    fn test_parse_unknown_fields_ignored() {
        let dir = temp_dir();
        let path = dir.path().join("skill.yaml");
        fs::write(&path, r#"
name: graceful-skill
version: 1.0.0
unknown_field: will be ignored
another_unknown:
  nested: true
"#).unwrap();

        let skill = parse_skill_yaml(&path).unwrap();
        assert_eq!(skill.name, "graceful-skill");
    }
}
