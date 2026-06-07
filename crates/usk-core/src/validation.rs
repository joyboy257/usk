use crate::error::{Result, SkillError};
use crate::schema::Skill;
use std::path::Path;

pub fn validate(skill: &Skill, base_dir: &Path) -> Result<()> {
    if skill.name.is_empty() {
        return Err(SkillError::ValidationError(
            "skill name is required".to_string(),
        ));
    }

    if skill.version.is_empty() {
        return Err(SkillError::ValidationError(
            "skill version is required".to_string(),
        ));
    }

    // Validate semver
    let _ = semver::Version::parse(&skill.version).map_err(|e| {
        SkillError::ValidationError(format!("invalid semver version '{}': {}", skill.version, e))
    })?;

    // Validate entry file exists
    let entry_path = base_dir.join(&skill.entry);
    if !entry_path.exists() {
        return Err(SkillError::ValidationError(format!(
            "entry file '{}' not found at {:?}",
            skill.entry, entry_path
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Skill;
    use std::collections::HashMap;

    fn valid_skill() -> Skill {
        Skill {
            name: "test-skill".to_string(),
            version: "1.0.0".to_string(),
            description: Some("a test".to_string()),
            author: Some("tester".to_string()),
            license: Some("MIT".to_string()),
            tags: vec!["test".to_string()],
            harnesses: HashMap::new(),
            requires: vec![],
            entry: "Cargo.toml".to_string(),
            config: None,
        }
    }

    #[test]
    fn test_valid_skill_passes() {
        let skill = valid_skill();
        let result = validate(&skill, Path::new("."));
        assert!(result.is_ok());
    }

    #[test]
    fn test_empty_name_fails() {
        let mut skill = valid_skill();
        skill.name = "".to_string();
        let result = validate(&skill, Path::new("."));
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_version_fails() {
        let mut skill = valid_skill();
        skill.version = "".to_string();
        let result = validate(&skill, Path::new("."));
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_semver_fails() {
        let mut skill = valid_skill();
        skill.version = "not-a-version".to_string();
        let result = validate(&skill, Path::new("."));
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_entry_fails() {
        let mut skill = valid_skill();
        skill.entry = "nonexistent.md".to_string();
        let result = validate(&skill, Path::new("."));
        assert!(result.is_err());
    }
}
