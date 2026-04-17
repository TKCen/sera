//! Error types for skill pack operations.

use thiserror::Error;

/// Errors that can occur during skill pack operations.
#[derive(Debug, Error)]
pub enum SkillsError {
    #[error("skill pack not found: {0}")]
    NotFound(String),

    #[error("invalid skill pack format: {0}")]
    InvalidFormat(String),

    #[error("skill not found in pack: {0}")]
    SkillNotFound(String),

    #[error("failed to load skill pack: {0}")]
    LoadFailed(String),

    #[error("skill validation failed: {0}")]
    ValidationFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("YAML parsing error: {0}")]
    YamlParsing(#[from] serde_yaml::Error),

    #[error("skill markdown format error: {0}")]
    Format(String),
}
