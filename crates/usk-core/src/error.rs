use thiserror::Error;

#[derive(Error, Debug)]
pub enum SkillError {
    #[error("failed to parse skill.yaml: {0}")]
    ParseError(#[from] serde_yaml::Error),

    #[error("validation error: {0}")]
    ValidationError(String),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("semver error: {0}")]
    SemverError(#[from] semver::Error),

    #[error("walkdir error: {0}")]
    WalkdirError(#[from] walkdir::Error),

    #[error("not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, SkillError>;
