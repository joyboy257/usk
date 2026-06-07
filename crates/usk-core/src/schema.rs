use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub harnesses: HashMap<String, String>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default)]
    pub config: Option<SkillConfig>,
}

fn default_entry() -> String {
    "SKILL.md".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillConfig {
    pub timeout: Option<u64>,
    pub temperature: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub tags: Vec<String>,
    pub harnesses: Vec<String>,
}

impl From<&Skill> for SkillMeta {
    fn from(skill: &Skill) -> Self {
        SkillMeta {
            name: skill.name.clone(),
            version: skill.version.clone(),
            description: skill.description.clone(),
            author: skill.author.clone(),
            tags: skill.tags.clone(),
            harnesses: skill.harnesses.keys().cloned().collect(),
        }
    }
}
