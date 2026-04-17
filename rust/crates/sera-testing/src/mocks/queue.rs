use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::Mutex;
use async_trait::async_trait;
use sera_queue::{QueueBackend, QueueError};

type QueueMap = HashMap<String, VecDeque<(String, serde_json::Value)>>;

/// In-memory mock queue backend for testing.
#[derive(Clone)]
pub struct MockQueueBackend {
    queues: Arc<Mutex<QueueMap>>,
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

    #[tokio::test]
    async fn mock_queue_len_and_is_empty() {
        let q = MockQueueBackend::new();

        // Unknown lane reports 0 / empty
        assert_eq!(q.len("x").await, 0);
        assert!(q.is_empty("x").await);

        q.push("x", json!(1)).await.unwrap();
        assert_eq!(q.len("x").await, 1);
        assert!(!q.is_empty("x").await);

        q.push("x", json!(2)).await.unwrap();
        assert_eq!(q.len("x").await, 2);

        q.pull("x").await.unwrap();
        assert_eq!(q.len("x").await, 1);

        q.pull("x").await.unwrap();
        assert_eq!(q.len("x").await, 0);
        assert!(q.is_empty("x").await);
    }

    #[tokio::test]
    async fn mock_queue_job_ids_are_unique_and_monotone() {
        let q = MockQueueBackend::new();
        let id1 = q.push("lane", json!(null)).await.unwrap();
        let id2 = q.push("lane", json!(null)).await.unwrap();
        let id3 = q.push("lane", json!(null)).await.unwrap();

        // All distinct
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);

        // Pulled IDs match push IDs in order
        let (pulled_id1, _) = q.pull("lane").await.unwrap().unwrap();
        let (pulled_id2, _) = q.pull("lane").await.unwrap().unwrap();
        let (pulled_id3, _) = q.pull("lane").await.unwrap().unwrap();
        assert_eq!(pulled_id1, id1);
        assert_eq!(pulled_id2, id2);
        assert_eq!(pulled_id3, id3);
    }

    #[tokio::test]
    async fn mock_queue_ack_nack_recover_are_noops() {
        let q = MockQueueBackend::new();
        let job_id = q.push("lane", json!(42)).await.unwrap();

        // ack and nack always succeed and do not remove items from the queue
        q.ack(&job_id).await.unwrap();
        q.nack(&job_id).await.unwrap();
        // recover_orphans always returns 0
        let recovered = q
            .recover_orphans(std::time::Duration::from_secs(30))
            .await
            .unwrap();
        assert_eq!(recovered, 0);
    }

    #[tokio::test]
    async fn mock_queue_default_is_empty_queue() {
        let q = MockQueueBackend::default();
        assert!(q.pull("any").await.unwrap().is_none());
        assert_eq!(q.len("any").await, 0);
    }

    #[tokio::test]
    async fn mock_queue_clone_shares_state() {
        let q1 = MockQueueBackend::new();
        let q2 = q1.clone();

        // Push via q1, pull via q2 — they share the same Arc
        let id = q1.push("shared", json!("value")).await.unwrap();
        let (pulled_id, pulled_val) = q2.pull("shared").await.unwrap().unwrap();
        assert_eq!(pulled_id, id);
        assert_eq!(pulled_val, json!("value"));

        // Now empty for both
        assert!(q1.pull("shared").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn mock_queue_drain_to_empty() {
        let q = MockQueueBackend::new();
        for i in 0..5u32 {
            q.push("drain", json!(i)).await.unwrap();
        }
        assert_eq!(q.len("drain").await, 5);

        // Drain all items
        let mut count = 0u32;
        while q.pull("drain").await.unwrap().is_some() {
            count += 1;
        }
        assert_eq!(count, 5);
        assert!(q.is_empty("drain").await);
    }
}
