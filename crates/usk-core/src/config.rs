use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledSkill {
    pub version: String,
    pub harness: String,
    pub install_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub registry_url: String,
    pub install_dir: PathBuf,
    pub harnesses: HashMap<String, String>,
    pub installed: HashMap<String, InstalledSkill>,
}

impl Default for Config {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let usk_dir = PathBuf::from(&home).join(".usk");
        Config {
            registry_url: "http://localhost:8080".to_string(),
            install_dir: usk_dir.join("skills"),
            harnesses: HashMap::from([
                ("claude-code".to_string(), "usk-harness-claude".to_string()),
                ("codex-cli".to_string(), "usk-harness-codex".to_string()),
            ]),
            installed: HashMap::new(),
        }
    }
}

impl Config {
    pub fn config_dir() -> PathBuf {
        std::env::var("USK_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                PathBuf::from(&home).join(".usk")
            })
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(config) => return config,
                    Err(e) => {
                        eprintln!("warning: failed to parse config at {:?}: {}", path, e);
                    }
                },
                Err(e) => {
                    eprintln!("warning: failed to read config at {:?}: {}", path, e);
                }
            }
        }
        let config = Config::default();
        if let Err(e) = config.save() {
            eprintln!("warning: failed to save default config: {}", e);
        }
        config
    }

    pub fn save(&self) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir)?;
        let content = toml::to_string_pretty(self)?;
        std::fs::write(Self::config_path(), content)?;
        Ok(())
    }
}
