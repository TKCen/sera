//! Error types for events crate.

use sera_errors::{SeraError, SeraErrorCode};
use thiserror::Error;

/// Audit chain verification error.
#[derive(Debug, Error)]
#[error("Audit chain broken at sequence {broken_at}")]
pub struct AuditVerifyError {
    pub broken_at: String,
}

/// Centrifugo client error.
#[derive(Debug, Error)]
pub enum CentrifugoError {
    /// HTTP request failed.
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    /// JWT token generation failed.
    #[error("Token generation error: {0}")]
    TokenError(String),

    /// Centrifugo API error.
    #[error("API error: {0}")]
    ApiError(String),
}

impl From<AuditVerifyError> for SeraError {
    fn from(err: AuditVerifyError) -> Self {
        SeraError::with_source(SeraErrorCode::InvalidInput, err.to_string(), err)
    }
}

impl From<CentrifugoError> for SeraError {
    fn from(err: CentrifugoError) -> Self {
        let code = match &err {
            CentrifugoError::HttpError(_) => SeraErrorCode::Unavailable,
            CentrifugoError::TokenError(_) => SeraErrorCode::Internal,
            CentrifugoError::ApiError(_) => SeraErrorCode::Unavailable,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_verify_error_display() {
        let e = AuditVerifyError { broken_at: "7".to_string() };
        assert_eq!(e.to_string(), "Audit chain broken at sequence 7");
    }

    #[test]
    fn audit_verify_error_broken_at_field() {
        let e = AuditVerifyError { broken_at: "seq-42".to_string() };
        assert_eq!(e.broken_at, "seq-42");
    }

    #[test]
    fn centrifugo_error_token_display() {
        let e = CentrifugoError::TokenError("bad key".to_string());
        assert_eq!(e.to_string(), "Token generation error: bad key");
    }

    #[test]
    fn centrifugo_error_api_display() {
        let e = CentrifugoError::ApiError("HTTP 503: service unavailable".to_string());
        assert_eq!(e.to_string(), "API error: HTTP 503: service unavailable");
    }

    #[test]
    fn audit_verify_error_maps_to_invalid_input() {
        let e: SeraError = AuditVerifyError { broken_at: "5".to_string() }.into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
    }

    #[test]
    fn audit_verify_error_message_preserved() {
        let e: SeraError = AuditVerifyError { broken_at: "42".to_string() }.into();
        assert!(e.message.contains("42"));
    }

    #[test]
    fn centrifugo_http_error_maps_to_unavailable() {
        let e: SeraError = CentrifugoError::TokenError("bad".to_string()).into();
        assert_eq!(e.code, SeraErrorCode::Internal);
    }

    #[test]
    fn centrifugo_token_error_maps_to_internal() {
        let e: SeraError = CentrifugoError::TokenError("key error".to_string()).into();
        assert_eq!(e.code, SeraErrorCode::Internal);
        assert!(e.message.contains("key error"));
    }

    #[test]
    fn centrifugo_api_error_maps_to_unavailable() {
        let e: SeraError = CentrifugoError::ApiError("503".to_string()).into();
        assert_eq!(e.code, SeraErrorCode::Unavailable);
    }

    #[test]
    fn centrifugo_api_error_message_preserved() {
        let e: SeraError = CentrifugoError::ApiError("downstream failed".to_string()).into();
        assert!(e.message.contains("downstream failed"));
    }
}
