use thiserror::Error;

#[derive(Error, Debug)]
pub enum HarnessError {
    #[error("skill error: {0}")]
    SkillError(#[from] usk_core::error::SkillError),

    #[error("conversion error: {0}")]
    ConversionError(String),

    #[error("unsupported harness version: {0}")]
    UnsupportedVersion(String),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("YAML error: {0}")]
    YamlError(#[from] serde_yaml::Error),
}

pub type Result<T> = std::result::Result<T, HarnessError>;
