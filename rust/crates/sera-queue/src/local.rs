use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::{
    backend::{QueueBackend, QueueError},
    lane::LaneQueue,
};

/// In-process queue backend backed by `LaneQueue` behind a `Mutex`.
pub struct LocalQueueBackend {
    inner: Mutex<LaneQueue>,
}

impl LocalQueueBackend {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(LaneQueue::new()),
        }
    }
}

impl Default for LocalQueueBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl QueueBackend for LocalQueueBackend {
    async fn push(&self, lane: &str, payload: serde_json::Value) -> Result<String, QueueError> {
        let mut q = self.inner.lock().await;
        let result = q.enqueue(lane, payload, crate::lane::QueueMode::Collect);
        Ok(result.id)
    }

    async fn pull(&self, lane: &str) -> Result<Option<(String, serde_json::Value)>, QueueError> {
        let mut q = self.inner.lock().await;
        Ok(q.dequeue(lane).map(|e| (e.id, e.payload)))
    }

    async fn ack(&self, _job_id: &str) -> Result<(), QueueError> {
        // Local backend: ack is a no-op (item already removed by pull).
        Ok(())
    }

    async fn nack(&self, job_id: &str) -> Result<(), QueueError> {
        // Local backend: nack is unsupported — callers should re-enqueue if needed.
        Err(QueueError::NotFound {
            id: job_id.to_owned(),
        })
    }

    async fn recover_orphans(
        &self,
        _stale_threshold: std::time::Duration,
    ) -> Result<usize, QueueError> {
        // Local backend has no persistence; orphan recovery is a no-op.
        Ok(0)
    }
}
