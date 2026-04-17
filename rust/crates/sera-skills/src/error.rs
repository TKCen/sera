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

    /// A `SkillRef` string failed to parse (bad grammar, bad version
    /// constraint, etc.).
    #[error("invalid skill reference: {0}")]
    InvalidReference(String),

    /// A source kind is recognised but not yet wired up (phase-gated).
    #[error("skill source unavailable ({source_kind:?}): {reason}")]
    Unavailable {
        source_kind: crate::skill_ref::SkillSourceKind,
        reason: String,
    },

    /// An operation is not supported by this source type.
    #[error("unsupported operation ({source_kind:?}): {reason}")]
    Unsupported {
        source_kind: crate::skill_ref::SkillSourceKind,
        reason: String,
    },

    /// TOML (de)serialization for the lock file.
    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("TOML deserialize error: {0}")]
    TomlDeserialize(#[from] toml::de::Error),

    /// Propagated OCI client error.
    #[error("OCI error: {0}")]
    Oci(String),
}
