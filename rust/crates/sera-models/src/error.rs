//! Error types for model provider operations.

use thiserror::Error;

/// Errors that can occur during model provider operations.
#[derive(Debug, Error)]
pub enum ModelError {
    #[error("provider error: {0}")]
    Provider(String),

    #[error("request serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("invalid response from provider: {0}")]
    InvalidResponse(String),

    #[error("authentication failed: {0}")]
    Authentication(String),

    #[error("rate limit exceeded")]
    RateLimit,

    #[error("context length exceeded")]
    ContextLengthExceeded,

    #[error("provider not available: {0}")]
    NotAvailable(String),

    #[error("timeout waiting for response")]
    Timeout,
}
