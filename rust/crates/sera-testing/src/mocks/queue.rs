use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::Mutex;
use async_trait::async_trait;
use sera_queue::{QueueBackend, QueueError};

/// In-memory mock queue backend for testing.
#[derive(Clone)]
pub struct MockQueueBackend {
    queues: Arc<Mutex<HashMap<String, VecDeque<(String, serde_json::Value)>>>>,
    next_id: Arc<std::sync::atomic::AtomicU64>,
}

impl MockQueueBackend {
    pub fn new() -> Self {
        Self {
            queues: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// Get the number of items in a lane.
    pub async fn len(&self, lane: &str) -> usize {
        let queues = self.queues.lock().await;
        queues.get(lane).map_or(0, |q| q.len())
    }

    /// Check if a lane is empty.
    pub async fn is_empty(&self, lane: &str) -> bool {
        self.len(lane).await == 0
    }
}

impl Default for MockQueueBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl QueueBackend for MockQueueBackend {
    async fn push(&self, lane: &str, payload: serde_json::Value) -> Result<String, QueueError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let job_id = format!("mock-job-{id}");
        let mut queues = self.queues.lock().await;
        queues
            .entry(lane.to_string())
            .or_default()
            .push_back((job_id.clone(), payload));
        Ok(job_id)
    }

    async fn pull(&self, lane: &str) -> Result<Option<(String, serde_json::Value)>, QueueError> {
        let mut queues = self.queues.lock().await;
        Ok(queues.get_mut(lane).and_then(|q| q.pop_front()))
    }

    async fn ack(&self, _job_id: &str) -> Result<(), QueueError> {
        Ok(())
    }

    async fn nack(&self, _job_id: &str) -> Result<(), QueueError> {
        Ok(())
    }

    async fn recover_orphans(
        &self,
        _stale_threshold: std::time::Duration,
    ) -> Result<usize, QueueError> {
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn mock_queue_push_pull_roundtrip() {
        let q = MockQueueBackend::new();
        let payload = json!({"hello": "world"});
        let job_id = q.push("test-lane", payload.clone()).await.unwrap();
        let result = q.pull("test-lane").await.unwrap();
        assert!(result.is_some());
        let (pulled_id, pulled_payload) = result.unwrap();
        assert_eq!(pulled_id, job_id);
        assert_eq!(pulled_payload, payload);
    }

    #[tokio::test]
    async fn mock_queue_empty_pull_returns_none() {
        let q = MockQueueBackend::new();
        let result = q.pull("empty-lane").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn mock_queue_fifo_ordering() {
        let q = MockQueueBackend::new();
        q.push("lane", json!("A")).await.unwrap();
        q.push("lane", json!("B")).await.unwrap();
        q.push("lane", json!("C")).await.unwrap();

        let (_, a) = q.pull("lane").await.unwrap().unwrap();
        let (_, b) = q.pull("lane").await.unwrap().unwrap();
        let (_, c) = q.pull("lane").await.unwrap().unwrap();

        assert_eq!(a, json!("A"));
        assert_eq!(b, json!("B"));
        assert_eq!(c, json!("C"));
    }

    #[tokio::test]
    async fn mock_queue_multiple_lanes() {
        let q = MockQueueBackend::new();
        q.push("lane-a", json!(1)).await.unwrap();
        q.push("lane-b", json!(2)).await.unwrap();

        let (_, val_a) = q.pull("lane-a").await.unwrap().unwrap();
        let (_, val_b) = q.pull("lane-b").await.unwrap().unwrap();

        assert_eq!(val_a, json!(1));
        assert_eq!(val_b, json!(2));

        // Each lane is independent — both should now be empty
        assert!(q.pull("lane-a").await.unwrap().is_none());
        assert!(q.pull("lane-b").await.unwrap().is_none());
    }
}
