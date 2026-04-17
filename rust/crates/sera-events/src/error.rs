//! Error types for events crate.

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
}
