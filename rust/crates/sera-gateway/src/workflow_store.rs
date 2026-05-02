//! Workflow task store — Wave E Phase 1 (sera-kgi8) + persistence (sera-d2xh) + GhRun gate (sera-4fel) + Change gate (sera-7ggi) + Human gate (sera-dgk1) + GhPr gate (sera-comg).
//!
//! Holds the set of pending [`WorkflowTask`]s the scheduler enumerates every
//! tick. Timer, GhRun, Change, Human, and GhPr gates are fully wired end-to-end; other `AwaitType`
//! variants are accepted at creation time but will not transition to "resolved"
//! until their dedicated gate beads land (Mail: sera-0zch).
//!
//! Two implementations ship today:
//! - [`InMemoryWorkflowTaskStore`] — `HashMap`-backed, used by tests and the
//!   default in-process gateway.
//! - [`SqliteWorkflowTaskStore`] — `rusqlite`-backed (sera-d2xh). Survives
//!   gateway restarts so a Timer or Human gate scheduled before a crash still
//!   resolves after recovery.
//!
//! # Storage boundary (ADR 2026-05-02)
//!
//! This trait owns the **gate / resume** lifecycle for runtime continuations
//! suspended on an `AwaitType` gate: `Pending → Resolved`, plus the
//! `agent_id` + `resume_token` routing pair the runtime supplies at suspension
//! time and consumes verbatim on the wake event.
//!
//! It is **not** a duplicate of `sera_workflow::WorkflowEngine` (and its
//! `SqliteWorkflowBackend`). That engine owns the orthogonal **claim /
//! dispatch** lifecycle (`Open → Hooked → InProgress → Closed`) plus the
//! stale-claim orphan reaper. A single [`WorkflowTask`] can in principle live
//! in both stores at once — they answer different questions over the same
//! payload. See `docs/public/decisions/2026-05-02-workflow-storage.md` for the
//! full boundary contract.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use sera_hitl::{ApprovalId, TicketStatus};
use sera_types::evolution::ChangeArtifactId;
use sera_workflow::task::{ChangeState, GhPrId, GhPrState, GhRunId, GhRunStatus};
use sera_workflow::WorkflowTask;
use sera_workflow::ready::HitlLookup;

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

impl SchedulerTaskStatus {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Resolved => "resolved",
        }
    }

    fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "resolved" => Some(Self::Resolved),
            _ => None,
        }
    }
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
/// The trait is `async` so backends with real I/O (e.g.
/// [`SqliteWorkflowTaskStore`]) fit naturally; the in-memory impl has no real
/// await points.
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

/// Process-local store backed by a `HashMap`. Sufficient for unit tests and
/// the default in-process gateway when no persistence path is configured.
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

// ── SQLite-backed store (sera-d2xh) ──────────────────────────────────────────

/// SQLite-backed [`WorkflowTaskStore`].
///
/// One row per `WorkflowTaskRecord`, keyed by hex task id. The full
/// [`WorkflowTask`] is serialised as JSON in the `task_json` column so
/// future additions to the task shape (new `AwaitType` variants, dependency
/// fields, …) round-trip without a schema migration. Scheduler-relevant
/// columns (`status`, `agent_id`, `resume_token`, `resolved_at`) are pulled
/// out as their own columns so the `list_pending` query can filter without
/// deserialising every row.
///
/// Concurrency: rusqlite `Connection` is `!Sync`, so we wrap it in a
/// `tokio::sync::Mutex` and hold the lock across the synchronous SQL call.
/// Workflow stores see human-cadence write traffic (gate creation + scheduler
/// resolutions every 5s), so a single shared connection is fine — no need to
/// reach for a connection pool.
pub struct SqliteWorkflowTaskStore {
    conn: Mutex<Connection>,
}

impl SqliteWorkflowTaskStore {
    /// Open (or create) a SQLite database at `path` and ensure the schema is
    /// in place.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        Self::initialize(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory SQLite database — useful for integration tests that
    /// want the SQLite code path without touching the filesystem.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::initialize(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn initialize(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS workflow_tasks (
                id           TEXT PRIMARY KEY,
                agent_id     TEXT NOT NULL,
                resume_token TEXT NOT NULL,
                status       TEXT NOT NULL CHECK (status IN ('pending', 'resolved')),
                resolved_at  TEXT,
                task_json    TEXT NOT NULL,
                created_at   TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_workflow_tasks_status
                ON workflow_tasks(status);
            ",
        )
    }

    /// Decode a row into a [`WorkflowTaskRecord`], surfacing JSON / status
    /// parse failures as `rusqlite::Error::FromSqlConversionFailure` so the
    /// caller doesn't need a separate error type.
    fn record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowTaskRecord> {
        let agent_id: String = row.get("agent_id")?;
        let resume_token: String = row.get("resume_token")?;
        let status_raw: String = row.get("status")?;
        let resolved_at_raw: Option<String> = row.get("resolved_at")?;
        let task_json: String = row.get("task_json")?;

        let status = SchedulerTaskStatus::from_db_str(&status_raw).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                format!("unknown scheduler status `{status_raw}`").into(),
            )
        })?;

        let resolved_at = match resolved_at_raw {
            None => None,
            Some(s) => Some(parse_rfc3339(&s).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Text,
                    e.into(),
                )
            })?),
        };

        let task: WorkflowTask = serde_json::from_str(&task_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })?;

        Ok(WorkflowTaskRecord {
            task,
            agent_id,
            resume_token,
            status,
            resolved_at,
        })
    }
}

fn parse_rfc3339(s: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| format!("invalid rfc3339 timestamp `{s}`: {e}"))
}

#[async_trait::async_trait]
impl WorkflowTaskStore for SqliteWorkflowTaskStore {
    async fn insert(&self, record: WorkflowTaskRecord) -> WorkflowTaskRecord {
        let conn = self.conn.lock().await;
        let id = record.task.id.to_string();
        let task_json = serde_json::to_string(&record.task)
            .expect("WorkflowTask serialisation is infallible by construction");
        let resolved_at_str = record.resolved_at.map(|t| t.to_rfc3339());
        // INSERT OR REPLACE preserves "insert returns the stored record"
        // semantics from the in-memory impl: the same task id replaces the
        // previous record verbatim. Callers that want a hard "no overwrite"
        // contract should `get` first.
        if let Err(e) = conn.execute(
            "INSERT OR REPLACE INTO workflow_tasks
                (id, agent_id, resume_token, status, resolved_at, task_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &id,
                &record.agent_id,
                &record.resume_token,
                record.status.as_db_str(),
                resolved_at_str,
                &task_json,
            ],
        ) {
            tracing::error!(
                event = "workflow_store_insert_failed",
                task_id = %id,
                error = %e,
                "SqliteWorkflowTaskStore::insert failed; record not persisted"
            );
        }
        record
    }

    async fn get(&self, id: &str) -> Option<WorkflowTaskRecord> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, agent_id, resume_token, status, resolved_at, task_json
                 FROM workflow_tasks WHERE id = ?1",
            )
            .ok()?;
        let mut rows = stmt
            .query_map(params![id], Self::record_from_row)
            .ok()?;
        rows.next().and_then(|r| r.ok())
    }

    async fn list(&self) -> Vec<WorkflowTaskRecord> {
        let conn = self.conn.lock().await;
        let mut stmt = match conn.prepare(
            "SELECT id, agent_id, resume_token, status, resolved_at, task_json
             FROM workflow_tasks
             ORDER BY created_at ASC, id ASC",
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    event = "workflow_store_list_prepare_failed",
                    error = %e,
                    "SqliteWorkflowTaskStore::list prepare failed"
                );
                return Vec::new();
            }
        };
        let iter = match stmt.query_map([], Self::record_from_row) {
            Ok(it) => it,
            Err(e) => {
                tracing::error!(
                    event = "workflow_store_list_query_failed",
                    error = %e,
                    "SqliteWorkflowTaskStore::list query failed"
                );
                return Vec::new();
            }
        };
        iter.filter_map(|r| match r {
            Ok(rec) => Some(rec),
            Err(e) => {
                tracing::warn!(
                    event = "workflow_store_list_decode_failed",
                    error = %e,
                    "SqliteWorkflowTaskStore::list skipping undecodable row"
                );
                None
            }
        })
        .collect()
    }

    async fn list_pending(&self) -> Vec<WorkflowTaskRecord> {
        let conn = self.conn.lock().await;
        let mut stmt = match conn.prepare(
            "SELECT id, agent_id, resume_token, status, resolved_at, task_json
             FROM workflow_tasks
             WHERE status = 'pending'
             ORDER BY created_at ASC, id ASC",
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    event = "workflow_store_list_pending_prepare_failed",
                    error = %e,
                    "SqliteWorkflowTaskStore::list_pending prepare failed"
                );
                return Vec::new();
            }
        };
        let iter = match stmt.query_map([], Self::record_from_row) {
            Ok(it) => it,
            Err(e) => {
                tracing::error!(
                    event = "workflow_store_list_pending_query_failed",
                    error = %e,
                    "SqliteWorkflowTaskStore::list_pending query failed"
                );
                return Vec::new();
            }
        };
        iter.filter_map(|r| match r {
            Ok(rec) => Some(rec),
            Err(e) => {
                tracing::warn!(
                    event = "workflow_store_list_pending_decode_failed",
                    error = %e,
                    "SqliteWorkflowTaskStore::list_pending skipping undecodable row; \
                     task remains in DB but will not be scheduled until row is fixed"
                );
                None
            }
        })
        .collect()
    }

    async fn mark_resolved(&self, id: &str, at: DateTime<Utc>) -> bool {
        let conn = self.conn.lock().await;
        let resolved_at_str = at.to_rfc3339();
        // Conditional UPDATE — only flip pending → resolved. The affected
        // row count tells us whether a real transition happened, matching
        // the in-memory impl's `Some(rec) if rec.status == Pending` arm.
        let updated = conn
            .execute(
                "UPDATE workflow_tasks
                 SET status = 'resolved', resolved_at = ?1
                 WHERE id = ?2 AND status = 'pending'",
                params![&resolved_at_str, id],
            )
            .unwrap_or(0);
        updated > 0
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;
    use sera_workflow::task::WorkflowTaskInput;
    use sera_workflow::{AwaitType, WorkflowTaskStatus, WorkflowTaskType};

    fn make_record(token: &str, secs_offset: i64) -> WorkflowTaskRecord {
        let now = Utc::now();
        let mut task = WorkflowTask::new(WorkflowTaskInput {
            title: format!("task-{token}"),
            description: "test".into(),
            acceptance_criteria: Vec::new(),
            status: WorkflowTaskStatus::Open,
            priority: 5,
            task_type: WorkflowTaskType::Meta,
            source_formula: None,
            source_location: None,
            created_at: now,
        });
        task.await_type = Some(AwaitType::Timer {
            not_before: now + ChronoDuration::seconds(secs_offset),
        });
        WorkflowTaskRecord {
            task,
            agent_id: "sera".into(),
            resume_token: token.into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        }
    }

    #[tokio::test]
    async fn sqlite_insert_get_round_trip() {
        let store = SqliteWorkflowTaskStore::open_in_memory().unwrap();
        let rec = make_record("rt-1", 60);
        let id = rec.task.id.to_string();
        store.insert(rec.clone()).await;

        let got = store.get(&id).await.expect("must round-trip");
        assert_eq!(got.task.id, rec.task.id);
        assert_eq!(got.agent_id, "sera");
        assert_eq!(got.resume_token, "rt-1");
        assert_eq!(got.status, SchedulerTaskStatus::Pending);
        assert!(got.resolved_at.is_none());
    }

    #[tokio::test]
    async fn sqlite_list_pending_filters_resolved() {
        let store = SqliteWorkflowTaskStore::open_in_memory().unwrap();
        let pending = make_record("p", 60);
        let resolved = make_record("r", 60);
        let resolved_id = resolved.task.id.to_string();
        store.insert(pending.clone()).await;
        store.insert(resolved.clone()).await;
        let resolved_at = Utc::now();
        assert!(store.mark_resolved(&resolved_id, resolved_at).await);

        let all = store.list().await;
        assert_eq!(all.len(), 2);
        let pend = store.list_pending().await;
        assert_eq!(pend.len(), 1);
        assert_eq!(pend[0].resume_token, "p");
    }

    #[tokio::test]
    async fn sqlite_mark_resolved_idempotent() {
        let store = SqliteWorkflowTaskStore::open_in_memory().unwrap();
        let rec = make_record("idem", 60);
        let id = rec.task.id.to_string();
        store.insert(rec).await;

        let at = Utc::now();
        assert!(store.mark_resolved(&id, at).await, "first transition wins");
        assert!(
            !store.mark_resolved(&id, at).await,
            "second transition is a no-op"
        );

        let got = store.get(&id).await.unwrap();
        assert_eq!(got.status, SchedulerTaskStatus::Resolved);
        assert!(got.resolved_at.is_some());
    }

    #[tokio::test]
    async fn sqlite_mark_resolved_unknown_id_returns_false() {
        let store = SqliteWorkflowTaskStore::open_in_memory().unwrap();
        assert!(!store.mark_resolved("nonexistent", Utc::now()).await);
    }

    #[tokio::test]
    async fn sqlite_get_unknown_returns_none() {
        let store = SqliteWorkflowTaskStore::open_in_memory().unwrap();
        assert!(store.get("nope").await.is_none());
    }

    #[tokio::test]
    async fn sqlite_persists_across_reopen() {
        // Open a file-backed DB, insert a record, drop the store, reopen,
        // confirm the record is still there with the same status. This is
        // the load-bearing claim of sera-d2xh: a Timer or Human gate
        // scheduled before a crash must survive recovery.
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("workflow.sqlite");

        let rec = make_record("survives-restart", 60);
        let id = rec.task.id.to_string();
        {
            let store = SqliteWorkflowTaskStore::open(&db_path).unwrap();
            store.insert(rec).await;
        }

        let store = SqliteWorkflowTaskStore::open(&db_path).unwrap();
        let got = store.get(&id).await.expect("record must survive reopen");
        assert_eq!(got.resume_token, "survives-restart");
        assert_eq!(got.status, SchedulerTaskStatus::Pending);
    }

    #[tokio::test]
    async fn sqlite_resolved_state_persists_across_reopen() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("workflow.sqlite");

        let rec = make_record("resolved-survives", 60);
        let id = rec.task.id.to_string();
        let resolved_at;
        {
            let store = SqliteWorkflowTaskStore::open(&db_path).unwrap();
            store.insert(rec).await;
            resolved_at = Utc::now();
            assert!(store.mark_resolved(&id, resolved_at).await);
        }

        let store = SqliteWorkflowTaskStore::open(&db_path).unwrap();
        let got = store.get(&id).await.unwrap();
        assert_eq!(got.status, SchedulerTaskStatus::Resolved);
        let restored = got.resolved_at.expect("resolved_at must round-trip");
        // RFC3339 round-trip is exact at the second/sub-second level for
        // chrono::DateTime<Utc>, so equality holds.
        assert_eq!(restored.timestamp(), resolved_at.timestamp());
    }
}

// ── GhRun state store (sera-4fel) ────────────────────────────────────────────

/// Persistence boundary for GitHub Actions run states consulted by the scheduler.
///
/// The scheduler snapshots this store each tick and wraps it in a
/// [`sera_workflow::ready::GhRunLookup`] impl so the ready-queue can evaluate
/// [`sera_workflow::task::AwaitType::GhRun`] gates without holding the async
/// store lock during the synchronous gate evaluation.
#[async_trait::async_trait]
pub trait GhRunStateStore: Send + Sync {
    /// Record (or overwrite) the current [`GhRunStatus`] for `run_id`.
    async fn upsert(&self, run_id: GhRunId, status: GhRunStatus);

    /// Return the current status for `run_id`, or `None` when unknown.
    async fn get_status(&self, run_id: &GhRunId) -> Option<GhRunStatus>;

    /// Snapshot all entries as a `HashMap` for the synchronous
    /// [`sera_workflow::ready::GhRunLookup`] bridge in the scheduler.
    async fn snapshot(&self) -> HashMap<String, GhRunStatus>;
}

/// Process-local GhRun state store backed by a `HashMap`.
#[derive(Default)]
pub struct InMemoryGhRunStateStore {
    inner: RwLock<HashMap<String, GhRunStatus>>,
}

impl InMemoryGhRunStateStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[async_trait::async_trait]
impl GhRunStateStore for InMemoryGhRunStateStore {
    async fn upsert(&self, run_id: GhRunId, status: GhRunStatus) {
        let mut map = self.inner.write().await;
        map.insert(run_id.0, status);
    }

    async fn get_status(&self, run_id: &GhRunId) -> Option<GhRunStatus> {
        let map = self.inner.read().await;
        map.get(run_id.as_str()).copied()
    }

    async fn snapshot(&self) -> HashMap<String, GhRunStatus> {
        let map = self.inner.read().await;
        map.clone()
    }
}

// ── Change artifact state store (sera-7ggi) ──────────────────────────────────

/// Persistence boundary for change-artifact states consulted by the scheduler.
///
/// The scheduler snapshots this store each tick and wraps it in a
/// [`sera_workflow::ready::ChangeLookup`] impl so the ready-queue can evaluate
/// [`sera_workflow::task::AwaitType::Change`] gates without holding the async
/// store lock during the synchronous gate evaluation.
///
/// Phase 1 ships [`InMemoryChangeArtifactStateStore`] only — the production
/// change-artifact poller (sera-meta integration) that pushes state changes
/// here lives in a follow-up bead. Test code drives the store directly to
/// exercise the wait-then-resume round trip.
#[async_trait::async_trait]
pub trait ChangeArtifactStateStore: Send + Sync {
    /// Record (or overwrite) the current [`ChangeState`] for `artifact_id`.
    async fn upsert(&self, artifact_id: ChangeArtifactId, state: ChangeState);

    /// Return the current state for `artifact_id`, or `None` when unknown.
    async fn get_state(&self, artifact_id: &ChangeArtifactId) -> Option<ChangeState>;

    /// Snapshot all entries as a `HashMap` keyed on the raw artifact hash for
    /// the synchronous [`sera_workflow::ready::ChangeLookup`] bridge in the
    /// scheduler.
    async fn snapshot(&self) -> HashMap<[u8; 32], ChangeState>;
}

/// Process-local change-artifact state store backed by a `HashMap`.
#[derive(Default)]
pub struct InMemoryChangeArtifactStateStore {
    inner: RwLock<HashMap<[u8; 32], ChangeState>>,
}

impl InMemoryChangeArtifactStateStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[async_trait::async_trait]
impl ChangeArtifactStateStore for InMemoryChangeArtifactStateStore {
    async fn upsert(&self, artifact_id: ChangeArtifactId, state: ChangeState) {
        let mut map = self.inner.write().await;
        map.insert(artifact_id.hash, state);
    }

    async fn get_state(&self, artifact_id: &ChangeArtifactId) -> Option<ChangeState> {
        let map = self.inner.read().await;
        map.get(&artifact_id.hash).cloned()
    }

    async fn snapshot(&self) -> HashMap<[u8; 32], ChangeState> {
        let map = self.inner.read().await;
        map.clone()
    }
}

// ── Human gate store (sera-dgk1) ─────────────────────────────────────────────

/// Async key-value store mapping [`ApprovalId`] → [`TicketStatus`].
///
/// The scheduler snapshots this store synchronously before each tick via
/// [`HumanGateStore::snapshot`] and wraps the result in a
/// [`SnapshotHitlLookup`], satisfying the synchronous [`HitlLookup`] contract
/// required by `sera_workflow::ready::ReadyContext`.
#[async_trait::async_trait]
pub trait HumanGateStore: Send + Sync {
    /// Record a terminal ticket status for `approval_id`.
    async fn set_ticket_status(&self, approval_id: ApprovalId, status: TicketStatus);

    /// Look up the current status for `approval_id`. Returns `None` when no
    /// status has been recorded yet.
    async fn get_ticket_status(&self, approval_id: &ApprovalId) -> Option<TicketStatus>;

    /// Return a point-in-time snapshot of all recorded statuses. Used by the
    /// scheduler to construct a [`SnapshotHitlLookup`] before the tick.
    async fn snapshot(&self) -> HashMap<String, TicketStatus>;
}

/// In-memory implementation of [`HumanGateStore`]. Sufficient for Phase 1.
#[derive(Default)]
pub struct InMemoryHumanGateStore {
    inner: RwLock<HashMap<String, TicketStatus>>,
}

impl InMemoryHumanGateStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[async_trait::async_trait]
impl HumanGateStore for InMemoryHumanGateStore {
    async fn set_ticket_status(&self, approval_id: ApprovalId, status: TicketStatus) {
        let mut map = self.inner.write().await;
        map.insert(approval_id.to_string(), status);
    }

    async fn get_ticket_status(&self, approval_id: &ApprovalId) -> Option<TicketStatus> {
        let map = self.inner.read().await;
        map.get(approval_id.as_str()).copied()
    }

    async fn snapshot(&self) -> HashMap<String, TicketStatus> {
        let map = self.inner.read().await;
        map.clone()
    }
}

/// Synchronous [`HitlLookup`] backed by a pre-taken snapshot of the
/// [`HumanGateStore`].
///
/// The scheduler calls [`HumanGateStore::snapshot`] once per tick to capture
/// the current ticket statuses, then wraps the result in this struct so the
/// pure-synchronous `ready_tasks_with_context` can consult it without async.
pub struct SnapshotHitlLookup {
    snapshot: HashMap<String, TicketStatus>,
}

impl SnapshotHitlLookup {
    pub fn new(snapshot: HashMap<String, TicketStatus>) -> Self {
        Self { snapshot }
    }
}

impl HitlLookup for SnapshotHitlLookup {
    fn ticket_status(&self, id: &ApprovalId) -> Option<TicketStatus> {
        self.snapshot.get(id.as_str()).copied()
    }
}

// ── GitHub PR state store (sera-comg) ────────────────────────────────────────

/// Persistence boundary for GitHub PR states consulted by the scheduler.
///
/// The scheduler snapshots this store each tick and wraps it in a
/// [`sera_workflow::ready::GhPrLookup`] impl so the ready-queue can evaluate
/// [`sera_workflow::task::AwaitType::GhPr`] gates without holding the async
/// store lock during the synchronous gate evaluation.
///
/// Phase 1 ships [`InMemoryGhPrStateStore`] only — the production GitHub API
/// poller that pushes state changes here lives in a follow-up bead. Test code
/// drives the store directly to exercise the wait-then-resume round trip.
#[async_trait::async_trait]
pub trait GhPrStateStore: Send + Sync {
    /// Record (or overwrite) the current [`GhPrState`] for `pr_id`.
    async fn upsert(&self, pr_id: GhPrId, state: GhPrState);

    /// Return the current state for `pr_id`, or `None` when unknown.
    async fn get_state(&self, pr_id: &GhPrId) -> Option<GhPrState>;

    /// Snapshot all entries as a `HashMap<String, GhPrState>` for the
    /// synchronous [`sera_workflow::ready::GhPrLookup`] bridge in the
    /// scheduler. Keyed by the underlying PR id string so the scheduler does
    /// not need to clone full [`GhPrId`] values.
    async fn snapshot(&self) -> HashMap<String, GhPrState>;
}

/// Process-local GitHub PR state store backed by a `HashMap`.
///
/// Used as a mock in tests and as the default in-process backend until the
/// production GitHub API poller lands.
#[derive(Default)]
pub struct InMemoryGhPrStateStore {
    inner: RwLock<HashMap<String, GhPrState>>,
}

impl InMemoryGhPrStateStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[async_trait::async_trait]
impl GhPrStateStore for InMemoryGhPrStateStore {
    async fn upsert(&self, pr_id: GhPrId, state: GhPrState) {
        let mut map = self.inner.write().await;
        map.insert(pr_id.0, state);
    }

    async fn get_state(&self, pr_id: &GhPrId) -> Option<GhPrState> {
        let map = self.inner.read().await;
        map.get(pr_id.as_str()).cloned()
    }

    async fn snapshot(&self) -> HashMap<String, GhPrState> {
        let map = self.inner.read().await;
        map.clone()
    }
}
