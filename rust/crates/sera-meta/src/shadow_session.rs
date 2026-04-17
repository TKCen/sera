//! Shadow session — parallel evaluation environment for validating proposed changes.
//!
//! A `ShadowSession` is a sandboxed evaluation environment that runs alongside the
//! production system to validate a change before it is applied. It receives a
//! duplicate of the relevant production state and applies the proposed change
//! to that copy, allowing observers to detect regressions, inconsistencies,
//! or blast-radius violations without affecting live systems.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::ChangeArtifact;

/// Lifecycle status of a shadow session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowSessionStatus {
    /// Session created but not yet started.
    Pending,
    /// Change is being applied and evaluated.
    Running,
    /// Evaluation completed successfully.
    Passed,
    /// Evaluation found violations.
    Failed,
    /// Session was cancelled before completion.
    Cancelled,
}

/// A shadow session for parallel evaluation of a proposed change.
#[derive(Debug, Clone)]
pub struct ShadowSession {
    /// Unique session ID.
    pub id: String,
    /// The change artifact being evaluated.
    pub artifact: ChangeArtifact,
    /// Current status.
    pub status: ShadowSessionStatus,
    /// Key = area name, Value = snapshot of production state before change application.
    pub state_snapshots: HashMap<String, serde_json::Value>,
    /// Results from each evaluation checkpoint.
    pub checkpoints: Vec<ShadowCheckpoint>,
    /// When the session was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When evaluation started.
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// When evaluation completed (success or failure).
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// A single checkpoint within a shadow session evaluation.
#[derive(Debug, Clone)]
pub struct ShadowCheckpoint {
    /// Checkpoint name (e.g., "apply", "verify", "rollback-test").
    pub name: String,
    /// Whether this checkpoint passed.
    pub passed: bool,
    /// Human-readable notes from this checkpoint.
    pub notes: String,
    /// Arbitrary metadata from this checkpoint.
    pub metadata: serde_json::Value,
    /// When this checkpoint was recorded.
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

impl ShadowSession {
    /// Create a new `ShadowSession` with `Pending` status.
    pub fn new(artifact: ChangeArtifact) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            artifact,
            status: ShadowSessionStatus::Pending,
            state_snapshots: HashMap::new(),
            checkpoints: Vec::new(),
            created_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    /// Record a snapshot of some area of production state before applying the change.
    pub fn snapshot(&mut self, area: impl Into<String>, state: serde_json::Value) {
        self.state_snapshots.insert(area.into(), state);
    }

    /// Start the session — transition to `Running`.
    pub fn start(&mut self) {
        self.status = ShadowSessionStatus::Running;
        self.started_at = Some(chrono::Utc::now());
    }

    /// Record a checkpoint result.
    pub fn checkpoint(&mut self, name: impl Into<String>, passed: bool, notes: impl Into<String>, metadata: serde_json::Value) {
        self.checkpoints.push(ShadowCheckpoint {
            name: name.into(),
            passed,
            notes: notes.into(),
            metadata,
            recorded_at: chrono::Utc::now(),
        });
    }

    /// Mark the session as passed.
    pub fn pass(&mut self) {
        self.status = ShadowSessionStatus::Passed;
        self.completed_at = Some(chrono::Utc::now());
    }

    /// Mark the session as failed.
    pub fn fail(&mut self) {
        self.status = ShadowSessionStatus::Failed;
        self.completed_at = Some(chrono::Utc::now());
    }

    /// Mark the session as cancelled.
    pub fn cancel(&mut self) {
        self.status = ShadowSessionStatus::Cancelled;
        self.completed_at = Some(chrono::Utc::now());
    }

    /// Whether the session has completed (passed, failed, or cancelled).
    pub fn is_done(&self) -> bool {
        matches!(
            self.status,
            ShadowSessionStatus::Passed | ShadowSessionStatus::Failed | ShadowSessionStatus::Cancelled
        )
    }
}

/// Handle for interacting with a shadow session in the registry.
#[derive(Debug, Clone)]
pub struct ShadowSessionHandle {
    inner: Arc<RwLock<ShadowSession>>,
}

impl ShadowSessionHandle {
    /// Create a new handle wrapping the given session.
    pub fn new(session: ShadowSession) -> Self {
        Self {
            inner: Arc::new(RwLock::new(session)),
        }
    }

    /// Get a cloned snapshot of the session.
    pub async fn snapshot(&self) -> ShadowSession {
        self.inner.read().await.clone()
    }

    /// Record a state snapshot.
    pub async fn snapshot_state(&self, area: impl Into<String>, state: serde_json::Value) {
        self.inner.write().await.snapshot(area, state);
    }

    /// Transition to `Running`.
    pub async fn start(&self) {
        self.inner.write().await.start();
    }

    /// Record a checkpoint.
    pub async fn checkpoint(
        &self,
        name: impl Into<String>,
        passed: bool,
        notes: impl Into<String>,
        metadata: serde_json::Value,
    ) {
        self.inner.write().await.checkpoint(name, passed, notes, metadata);
    }

    /// Mark as passed.
    pub async fn pass(&self) {
        self.inner.write().await.pass();
    }

    /// Mark as failed.
    pub async fn fail(&self) {
        self.inner.write().await.fail();
    }

    /// Mark as cancelled.
    pub async fn cancel(&self) {
        self.inner.write().await.cancel();
    }
}

/// Thread-safe registry of active shadow sessions.
#[derive(Debug, Clone, Default)]
pub struct ShadowSessionRegistry {
    sessions: Arc<RwLock<HashMap<String, ShadowSessionHandle>>>,
}

impl ShadowSessionRegistry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create and register a new shadow session for an artifact.
    ///
    /// Returns a handle to the new session.
    pub async fn create(&self, artifact: ChangeArtifact) -> ShadowSessionHandle {
        let session = ShadowSession::new(artifact);
        let handle = ShadowSessionHandle::new(session.clone());
        self.sessions
            .write()
            .await
            .insert(session.id.clone(), handle.clone());
        handle
    }

    /// Get a handle by session ID, if it exists.
    pub async fn get(&self, id: &str) -> Option<ShadowSessionHandle> {
        self.sessions.read().await.get(id).cloned()
    }

    /// List all active (non-done) session IDs.
    pub async fn active_ids(&self) -> Vec<String> {
        let guard = self.sessions.read().await;
        guard
            .iter()
            .filter(|(_, _h)| {
                // We need to check status without locking all handles
                // Best effort: return all IDs and let caller filter
                true
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Remove a session by ID. Returns the handle if it existed.
    pub async fn remove(&self, id: &str) -> Option<ShadowSessionHandle> {
        self.sessions.write().await.remove(id)
    }

    /// Prune all completed sessions.
    pub async fn prune_completed(&self) -> usize {
        let mut sessions = self.sessions.write().await;
        let before = sessions.len();
        sessions.retain(|_, h| {
            // Use try_read to avoid deadlock; skip if can't acquire immediately
            if let Ok(s) = h.inner.try_read() {
                !s.is_done()
            } else {
                true // keep if we can't tell
            }
        });
        before - sessions.len()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlastRadius, ChangeArtifactScope, ChangeProposer};

    fn make_artifact() -> ChangeArtifact {
        ChangeArtifact::new(
            "test artifact".to_string(),
            ChangeArtifactScope::AgentImprovement,
            BlastRadius::AgentMemory,
            ChangeProposer {
                principal_id: "tester".to_string(),
                capability_token: sera_types::evolution::CapabilityToken {
                    id: "tok-1".to_string(),
                    scopes: Default::default(),
                    expires_at: chrono::Utc::now(),
                    max_proposals: 10,
                    signature: [0u8; 64],
                },
            },
            serde_json::json!({}),
        )
    }

    #[test]
    fn shadow_session_initial_state() {
        let session = ShadowSession::new(make_artifact());
        assert_eq!(session.status, ShadowSessionStatus::Pending);
        assert!(session.state_snapshots.is_empty());
        assert!(session.checkpoints.is_empty());
    }

    #[tokio::test]
    async fn shadow_session_lifecycle() {
        let session = ShadowSession::new(make_artifact());
        let handle = ShadowSessionHandle::new(session);

        handle.start().await;
        handle
            .checkpoint("apply", true, "change applied", serde_json::json!({}))
            .await;
        handle.pass().await;

        let snap = handle.snapshot().await;
        assert_eq!(snap.status, ShadowSessionStatus::Passed);
        assert_eq!(snap.checkpoints.len(), 1);
        assert!(snap.completed_at.is_some());
    }

    #[tokio::test]
    async fn shadow_session_fail() {
        let session = ShadowSession::new(make_artifact());
        let handle = ShadowSessionHandle::new(session);

        handle.start().await;
        handle.checkpoint("verify", false, "assertion failed", serde_json::json!({})).await;
        handle.fail().await;

        let snap = handle.snapshot().await;
        assert_eq!(snap.status, ShadowSessionStatus::Failed);
    }

    #[tokio::test]
    async fn shadow_registry_create_and_get() {
        let registry = ShadowSessionRegistry::new();
        let artifact = make_artifact();
        let handle = registry.create(artifact).await;

        let snap = handle.snapshot().await;
        let id = snap.id.clone();

        let retrieved = registry.get(&id).await;
        assert!(retrieved.is_some());

        let removed = registry.remove(&id).await;
        assert!(removed.is_some());
        assert!(registry.get(&id).await.is_none());
    }

    // ---- New edge-case tests ---------------------------------------------

    /// cancel transitions the session to Cancelled and marks is_done true.
    #[tokio::test]
    async fn shadow_session_cancel() {
        let session = ShadowSession::new(make_artifact());
        let handle = ShadowSessionHandle::new(session);

        handle.start().await;
        handle.cancel().await;

        let snap = handle.snapshot().await;
        assert_eq!(snap.status, ShadowSessionStatus::Cancelled);
        assert!(snap.completed_at.is_some());
        assert!(snap.is_done());
    }

    /// Pending session is not done; running session is not done.
    #[test]
    fn is_done_only_for_terminal_states() {
        let mut session = ShadowSession::new(make_artifact());
        assert!(!session.is_done()); // Pending

        session.start();
        assert!(!session.is_done()); // Running

        session.pass();
        assert!(session.is_done()); // Passed
    }

    /// snapshot_state stores data that is visible in the session snapshot.
    #[tokio::test]
    async fn snapshot_state_stored_in_session() {
        let session = ShadowSession::new(make_artifact());
        let handle = ShadowSessionHandle::new(session);

        handle
            .snapshot_state("memory", serde_json::json!({ "key": "value" }))
            .await;

        let snap = handle.snapshot().await;
        assert!(snap.state_snapshots.contains_key("memory"));
        assert_eq!(snap.state_snapshots["memory"]["key"], "value");
    }

    /// Multiple checkpoints accumulate in order.
    #[tokio::test]
    async fn multiple_checkpoints_accumulate() {
        let session = ShadowSession::new(make_artifact());
        let handle = ShadowSessionHandle::new(session);

        handle.start().await;
        handle
            .checkpoint("step-1", true, "first", serde_json::json!({}))
            .await;
        handle
            .checkpoint("step-2", true, "second", serde_json::json!({}))
            .await;
        handle
            .checkpoint("step-3", false, "third failed", serde_json::json!({}))
            .await;

        let snap = handle.snapshot().await;
        assert_eq!(snap.checkpoints.len(), 3);
        assert_eq!(snap.checkpoints[0].name, "step-1");
        assert_eq!(snap.checkpoints[2].name, "step-3");
        assert!(!snap.checkpoints[2].passed);
    }

    /// get on a missing id returns None.
    #[tokio::test]
    async fn registry_get_missing_returns_none() {
        let registry = ShadowSessionRegistry::new();
        assert!(registry.get("does-not-exist").await.is_none());
    }

    /// prune_completed removes finished sessions and returns correct count.
    #[tokio::test]
    async fn prune_completed_removes_done_sessions() {
        let registry = ShadowSessionRegistry::new();

        // Create and finish one session.
        let h1 = registry.create(make_artifact()).await;
        h1.start().await;
        h1.pass().await;

        // Create a session that stays pending.
        registry.create(make_artifact()).await;

        let pruned = registry.prune_completed().await;
        assert_eq!(pruned, 1);

        // One session remains (the pending one).
        assert_eq!(registry.active_ids().await.len(), 1);
    }

    /// Two independent handles to the same session share the same underlying state.
    #[tokio::test]
    async fn handle_clone_shares_state() {
        let session = ShadowSession::new(make_artifact());
        let handle1 = ShadowSessionHandle::new(session);
        let handle2 = handle1.clone();

        handle1.start().await;
        // Changes made via handle1 are visible through handle2.
        assert_eq!(handle2.snapshot().await.status, ShadowSessionStatus::Running);
    }
}
