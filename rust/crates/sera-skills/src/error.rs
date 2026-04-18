//! Error types for skill pack operations.

use sera_errors::{IntoSeraError, SeraError, SeraErrorCode};
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

impl SkillsError {
    /// Convert to [`SeraError`] with the canonical code for this variant.
    pub fn into_sera_error(self) -> SeraError {
        let code = match &self {
            SkillsError::NotFound(_) => SeraErrorCode::NotFound,
            SkillsError::SkillNotFound(_) => SeraErrorCode::NotFound,
            SkillsError::InvalidFormat(_) => SeraErrorCode::InvalidInput,
            SkillsError::InvalidReference(_) => SeraErrorCode::InvalidInput,
            SkillsError::ValidationFailed(_) => SeraErrorCode::InvalidInput,
            SkillsError::Format(_) => SeraErrorCode::InvalidInput,
            SkillsError::LoadFailed(_) => SeraErrorCode::Internal,
            SkillsError::Io(_) => SeraErrorCode::Internal,
            SkillsError::Serialization(_) => SeraErrorCode::Serialization,
            SkillsError::YamlParsing(_) => SeraErrorCode::Serialization,
            SkillsError::TomlSerialize(_) => SeraErrorCode::Serialization,
            SkillsError::TomlDeserialize(_) => SeraErrorCode::Serialization,
            SkillsError::Unavailable { .. } => SeraErrorCode::Unavailable,
            SkillsError::Unsupported { .. } => SeraErrorCode::NotImplemented,
            SkillsError::Oci(_) => SeraErrorCode::Internal,
        };
        self.into_sera(code)
    }
}

#[cfg(test)]
mod into_sera_tests {
    use super::*;
    use sera_errors::SeraErrorCode;

    #[test]
    fn not_found_maps_correctly() {
        let err = SkillsError::NotFound("my-pack".into());
        let sera = err.into_sera_error();
        assert_eq!(sera.code, SeraErrorCode::NotFound);
        assert!(sera.message.contains("my-pack"));
    }

    #[test]
    fn invalid_format_maps_to_invalid_input() {
        let err = SkillsError::InvalidFormat("missing name field".into());
        let sera = err.into_sera_error();
        assert_eq!(sera.code, SeraErrorCode::InvalidInput);
        assert!(sera.message.contains("missing name field"));
    }

    #[test]
    fn unavailable_maps_correctly() {
        let err = SkillsError::Unavailable {
            source_kind: crate::skill_ref::SkillSourceKind::Registry,
            reason: "registry offline".into(),
        };
        let sera = err.into_sera_error();
        assert_eq!(sera.code, SeraErrorCode::Unavailable);
        assert!(sera.message.contains("registry offline"));
    }

    #[test]
    fn unsupported_maps_to_not_implemented() {
        let err = SkillsError::Unsupported {
            source_kind: crate::skill_ref::SkillSourceKind::Fs,
            reason: "search not supported".into(),
        };
        let sera = err.into_sera_error();
        assert_eq!(sera.code, SeraErrorCode::NotImplemented);
        assert!(sera.message.contains("search not supported"));
    }

    #[test]
    fn serialization_maps_correctly() {
        let err = SkillsError::Serialization(
            serde_json::from_str::<serde_json::Value>("not json").unwrap_err(),
        );
        let sera = err.into_sera_error();
        assert_eq!(sera.code, SeraErrorCode::Serialization);
    }
}
