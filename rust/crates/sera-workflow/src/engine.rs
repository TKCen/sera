//! `WorkflowEngine` — async orchestrator with durable persistence.
//!
//! Phase-1 surface from SPEC-workflow-engine §4a–§4d:
//!
//! - [`WorkflowEngine::submit_task`] — persist a freshly constructed
//!   [`WorkflowTask`].
//! - [`WorkflowEngine::claim_next_ready`] — atomic CAS that flips the lowest-
//!   priority Open task to Hooked and returns a [`ClaimToken`].
//! - [`WorkflowEngine::mark_complete`] / [`WorkflowEngine::mark_failed`] —
//!   terminal transitions with attached result / failure reason.
//! - [`WorkflowEngine::recover_orphans`] — Paperclip-style orphan reaper for
//!   [`WorkflowTaskStatus::Hooked`] tasks whose claim heartbeat is older than
//!   the configured cutoff.
//!
//! The engine is backend-agnostic via [`WorkflowEngineBackend`]. Two backends
//! ship here:
//!
//! - [`SqliteWorkflowBackend`] — durable, file-backed (or `:memory:`) store via
//!   `rusqlite`. All I/O is offloaded to `tokio::task::spawn_blocking` so the
//!   public async surface is preserved without pulling in `sqlx`.
//! - [`MemoryWorkflowBackend`] — `Vec<WorkflowTask>` behind a mutex, reusing
//!   the pure-function helpers in [`ready`](crate::ready) / [`claim`](crate::claim).
//!   Useful for tests and single-process fixtures.
//!
//! # SERA ADR — rusqlite (not sqlx)
//!
//! SPEC-workflow-engine §4b mentions `sqlx` as an implementation example, but
//! the SERA project ADR is rusqlite (see `.omc/wiki/sqlite-via-rusqlite.md`
//! and every sera-db module). We honour the project ADR: the engine is sync
//! under the hood, wrapped with `spawn_blocking` at the async boundary.
//!
//! Claims are atomic via `UPDATE ... WHERE status='open' ... RETURNING id` in a
//! single SQL statement — no application-level locks needed.

use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;
use uuid::Uuid;

use crate::claim::{claim_task as claim_task_in_memory, ClaimError, ClaimToken};
use crate::ready::ready_tasks;
use crate::task::{WorkflowTask, WorkflowTaskId, WorkflowTaskStatus};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors surfaced by the [`WorkflowEngine`] and its backends.
#[derive(Debug, Error)]
pub enum EngineError {
    /// A claim-protocol error bubbling up from the in-memory helpers.
    #[error(transparent)]
    Claim(#[from] ClaimError),

    /// A storage-layer failure (sqlite I/O, serialization, etc.).
    #[error("storage error: {0}")]
    Storage(String),

    /// The requested task was not found in the backing store.
    #[error("task not found: {0}")]
    NotFound(WorkflowTaskId),

    /// A background worker panicked while holding a `spawn_blocking` slot.
    #[error("background join error: {0}")]
    Join(String),
}

impl From<rusqlite::Error> for EngineError {
    fn from(e: rusqlite::Error) -> Self {
        EngineError::Storage(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Backend trait
// ---------------------------------------------------------------------------

/// Storage contract for [`WorkflowEngine`]. One impl per durability tier.
///
/// Implementations are expected to be internally thread-safe — `WorkflowEngine`
/// wraps them in an `Arc` and calls their methods from multiple tasks.
#[async_trait]
pub trait WorkflowEngineBackend: Send + Sync {
    /// Persist a new task. Returns its id.
    ///
    /// Submitting an id that already exists overwrites the stored row; this
    /// matches the beads-style content-hash identity model where the id IS the
    /// identity of the task content.
    async fn submit_task(&self, task: WorkflowTask) -> Result<WorkflowTaskId, EngineError>;

    /// Atomically claim the next ready task (priority ASC, then id bytes).
    ///
    /// Returns `None` when no Open tasks are ready. Ready semantics here are
    /// intentionally simpler than [`ready_tasks`](crate::ready::ready_tasks):
    /// we only consider `status = Open` and `defer_until <= now`. Dependency
    /// and external-gate evaluation belongs to a caller-held in-memory view
    /// (sera-workflow is explicit about this in `ready.rs`).
    ///
    /// The CAS flips the task to [`WorkflowTaskStatus::Hooked`] in the same
    /// statement, so two concurrent callers cannot both observe the same id.
    async fn claim_next_ready(
        &self,
        agent_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<ClaimToken>, EngineError>;

    /// Terminal success transition: `Hooked | InProgress -> Closed`.
    async fn mark_complete(
        &self,
        id: &WorkflowTaskId,
        result: serde_json::Value,
    ) -> Result<(), EngineError>;

    /// Terminal failure transition: `Hooked | InProgress -> Blocked`.
    ///
    /// NOTE: the SPEC reserves `Closed` for successful completion, so failures
    /// flip to `Blocked` with a recorded `failure_reason`. Operators can re-
    /// open a failed task by updating its status back to `Open` externally.
    async fn mark_failed(
        &self,
        id: &WorkflowTaskId,
        reason: &str,
    ) -> Result<(), EngineError>;

    /// Reset any `Hooked` task whose claim heartbeat is `<= cutoff` back to
    /// `Open` and return the ids that were reset. Paperclip orphan-reap layer.
    async fn recover_orphans(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<WorkflowTaskId>, EngineError>;

    /// Load a single task by id. Returns [`EngineError::NotFound`] if absent.
    async fn load(&self, id: &WorkflowTaskId) -> Result<WorkflowTask, EngineError>;
}

// ---------------------------------------------------------------------------
// WorkflowEngine
// ---------------------------------------------------------------------------

/// Facade over a [`WorkflowEngineBackend`] — the object agents use.
///
/// Cheap to clone (wraps `Arc<dyn WorkflowEngineBackend>`).
#[derive(Clone)]
pub struct WorkflowEngine {
    backend: Arc<dyn WorkflowEngineBackend>,
}

impl WorkflowEngine {
    /// Construct an engine from any backend implementation.
    pub fn new(backend: Arc<dyn WorkflowEngineBackend>) -> Self {
        Self { backend }
    }

    /// Submit a fresh task to the backing store.
    pub async fn submit_task(
        &self,
        task: WorkflowTask,
    ) -> Result<WorkflowTaskId, EngineError> {
        self.backend.submit_task(task).await
    }

    /// Claim the next ready task atomically.
    pub async fn claim_next_ready(
        &self,
        agent_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<ClaimToken>, EngineError> {
        self.backend.claim_next_ready(agent_id, now).await
    }

    /// Mark a task complete.
    pub async fn mark_complete(
        &self,
        id: &WorkflowTaskId,
        result: serde_json::Value,
    ) -> Result<(), EngineError> {
        self.backend.mark_complete(id, result).await
    }

    /// Mark a task failed.
    pub async fn mark_failed(
        &self,
        id: &WorkflowTaskId,
        reason: &str,
    ) -> Result<(), EngineError> {
        self.backend.mark_failed(id, reason).await
    }

    /// Recover orphaned (stale-claim) tasks.
    pub async fn recover_orphans(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<WorkflowTaskId>, EngineError> {
        self.backend.recover_orphans(cutoff).await
    }

    /// Load a task.
    pub async fn load(&self, id: &WorkflowTaskId) -> Result<WorkflowTask, EngineError> {
        self.backend.load(id).await
    }
}

// ---------------------------------------------------------------------------
// SQL status tokens
// ---------------------------------------------------------------------------

fn status_to_str(status: WorkflowTaskStatus) -> &'static str {
    match status {
        WorkflowTaskStatus::Open => "open",
        WorkflowTaskStatus::InProgress => "in_progress",
        WorkflowTaskStatus::Hooked => "hooked",
        WorkflowTaskStatus::Blocked => "blocked",
        WorkflowTaskStatus::Deferred => "deferred",
        WorkflowTaskStatus::Closed => "closed",
        WorkflowTaskStatus::Pinned => "pinned",
    }
}

// ---------------------------------------------------------------------------
// SqliteWorkflowBackend
// ---------------------------------------------------------------------------

/// Durable rusqlite-backed implementation of [`WorkflowEngineBackend`].
///
/// The connection is wrapped in `Arc<Mutex<Connection>>` because rusqlite is
/// synchronous; every public method offloads to `tokio::task::spawn_blocking`.
/// This matches the pattern established in [`sera-db`].
pub struct SqliteWorkflowBackend {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteWorkflowBackend {
    /// Open or create a database file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EngineError> {
        let conn = Connection::open(path.as_ref())?;
        Self::initialize(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Open an in-memory backend — for tests.
    pub fn open_in_memory() -> Result<Self, EngineError> {
        let conn = Connection::open_in_memory()?;
        Self::initialize(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn initialize(conn: &Connection) -> Result<(), EngineError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workflow_tasks (
                id               BLOB PRIMARY KEY,
                status           TEXT NOT NULL,
                priority         INTEGER NOT NULL,
                payload          BLOB NOT NULL,
                defer_until      INTEGER NULL,
                claimed_at       INTEGER NULL,
                claimed_by       TEXT NULL,
                result           BLOB NULL,
                failure_reason   TEXT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_workflow_tasks_status_priority
                ON workflow_tasks(status, priority);
            CREATE INDEX IF NOT EXISTS idx_workflow_tasks_claimed_at
                ON workflow_tasks(claimed_at);",
        )?;
        Ok(())
    }

    async fn with_conn<F, R>(&self, f: F) -> Result<R, EngineError>
    where
        F: FnOnce(&mut Connection) -> Result<R, EngineError> + Send + 'static,
        R: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn
                .lock()
                .map_err(|e| EngineError::Storage(format!("mutex poisoned: {e}")))?;
            f(&mut guard)
        })
        .await
        .map_err(|e| EngineError::Join(e.to_string()))?
    }
}

#[async_trait]
impl WorkflowEngineBackend for SqliteWorkflowBackend {
    async fn submit_task(
        &self,
        task: WorkflowTask,
    ) -> Result<WorkflowTaskId, EngineError> {
        let id = task.id;
        let payload = serde_json::to_vec(&task)
            .map_err(|e| EngineError::Storage(format!("serialize task: {e}")))?;
        let status = status_to_str(task.status).to_string();
        let priority = task.priority as i64;
        let defer_until = task.defer_until.map(|t| t.timestamp());

        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO workflow_tasks (id, status, priority, payload, defer_until)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                    status = excluded.status,
                    priority = excluded.priority,
                    payload = excluded.payload,
                    defer_until = excluded.defer_until",
                params![&id.hash[..], status, priority, payload, defer_until],
            )?;
            Ok(())
        })
        .await?;

        Ok(id)
    }

    async fn claim_next_ready(
        &self,
        agent_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<ClaimToken>, EngineError> {
        let now_ts = now.timestamp();
        let agent_id_owned = agent_id.to_string();

        let claimed: Option<(Vec<u8>, Vec<u8>)> = self
            .with_conn(move |conn| {
                // Atomic CAS: pick the highest-priority Open task whose defer
                // window has elapsed, flip it to Hooked, and RETURN its id +
                // payload. sqlite's UPDATE ... RETURNING is atomic within a
                // single statement, and the subquery is evaluated before the
                // UPDATE acquires its write lock, so two concurrent processes
                // on the same db will serialise here and only one wins.
                let mut stmt = conn.prepare(
                    "UPDATE workflow_tasks
                       SET status = 'hooked',
                           claimed_at = ?1,
                           claimed_by = ?2
                     WHERE id = (
                         SELECT id FROM workflow_tasks
                          WHERE status = 'open'
                            AND (defer_until IS NULL OR defer_until <= ?1)
                          ORDER BY priority ASC, id ASC
                          LIMIT 1
                     )
                     RETURNING id, payload",
                )?;
                let row: Option<(Vec<u8>, Vec<u8>)> = stmt
                    .query_row(params![now_ts, agent_id_owned], |r| {
                        Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
                    })
                    .optional()?;
                Ok(row)
            })
            .await?;

        let Some((id_bytes, payload)) = claimed else {
            return Ok(None);
        };

        let id_arr: [u8; 32] = id_bytes
            .try_into()
            .map_err(|_| EngineError::Storage("claimed id: bad length".to_string()))?;
        let task_id = WorkflowTaskId { hash: id_arr };

        // Update the persisted payload's status to match what we just wrote
        // to the row. The payload is only read by `load`, so keeping it in
        // sync preserves round-trip equality for observers.
        let mut task: WorkflowTask = serde_json::from_slice(&payload)
            .map_err(|e| EngineError::Storage(format!("deserialize task: {e}")))?;
        task.status = WorkflowTaskStatus::Hooked;
        task.assignee = Some(agent_id.to_string());
        let refreshed_payload = serde_json::to_vec(&task)
            .map_err(|e| EngineError::Storage(format!("serialize task: {e}")))?;

        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE workflow_tasks SET payload = ?1 WHERE id = ?2",
                params![refreshed_payload, &id_arr[..]],
            )?;
            Ok(())
        })
        .await?;

        Ok(Some(ClaimToken {
            task_id,
            agent_id: agent_id.to_string(),
            claimed_at: now,
            idempotency_key: Uuid::new_v4(),
        }))
    }

    async fn mark_complete(
        &self,
        id: &WorkflowTaskId,
        result: serde_json::Value,
    ) -> Result<(), EngineError> {
        let id_bytes = id.hash;
        let id_copy = *id;
        let result_bytes = serde_json::to_vec(&result)
            .map_err(|e| EngineError::Storage(format!("serialize result: {e}")))?;

        // Update the embedded payload so loads reflect the terminal status.
        let task = self.load(id).await?;
        let mut task = task;
        task.status = WorkflowTaskStatus::Closed;
        let payload = serde_json::to_vec(&task)
            .map_err(|e| EngineError::Storage(format!("serialize task: {e}")))?;

        let rows = self
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE workflow_tasks
                        SET status = 'closed',
                            result = ?1,
                            payload = ?2,
                            claimed_at = NULL,
                            claimed_by = NULL
                      WHERE id = ?3",
                    params![result_bytes, payload, &id_bytes[..]],
                )?;
                Ok(n)
            })
            .await?;

        if rows == 0 {
            return Err(EngineError::NotFound(id_copy));
        }
        Ok(())
    }

    async fn mark_failed(
        &self,
        id: &WorkflowTaskId,
        reason: &str,
    ) -> Result<(), EngineError> {
        let id_bytes = id.hash;
        let id_copy = *id;
        let reason_owned = reason.to_string();

        let task = self.load(id).await?;
        let mut task = task;
        task.status = WorkflowTaskStatus::Blocked;
        let payload = serde_json::to_vec(&task)
            .map_err(|e| EngineError::Storage(format!("serialize task: {e}")))?;

        let rows = self
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE workflow_tasks
                        SET status = 'blocked',
                            failure_reason = ?1,
                            payload = ?2,
                            claimed_at = NULL,
                            claimed_by = NULL
                      WHERE id = ?3",
                    params![reason_owned, payload, &id_bytes[..]],
                )?;
                Ok(n)
            })
            .await?;

        if rows == 0 {
            return Err(EngineError::NotFound(id_copy));
        }
        Ok(())
    }

    async fn recover_orphans(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<WorkflowTaskId>, EngineError> {
        let cutoff_ts = cutoff.timestamp();

        let rows: Vec<(Vec<u8>, Vec<u8>)> = self
            .with_conn(move |conn| {
                // Snapshot the orphans under the same write lock used to reset
                // them, so a concurrent claim can't see a row between the
                // SELECT and the UPDATE.
                let tx = conn.transaction()?;
                let rows: Vec<(Vec<u8>, Vec<u8>)> = {
                    let mut stmt = tx.prepare(
                        "SELECT id, payload FROM workflow_tasks
                          WHERE status = 'hooked'
                            AND claimed_at IS NOT NULL
                            AND claimed_at <= ?1",
                    )?;
                    let mapped = stmt.query_map(params![cutoff_ts], |r| {
                        Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
                    })?;
                    mapped.collect::<Result<Vec<_>, _>>()?
                };
                tx.execute(
                    "UPDATE workflow_tasks
                        SET status = 'open',
                            claimed_at = NULL,
                            claimed_by = NULL
                      WHERE status = 'hooked'
                        AND claimed_at IS NOT NULL
                        AND claimed_at <= ?1",
                    params![cutoff_ts],
                )?;
                tx.commit()?;
                Ok(rows)
            })
            .await?;

        let mut ids = Vec::with_capacity(rows.len());
        for (id_bytes, payload) in rows {
            let id_arr: [u8; 32] = id_bytes
                .try_into()
                .map_err(|_| EngineError::Storage("orphan id: bad length".to_string()))?;
            let id = WorkflowTaskId { hash: id_arr };

            // Refresh embedded payload so `load` reflects the reset state.
            let mut task: WorkflowTask = serde_json::from_slice(&payload)
                .map_err(|e| EngineError::Storage(format!("deserialize task: {e}")))?;
            task.status = WorkflowTaskStatus::Open;
            task.assignee = None;
            let refreshed = serde_json::to_vec(&task)
                .map_err(|e| EngineError::Storage(format!("serialize task: {e}")))?;
            self.with_conn(move |conn| {
                conn.execute(
                    "UPDATE workflow_tasks SET payload = ?1 WHERE id = ?2",
                    params![refreshed, &id_arr[..]],
                )?;
                Ok(())
            })
            .await?;

            ids.push(id);
        }
        Ok(ids)
    }

    async fn load(&self, id: &WorkflowTaskId) -> Result<WorkflowTask, EngineError> {
        let id_bytes = id.hash;
        let id_copy = *id;
        let row: Option<Vec<u8>> = self
            .with_conn(move |conn| {
                let r = conn
                    .query_row(
                        "SELECT payload FROM workflow_tasks WHERE id = ?1",
                        params![&id_bytes[..]],
                        |r| r.get::<_, Vec<u8>>(0),
                    )
                    .optional()?;
                Ok(r)
            })
            .await?;

        let Some(payload) = row else {
            return Err(EngineError::NotFound(id_copy));
        };
        let task: WorkflowTask = serde_json::from_slice(&payload)
            .map_err(|e| EngineError::Storage(format!("deserialize task: {e}")))?;
        Ok(task)
    }
}

// ---------------------------------------------------------------------------
// MemoryWorkflowBackend
// ---------------------------------------------------------------------------

/// Non-durable `Vec`-backed implementation of [`WorkflowEngineBackend`].
///
/// Reuses the pure-function helpers in [`ready`](crate::ready) /
/// [`claim`](crate::claim) so unit tests and single-process fixtures behave
/// identically to the in-memory surface that already existed before the
/// engine was introduced.
#[derive(Default)]
pub struct MemoryWorkflowBackend {
    tasks: Mutex<Vec<WorkflowTask>>,
}

impl MemoryWorkflowBackend {
    /// Create an empty memory backend.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl WorkflowEngineBackend for MemoryWorkflowBackend {
    async fn submit_task(
        &self,
        task: WorkflowTask,
    ) -> Result<WorkflowTaskId, EngineError> {
        let id = task.id;
        let mut guard = self
            .tasks
            .lock()
            .map_err(|e| EngineError::Storage(format!("mutex poisoned: {e}")))?;
        if let Some(existing) = guard.iter_mut().find(|t| t.id == id) {
            *existing = task;
        } else {
            guard.push(task);
        }
        Ok(id)
    }

    async fn claim_next_ready(
        &self,
        agent_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<ClaimToken>, EngineError> {
        let mut guard = self
            .tasks
            .lock()
            .map_err(|e| EngineError::Storage(format!("mutex poisoned: {e}")))?;
        // Pick the highest-priority Open task whose defer gate has elapsed.
        // We deliberately ignore dependency / external-gate evaluation here —
        // that's ready_tasks_with_context territory; MemoryWorkflowBackend is
        // a thin durability mirror for tests, not a replacement for the
        // full scheduler.
        let snapshot: Vec<WorkflowTask> = guard.iter().cloned().collect();
        let candidate_id = ready_tasks(&snapshot, now)
            .into_iter()
            .map(|t| t.id)
            .next();

        let Some(id) = candidate_id else {
            return Ok(None);
        };
        let token = claim_task_in_memory(&mut guard, &id, agent_id, now)?;
        Ok(Some(token))
    }

    async fn mark_complete(
        &self,
        id: &WorkflowTaskId,
        _result: serde_json::Value,
    ) -> Result<(), EngineError> {
        let mut guard = self
            .tasks
            .lock()
            .map_err(|e| EngineError::Storage(format!("mutex poisoned: {e}")))?;
        let task = guard
            .iter_mut()
            .find(|t| t.id == *id)
            .ok_or(EngineError::NotFound(*id))?;
        task.status = WorkflowTaskStatus::Closed;
        Ok(())
    }

    async fn mark_failed(
        &self,
        id: &WorkflowTaskId,
        _reason: &str,
    ) -> Result<(), EngineError> {
        let mut guard = self
            .tasks
            .lock()
            .map_err(|e| EngineError::Storage(format!("mutex poisoned: {e}")))?;
        let task = guard
            .iter_mut()
            .find(|t| t.id == *id)
            .ok_or(EngineError::NotFound(*id))?;
        task.status = WorkflowTaskStatus::Blocked;
        Ok(())
    }

    async fn recover_orphans(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<WorkflowTaskId>, EngineError> {
        let mut guard = self
            .tasks
            .lock()
            .map_err(|e| EngineError::Storage(format!("mutex poisoned: {e}")))?;
        let mut out = Vec::new();
        for task in guard.iter_mut() {
            if task.status != WorkflowTaskStatus::Hooked {
                continue;
            }
            let hooked_at = task
                .metadata
                .get("hooked_at")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                .unwrap_or(task.created_at);
            if hooked_at <= cutoff {
                task.status = WorkflowTaskStatus::Open;
                task.assignee = None;
                out.push(task.id);
            }
        }
        Ok(out)
    }

    async fn load(&self, id: &WorkflowTaskId) -> Result<WorkflowTask, EngineError> {
        let guard = self
            .tasks
            .lock()
            .map_err(|e| EngineError::Storage(format!("mutex poisoned: {e}")))?;
        guard
            .iter()
            .find(|t| t.id == *id)
            .cloned()
            .ok_or(EngineError::NotFound(*id))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{WorkflowTaskInput, WorkflowTaskType};

    fn make_task(title: &str, priority: u8) -> WorkflowTask {
        WorkflowTask::new(WorkflowTaskInput {
            title: title.to_string(),
            description: "d".to_string(),
            acceptance_criteria: vec!["ac".to_string()],
            status: WorkflowTaskStatus::Open,
            priority,
            task_type: WorkflowTaskType::Chore,
            source_formula: None,
            source_location: None,
            created_at: Utc::now(),
        })
    }

    #[tokio::test]
    async fn memory_backend_submit_and_claim() {
        let backend = Arc::new(MemoryWorkflowBackend::new());
        let engine = WorkflowEngine::new(backend);
        let id = engine.submit_task(make_task("one", 1)).await.unwrap();

        let token = engine
            .claim_next_ready("worker", Utc::now())
            .await
            .unwrap()
            .expect("ready");
        assert_eq!(token.task_id, id);
        assert_eq!(token.agent_id, "worker");
    }

    #[tokio::test]
    async fn memory_backend_claim_none_when_empty() {
        let backend = Arc::new(MemoryWorkflowBackend::new());
        let engine = WorkflowEngine::new(backend);
        let token = engine.claim_next_ready("worker", Utc::now()).await.unwrap();
        assert!(token.is_none());
    }

    #[tokio::test]
    async fn sqlite_backend_round_trip_in_memory() {
        let backend = Arc::new(SqliteWorkflowBackend::open_in_memory().unwrap());
        let engine = WorkflowEngine::new(backend);
        let id = engine.submit_task(make_task("a", 0)).await.unwrap();

        let loaded = engine.load(&id).await.unwrap();
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.status, WorkflowTaskStatus::Open);

        let token = engine
            .claim_next_ready("worker-1", Utc::now())
            .await
            .unwrap()
            .expect("ready");
        assert_eq!(token.task_id, id);

        engine
            .mark_complete(&id, serde_json::json!({ "ok": true }))
            .await
            .unwrap();
        let reloaded = engine.load(&id).await.unwrap();
        assert_eq!(reloaded.status, WorkflowTaskStatus::Closed);
    }
}

