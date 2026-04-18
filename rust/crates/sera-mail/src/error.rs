//! Error types for mail correlation.

use thiserror::Error;

/// Errors produced by the mail correlation pipeline.
///
/// Variants map to [`sera_errors::SeraErrorCode`] via the `From` impl so
/// higher-layer transport errors can be lifted into the shared taxonomy.
#[derive(Debug, Error)]
pub enum MailCorrelationError {
    /// The raw MIME blob failed to parse. Malformed headers, broken encoding,
    /// or fundamentally non-RFC-5322 input.
    #[error("failed to parse inbound mail: {0}")]
    ParseFailed(String),

    /// The envelope index was poisoned (mutex lock failure). Non-recoverable
    /// within a process; the caller should panic or restart.
    #[error("envelope index lock poisoned")]
    IndexPoisoned,

    /// The `IssuanceHook` implementation returned an error while recording an
    /// outbound envelope.
    #[error("issuance hook failed: {0}")]
    HookFailed(String),

    /// Input validation failed (e.g. an envelope without a `message_id`).
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

impl From<MailCorrelationError> for sera_errors::SeraError {
    fn from(err: MailCorrelationError) -> Self {
        use sera_errors::SeraErrorCode;
        let code = match err {
            MailCorrelationError::ParseFailed(_) => SeraErrorCode::Serialization,
            MailCorrelationError::IndexPoisoned => SeraErrorCode::Internal,
            MailCorrelationError::HookFailed(_) => SeraErrorCode::Internal,
            MailCorrelationError::InvalidInput(_) => SeraErrorCode::InvalidInput,
        };
        sera_errors::SeraError::new(code, err.to_string())
    }
}
