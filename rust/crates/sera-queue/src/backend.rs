use async_trait::async_trait;
use sera_errors::{IntoSeraError, SeraError, SeraErrorCode};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueueError {
    #[error("queue unavailable: {reason}")]
    Unavailable { reason: String },
    #[error("serialization error: {reason}")]
    Serde { reason: String },
    #[error("job not found: {id}")]
    NotFound { id: String },
    #[error("storage error: {reason}")]
    Storage { reason: String },
}

impl QueueError {
    /// Convert to [`SeraError`] with the canonical code for this variant.
    pub fn into_sera_error(self) -> SeraError {
        let code = match &self {
            QueueError::Unavailable { .. } => SeraErrorCode::Unavailable,
            QueueError::Serde { .. } => SeraErrorCode::Serialization,
            QueueError::NotFound { .. } => SeraErrorCode::NotFound,
            QueueError::Storage { .. } => SeraErrorCode::Internal,
        };
        self.into_sera(code)
    }
}

#[cfg(test)]
mod into_sera_tests {
    use super::*;
    use sera_errors::SeraErrorCode;

    #[test]
    fn unavailable_maps_correctly() {
        let err = QueueError::Unavailable { reason: "broker offline".into() };
        let sera = err.into_sera_error();
        assert_eq!(sera.code, SeraErrorCode::Unavailable);
        assert!(sera.message.contains("broker offline"));
    }

    #[test]
    fn serde_maps_to_serialization() {
        let err = QueueError::Serde { reason: "bad json".into() };
        let sera = err.into_sera_error();
        assert_eq!(sera.code, SeraErrorCode::Serialization);
        assert!(sera.message.contains("bad json"));
    }

    #[test]
    fn not_found_maps_correctly() {
        let err = QueueError::NotFound { id: "job-abc".into() };
        let sera = err.into_sera_error();
        assert_eq!(sera.code, SeraErrorCode::NotFound);
        assert!(sera.message.contains("job-abc"));
    }

    #[test]
    fn storage_maps_to_internal() {
        let err = QueueError::Storage { reason: "disk full".into() };
        let sera = err.into_sera_error();
        assert_eq!(sera.code, SeraErrorCode::Internal);
        assert!(sera.message.contains("disk full"));
    }

    #[test]
    fn message_roundtrip_preserves_context() {
        let err = QueueError::NotFound { id: "lane:priority:99".into() };
        let sera = err.into_sera_error();
        assert!(sera.message.contains("lane:priority:99"));
    }
}

/// Object-safe queue backend trait — no associated types, no generics on methods.
#[async_trait]
pub trait QueueBackend: Send + Sync + 'static {
    async fn push(&self, lane: &str, payload: serde_json::Value) -> Result<String, QueueError>;
    async fn pull(&self, lane: &str) -> Result<Option<(String, serde_json::Value)>, QueueError>;
    async fn ack(&self, job_id: &str) -> Result<(), QueueError>;
    async fn nack(&self, job_id: &str) -> Result<(), QueueError>;
    async fn recover_orphans(
        &self,
        stale_threshold: std::time::Duration,
    ) -> Result<usize, QueueError>;
}
