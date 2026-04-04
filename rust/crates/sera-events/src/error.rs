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
