//! sera-errors — unified error codes and structured errors for SERA crates.
//!
//! Provides `SeraErrorCode` (the canonical error taxonomy) and `SeraError`
//! (a structured error type carrying code + message + optional source).
//! Crates define their own thiserror enums, then implement `From<LocalError>`
//! for `SeraError` or use `.into_sera(code)` to bridge into the shared taxonomy.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unified error code taxonomy for cross-crate error categorisation.
///
/// Maps 1:1 to HTTP status codes via [`SeraErrorCode::http_status`] and to
/// gRPC status codes via [`SeraErrorCode::grpc_code`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SeraErrorCode {
    /// An internal error with no specific classification (500).
    Internal,
    /// The requested resource was not found (404).
    NotFound,
    /// The caller is not authorised for the requested action (401).
    Unauthorized,
    /// The caller lacks permission for the requested action (403).
    Forbidden,
    /// A timeout occurred (408).
    Timeout,
    /// A configuration error (500).
    Configuration,
    /// A serialisation/deserialisation error (400).
    Serialization,
    /// Invalid input or request (400).
    InvalidInput,
    /// The resource already exists (409).
    AlreadyExists,
    /// A required precondition was not met (412).
    PreconditionFailed,
    /// Rate limit exceeded (429).
    RateLimited,
    /// The service is unavailable (503).
    Unavailable,
    /// The operation was cancelled (499).
    Cancelled,
    /// Resource exhausted — quota, memory, disk (507).
    ResourceExhausted,
    /// The operation is not implemented (501).
    NotImplemented,
}

impl SeraErrorCode {
    /// Map to the closest HTTP status code.
    pub fn http_status(&self) -> u16 {
        match self {
            Self::Internal => 500,
            Self::NotFound => 404,
            Self::Unauthorized => 401,
            Self::Forbidden => 403,
            Self::Timeout => 408,
            Self::Configuration => 500,
            Self::Serialization => 400,
            Self::InvalidInput => 400,
            Self::AlreadyExists => 409,
            Self::PreconditionFailed => 412,
            Self::RateLimited => 429,
            Self::Unavailable => 503,
            Self::Cancelled => 499,
            Self::ResourceExhausted => 507,
            Self::NotImplemented => 501,
        }
    }

    /// String tag suitable for JSON error responses and logging.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Internal => "INTERNAL",
            Self::NotFound => "NOT_FOUND",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Forbidden => "FORBIDDEN",
            Self::Timeout => "TIMEOUT",
            Self::Configuration => "CONFIGURATION",
            Self::Serialization => "SERIALIZATION",
            Self::InvalidInput => "INVALID_INPUT",
            Self::AlreadyExists => "ALREADY_EXISTS",
            Self::PreconditionFailed => "PRECONDITION_FAILED",
            Self::RateLimited => "RATE_LIMITED",
            Self::Unavailable => "UNAVAILABLE",
            Self::Cancelled => "CANCELLED",
            Self::ResourceExhausted => "RESOURCE_EXHAUSTED",
            Self::NotImplemented => "NOT_IMPLEMENTED",
        }
    }
}

impl fmt::Display for SeraErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Structured error that carries a [`SeraErrorCode`], human-readable message,
/// and an optional boxed source error.
///
/// This is the canonical error type returned at API boundaries (gateway
/// responses, protocol adapters). Internal crates may keep their own
/// thiserror enums and convert via `From` or [`IntoSeraError::into_sera`].
#[derive(Debug)]
pub struct SeraError {
    /// The classified error code.
    pub code: SeraErrorCode,
    /// Human-readable error message.
    pub message: String,
    /// Optional underlying cause.
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl SeraError {
    pub fn new(code: SeraErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            source: None,
        }
    }

    pub fn with_source(
        code: SeraErrorCode,
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(SeraErrorCode::Internal, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(SeraErrorCode::NotFound, message)
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(SeraErrorCode::InvalidInput, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(SeraErrorCode::Unauthorized, message)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(SeraErrorCode::Unavailable, message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(SeraErrorCode::Timeout, message)
    }
}

impl fmt::Display for SeraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for SeraError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

/// Convenience trait for converting crate-local errors into [`SeraError`].
pub trait IntoSeraError {
    fn into_sera(self, code: SeraErrorCode) -> SeraError;
}

impl<E: std::error::Error + Send + Sync + 'static> IntoSeraError for E {
    fn into_sera(self, code: SeraErrorCode) -> SeraError {
        let message = self.to_string();
        SeraError::with_source(code, message, self)
    }
}

/// Serialisable error response body for JSON API responses.
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub code: SeraErrorCode,
    pub message: String,
}

impl From<&SeraError> for ErrorResponse {
    fn from(err: &SeraError) -> Self {
        Self {
            code: err.code,
            message: err.message.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_http_status_mapping() {
        assert_eq!(SeraErrorCode::NotFound.http_status(), 404);
        assert_eq!(SeraErrorCode::Internal.http_status(), 500);
        assert_eq!(SeraErrorCode::RateLimited.http_status(), 429);
        assert_eq!(SeraErrorCode::Unavailable.http_status(), 503);
    }

    #[test]
    fn error_code_display() {
        assert_eq!(SeraErrorCode::InvalidInput.to_string(), "INVALID_INPUT");
        assert_eq!(SeraErrorCode::Forbidden.to_string(), "FORBIDDEN");
    }

    #[test]
    fn sera_error_display_and_source() {
        use std::error::Error;
        let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
        let err = SeraError::with_source(SeraErrorCode::NotFound, "resource missing", inner);
        assert!(err.to_string().contains("NOT_FOUND"));
        assert!(err.to_string().contains("resource missing"));
        assert!(err.source().is_some());
    }

    #[test]
    fn error_response_serialization() {
        let resp = ErrorResponse {
            code: SeraErrorCode::Timeout,
            message: "took too long".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("Timeout"));
        assert!(json.contains("took too long"));
    }

    #[test]
    fn into_sera_error_trait() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "nope");
        let sera_err = io_err.into_sera(SeraErrorCode::Forbidden);
        assert_eq!(sera_err.code, SeraErrorCode::Forbidden);
        assert!(sera_err.message.contains("nope"));
    }
}
