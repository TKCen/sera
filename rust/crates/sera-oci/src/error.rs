//! Error type for OCI pull operations.

use sera_errors::{SeraError, SeraErrorCode};
use thiserror::Error;

/// Errors produced by the OCI registry client.
#[derive(Debug, Error)]
pub enum OciError {
    /// The OCI reference string could not be parsed.
    #[error("invalid OCI reference: {0}")]
    InvalidReference(String),

    /// A network-level failure talking to the registry.
    #[error("registry transport error: {0}")]
    Transport(String),

    /// The requested image, manifest, or blob was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// The registry rejected the request for authentication reasons.
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    /// The image manifest did not contain a layer with the expected
    /// `application/vnd.sera.plugin.manifest.v1+yaml` media type.
    #[error("image manifest does not contain a SERA plugin manifest layer")]
    MissingManifestLayer,

    /// Loading or parsing `docker config.json` failed.
    #[error("docker auth error: {0}")]
    Auth(String),

    /// Local filesystem I/O error (reading docker config, etc).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<OciError> for SeraError {
    fn from(err: OciError) -> Self {
        let code = match &err {
            OciError::InvalidReference(_) => SeraErrorCode::InvalidInput,
            OciError::Transport(_) => SeraErrorCode::Unavailable,
            OciError::NotFound(_) => SeraErrorCode::NotFound,
            OciError::Unauthorized(_) => SeraErrorCode::Unauthorized,
            OciError::MissingManifestLayer => SeraErrorCode::NotFound,
            OciError::Auth(_) => SeraErrorCode::Unauthorized,
            OciError::Io(_) => SeraErrorCode::Internal,
        };
        SeraError::new(code, err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_reference_maps_to_invalid_input() {
        let err: SeraError = OciError::InvalidReference("bad".into()).into();
        assert_eq!(err.code, SeraErrorCode::InvalidInput);
    }

    #[test]
    fn not_found_maps_to_not_found() {
        let err: SeraError = OciError::NotFound("ghcr.io/x/y:1".into()).into();
        assert_eq!(err.code, SeraErrorCode::NotFound);
    }

    #[test]
    fn unauthorized_maps_to_unauthorized() {
        let err: SeraError = OciError::Unauthorized("token expired".into()).into();
        assert_eq!(err.code, SeraErrorCode::Unauthorized);
    }

    #[test]
    fn transport_maps_to_unavailable() {
        let err: SeraError = OciError::Transport("connection refused".into()).into();
        assert_eq!(err.code, SeraErrorCode::Unavailable);
    }

    #[test]
    fn missing_manifest_layer_maps_to_not_found() {
        let err: SeraError = OciError::MissingManifestLayer.into();
        assert_eq!(err.code, SeraErrorCode::NotFound);
    }
}
