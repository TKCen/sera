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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_error_display() {
        let err = ModelError::Provider("quota exceeded".to_string());
        assert_eq!(err.to_string(), "provider error: quota exceeded");
    }

    #[test]
    fn invalid_response_display() {
        let err = ModelError::InvalidResponse("unexpected null".to_string());
        assert_eq!(
            err.to_string(),
            "invalid response from provider: unexpected null"
        );
    }

    #[test]
    fn authentication_error_display() {
        let err = ModelError::Authentication("invalid api key".to_string());
        assert_eq!(err.to_string(), "authentication failed: invalid api key");
    }

    #[test]
    fn rate_limit_display() {
        let err = ModelError::RateLimit;
        assert_eq!(err.to_string(), "rate limit exceeded");
    }

    #[test]
    fn context_length_exceeded_display() {
        let err = ModelError::ContextLengthExceeded;
        assert_eq!(err.to_string(), "context length exceeded");
    }

    #[test]
    fn not_available_display() {
        let err = ModelError::NotAvailable("openai".to_string());
        assert_eq!(err.to_string(), "provider not available: openai");
    }

    #[test]
    fn timeout_display() {
        let err = ModelError::Timeout;
        assert_eq!(err.to_string(), "timeout waiting for response");
    }

    #[test]
    fn serialization_error_from_serde_json() {
        // Force a serde_json::Error via invalid JSON deserialization
        let serde_err: serde_json::Error =
            serde_json::from_str::<serde_json::Value>("not-json").expect_err("must fail");
        let model_err: ModelError = serde_err.into();
        assert!(
            model_err.to_string().starts_with("request serialization failed:"),
            "expected serialization prefix, got: {model_err}"
        );
    }
}
