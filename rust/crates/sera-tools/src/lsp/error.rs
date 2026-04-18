//! Error type for the LSP subsystem.
//!
//! Phase 1 scope — see `docs/plan/LSP-TOOLS-DESIGN.md` §13.

use sera_errors::{SeraError, SeraErrorCode};
use std::io;

/// Errors produced by the LSP supervisor, client, registry, cache, and tools.
#[derive(Debug, thiserror::Error)]
pub enum LspError {
    /// Failed to spawn the language-server child process.
    #[error("failed to spawn LSP server process: {0}")]
    SpawnFailed(#[source] io::Error),

    /// The `initialize` handshake with the LSP server failed.
    #[error("LSP initialize failed: {0}")]
    Initialize(String),

    /// An LSP request failed (non-timeout).
    #[error("LSP request `{method}` failed: {reason}")]
    Request { method: String, reason: String },

    /// A request exceeded the per-request timeout budget.
    #[error("LSP request timed out")]
    Timeout,

    /// A caller attempted to use a `..` path that escapes the project root.
    #[error("path traversal detected: relative path escapes project root")]
    PathTraversal,

    /// No LSP server configured for the requested language / extension.
    #[error("unsupported language: {language}")]
    Unsupported { language: String },

    /// Cache miss (consumer should repopulate via an LSP round-trip).
    #[error("symbol cache miss")]
    CacheMiss,

    /// Caller-supplied name-path pattern failed to parse.
    #[error("invalid name path `{raw}`: {reason}")]
    InvalidNamePath { raw: String, reason: String },

    /// A `find_referencing_symbols` call could not locate its target symbol
    /// in the supplied file.
    #[error("symbol not found: `{name_path}`")]
    SymbolNotFound { name_path: String },
}

impl From<LspError> for SeraError {
    fn from(err: LspError) -> Self {
        let code = match &err {
            LspError::SpawnFailed(_) | LspError::Initialize(_) => SeraErrorCode::Unavailable,
            LspError::Request { .. } => SeraErrorCode::Internal,
            LspError::Timeout => SeraErrorCode::Timeout,
            LspError::PathTraversal => SeraErrorCode::InvalidInput,
            LspError::Unsupported { .. } => SeraErrorCode::NotImplemented,
            LspError::CacheMiss => SeraErrorCode::NotFound,
            LspError::InvalidNamePath { .. } => SeraErrorCode::InvalidInput,
            LspError::SymbolNotFound { .. } => SeraErrorCode::NotFound,
        };
        let message = err.to_string();
        SeraError::with_source(code, message, err)
    }
}

/// Tool-level error alias — phase 1 keeps this co-located with `LspError`
/// because the existing `Tool` trait does not yet define its own error type
/// (see `docs/plan/specs/SPEC-tools.md` §3.1 upgrade scheduled as a follow-up bead).
pub type ToolError = LspError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_traversal_maps_to_invalid_input() {
        let err: SeraError = LspError::PathTraversal.into();
        assert_eq!(err.code, SeraErrorCode::InvalidInput);
        assert!(err.message.contains("path traversal"));
    }

    #[test]
    fn timeout_maps_to_timeout_code() {
        let err: SeraError = LspError::Timeout.into();
        assert_eq!(err.code, SeraErrorCode::Timeout);
    }

    #[test]
    fn unsupported_maps_to_not_implemented() {
        let err: SeraError = LspError::Unsupported {
            language: "kotlin".into(),
        }
        .into();
        assert_eq!(err.code, SeraErrorCode::NotImplemented);
        assert!(err.message.contains("kotlin"));
    }
}
