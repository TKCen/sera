//! Workflow task store — Wave E Phase 1 (sera-kgi8).
//!
//! Holds the set of pending [`WorkflowTask`]s the scheduler enumerates every
//! tick. Only Timer-gated tasks are fully wired end-to-end in Phase 1; other
//! `AwaitType` variants are accepted at creation time but will not transition
//! to "resolved" until their dedicated gate beads land (Human: sera-dgk1,
//! GhPr: sera-comg, GhRun: sera-4fel, Change: sera-7ggi, Mail: sera-0zch).
//!
//! Persistent storage is a follow-up; Phase 1 uses an in-memory store only.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use sera_workflow::WorkflowTask;

/// Lifecycle status of a [`WorkflowTaskRecord`] as seen by the scheduler.
///
/// Distinct from `WorkflowTaskStatus` in sera-workflow: that enum mirrors the
/// beads Issue surface; this one captures the narrower "am I still waiting
/// for my gate?" view that the scheduler cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulerTaskStatus {
    /// Task is waiting for its gate to resolve.
    Pending,
    /// Gate resolved — scheduler has emitted the wake event.
    Resolved,
}

/// Envelope wrapping a [`WorkflowTask`] with the scheduler-side metadata the
/// gateway needs to route a resolution back to the originating agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTaskRecord {
    pub task: WorkflowTask,
    /// Logical agent identifier that created the task. Surfaced on the
    /// wake event so the runtime can re-entry the correct session.
    pub agent_id: String,
    /// Opaque token the runtime hands out at suspension time. Returned
    /// verbatim on the wake event so the runtime can correlate the resume
    /// back to the paused continuation.
    pub resume_token: String,
    pub status: SchedulerTaskStatus,
    /// Set once when the scheduler observes the gate as ready.
    pub resolved_at: Option<DateTime<Utc>>,
}

/// Persistence boundary for workflow tasks owned by the scheduler.
///
/// Phase 1 ships [`InMemoryWorkflowTaskStore`] only; a SQLite-backed impl is
/// the follow-up bead. The trait is `async` for forward-compatibility with
/// that store — the in-memory impl has no real await points.
#[async_trait::async_trait]
pub trait WorkflowTaskStore: Send + Sync {
    /// Insert a freshly minted record. Returns the stored record so the
    /// caller has the canonical view including the computed task id.
    async fn insert(&self, record: WorkflowTaskRecord) -> WorkflowTaskRecord;

    /// Fetch a record by hex task id. Returns `None` when unknown.
    async fn get(&self, id: &str) -> Option<WorkflowTaskRecord>;

    /// Snapshot every record currently in the store.
    async fn list(&self) -> Vec<WorkflowTaskRecord>;

    /// Snapshot every record whose status is [`SchedulerTaskStatus::Pending`].
    /// Dedicated method so the scheduler does not have to filter client-side.
    async fn list_pending(&self) -> Vec<WorkflowTaskRecord>;

    /// Mark the record identified by `id` as resolved at `at`. Returns `true`
    /// when a transition actually occurred (record existed and was pending).
    async fn mark_resolved(&self, id: &str, at: DateTime<Utc>) -> bool;
}

/// Process-local store backed by a `HashMap`. Sufficient for Phase 1.
#[derive(Default)]
pub struct InMemoryWorkflowTaskStore {
    inner: RwLock<HashMap<String, WorkflowTaskRecord>>,
}

impl InMemoryWorkflowTaskStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[async_trait::async_trait]
impl WorkflowTaskStore for InMemoryWorkflowTaskStore {
    async fn insert(&self, record: WorkflowTaskRecord) -> WorkflowTaskRecord {
        let mut map = self.inner.write().await;
        let key = record.task.id.to_string();
        map.insert(key, record.clone());
        record
    }

    async fn get(&self, id: &str) -> Option<WorkflowTaskRecord> {
        let map = self.inner.read().await;
        map.get(id).cloned()
    }

    async fn list(&self) -> Vec<WorkflowTaskRecord> {
        let map = self.inner.read().await;
        map.values().cloned().collect()
    }

    async fn list_pending(&self) -> Vec<WorkflowTaskRecord> {
        let map = self.inner.read().await;
        map.values()
            .filter(|r| r.status == SchedulerTaskStatus::Pending)
            .cloned()
            .collect()
    }

    async fn mark_resolved(&self, id: &str, at: DateTime<Utc>) -> bool {
        let mut map = self.inner.write().await;
        match map.get_mut(id) {
            Some(rec) if rec.status == SchedulerTaskStatus::Pending => {
                rec.status = SchedulerTaskStatus::Resolved;
                rec.resolved_at = Some(at);
                true
            }
            _ => false,
        }
    }
}
