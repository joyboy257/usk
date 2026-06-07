use crate::error::Result;
use crate::schema::{Skill, SkillMeta};
use std::path::Path;
use walkdir::WalkDir;

pub struct RegistryIndex {
    skills: Vec<SkillMeta>,
}

impl RegistryIndex {
    pub fn new() -> Self {
        RegistryIndex { skills: Vec::new() }
    }

    pub fn load_from_dir(&mut self, dir: &Path) -> Result<()> {
        for entry in WalkDir::new(dir).into_iter() {
            let entry = entry?;
            if entry.file_name() == "skill.yaml" {
                let path = entry.path();
                let content = std::fs::read_to_string(path)?;
                match serde_yaml::from_str::<Skill>(&content) {
                    Ok(skill) => {
                        let dir_name = path
                            .parent()
                            .and_then(|p| p.parent())
                            .and_then(|p| p.file_name())
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let mut meta = SkillMeta::from(&skill);
                        if meta.name.is_empty() {
                            meta.name = dir_name;
                        }
                        self.skills.push(meta);
                    }
                    Err(e) => {
                        eprintln!("warning: skipping invalid skill.yaml at {:?}: {}", path, e);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn search(&self, query: &str) -> Vec<&SkillMeta> {
        let q = query.to_lowercase();
        self.skills
            .iter()
            .filter(|s| {
                s.name.to_lowercase().contains(&q)
                    || s.description
                        .as_deref()
                        .map(|d| d.to_lowercase().contains(&q))
                        .unwrap_or(false)
                    || s.tags.iter().any(|t| t.to_lowercase().contains(&q))
            })
            .collect()
    }

    pub fn search_by_tag(&self, tag: &str) -> Vec<&SkillMeta> {
        let t = tag.to_lowercase();
        self.skills
            .iter()
            .filter(|s| s.tags.iter().any(|tag| tag.to_lowercase() == t))
            .collect()
    }

    pub fn all(&self) -> &[SkillMeta] {
        &self.skills
    }

    pub fn get(&self, name: &str) -> Option<&SkillMeta> {
        self.skills.iter().find(|s| s.name == name)
    }

    pub fn get_latest(&self, name: &str) -> Option<&SkillMeta> {
        self.skills
            .iter()
            .filter(|s| s.name == name)
            .max_by(|a, b| {
                let av = semver::Version::parse(&a.version).ok();
                let bv = semver::Version::parse(&b.version).ok();
                match (av, bv) {
                    (Some(av), Some(bv)) => av.cmp(&bv),
                    (Some(_), None) => std::cmp::Ordering::Greater,
                    (None, Some(_)) => std::cmp::Ordering::Less,
                    (None, None) => a.version.cmp(&b.version),
                }
            })
    }

    pub fn get_version(&self, name: &str, version: &str) -> Option<&SkillMeta> {
        self.skills
            .iter()
            .find(|s| s.name == name && s.version == version)
    }

    pub fn versions(&self, name: &str) -> Vec<String> {
        let mut versions: Vec<&SkillMeta> = self.skills.iter().filter(|s| s.name == name).collect();
        versions.sort_by(|a, b| {
            let av = semver::Version::parse(&a.version).ok();
            let bv = semver::Version::parse(&b.version).ok();
            match (av, bv) {
                (Some(av), Some(bv)) => bv.cmp(&av),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });
        versions.into_iter().map(|s| s.version.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

impl Default for RegistryIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_empty_index() {
        let index = RegistryIndex::new();
        assert!(index.is_empty());
        assert_eq!(index.search("anything").len(), 0);
    }

    #[test]
    fn test_load_and_search() {
        let dir = temp_dir();
        let skill_dir = dir.path().join("skills").join("test-skill").join("1.0.0");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("skill.yaml"),
            r#"
name: test-skill
version: 1.0.0
description: a test skill
tags: [test, example]
harnesses:
  claude-code: ">=0.1"
"#,
        )
        .unwrap();

        let mut index = RegistryIndex::new();
        index.load_from_dir(dir.path()).unwrap();
        assert_eq!(index.len(), 1);

        let results = index.search("test");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "test-skill");
    }

    #[test]
    fn test_skip_invalid_yaml() {
        let dir = temp_dir();
        let skill_dir = dir.path().join("skills").join("bad-skill").join("1.0.0");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("skill.yaml"), "invalid: [yaml").unwrap();

        let mut index = RegistryIndex::new();
        index.load_from_dir(dir.path()).unwrap();
        assert!(index.is_empty());
    }

    #[test]
    fn test_search_by_tag() {
        let dir = temp_dir();
        let skill_dir = dir.path().join("skills").join("tagged-skill").join("1.0.0");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("skill.yaml"),
            r#"
name: tagged-skill
version: 1.0.0
tags: [networking, security]
"#,
        )
        .unwrap();

        let mut index = RegistryIndex::new();
        index.load_from_dir(dir.path()).unwrap();

        let results = index.search_by_tag("security");
        assert_eq!(results.len(), 1);
    }

    fn write_skill(dir: &std::path::Path, name: &str, version: &str, yaml_body: &str) {
        let skill_dir = dir.join("skills").join(name).join(version);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("skill.yaml"), yaml_body).unwrap();
    }

    #[test]
    fn test_get_latest_picks_highest_semver() {
        let dir = temp_dir();
        for (v, body) in [
            ("1.0.0", "name: my-skill\nversion: 1.0.0\n"),
            ("2.0.0", "name: my-skill\nversion: 2.0.0\n"),
            ("1.5.0", "name: my-skill\nversion: 1.5.0\n"),
        ] {
            write_skill(dir.path(), "my-skill", v, body);
        }

        let mut index = RegistryIndex::new();
        index.load_from_dir(dir.path()).unwrap();
        assert_eq!(index.len(), 3);

        let latest = index.get_latest("my-skill").unwrap();
        assert_eq!(latest.version, "2.0.0");
    }

    #[test]
    fn test_get_version_returns_specific() {
        let dir = temp_dir();
        for (v, body) in [
            ("1.0.0", "name: my-skill\nversion: 1.0.0\n"),
            ("1.5.0", "name: my-skill\nversion: 1.5.0\n"),
            ("2.0.0", "name: my-skill\nversion: 2.0.0\n"),
        ] {
            write_skill(dir.path(), "my-skill", v, body);
        }

        let mut index = RegistryIndex::new();
        index.load_from_dir(dir.path()).unwrap();

        let specific = index.get_version("my-skill", "1.5.0").unwrap();
        assert_eq!(specific.version, "1.5.0");

        let missing = index.get_version("my-skill", "9.9.9");
        assert!(missing.is_none());
    }

    #[test]
    fn test_get_latest_with_invalid_semver_falls_back() {
        let dir = temp_dir();
        // Loaded out of order to make sure get_latest isn't relying on insertion order
        write_skill(dir.path(), "weird-skill", "weird", "name: weird-skill\nversion: weird\n");
        write_skill(
            dir.path(),
            "weird-skill",
            "1.2.3",
            "name: weird-skill\nversion: 1.2.3\n",
        );

        let mut index = RegistryIndex::new();
        index.load_from_dir(dir.path()).unwrap();

        let latest = index.get_latest("weird-skill").unwrap();
        assert_eq!(latest.version, "1.2.3");
    }
}
