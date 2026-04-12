use async_trait::async_trait;
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
