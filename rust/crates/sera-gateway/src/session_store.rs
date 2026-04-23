//! Two-layer session persistence — `SessionStore` (sera-r9ed, Phase 1).
//!
//! Per SPEC-gateway §6.1b, the gateway keeps current session state in a SQLite
//! **PartTable** and pushes an audit-grade replay log into a **shadow git**
//! repo. PartTable rows reference a commit SHA on the shadow repo; replaying
//! from that SHA reproduces the session byte-for-byte.
//!
//! This module exposes:
//!
//! * [`SessionStore`] — the trait the gateway session lifecycle calls into.
//! * [`SqliteGitSessionStore`] — the default implementation backing PartTable
//!   by SQLite (via `rusqlite`, following the [`sera_db::signals`] pattern)
//!   and the shadow repo by bare `git2` repositories on disk.
//!
//! Each submission (with its emitted events) becomes exactly **one** commit on
//! `refs/heads/main` in a per-session bare repo at
//! `<data_root>/sessions/<session_id>/git`. The commit's tree contains:
//!
//! * `submission.json` — serialized [`sera_types::envelope::Submission`]
//! * `emissions/<0000>.json` … `emissions/<NNNN>.json` — serialized
//!   [`sera_types::envelope::Event`]s in emission order
//!
//! The head SHA of that commit is the durable [`SubmissionRef`]; PartTable
//! stores it as the session head plus one row per part for fast indexed
//! access.
//!
//! # Minimal API
//!
//! The trait intentionally exposes only what the sera-r1g8 envelope wrapping
//! needs. More session methods will arrive in a follow-up when the route
//! layer asks for them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use sera_types::envelope::{Event, Submission};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Reference to an appended submission — a session-scoped (commit, index)
/// pair. Equality is structural; the commit SHA alone uniquely identifies the
/// entry within a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmissionRef {
    pub session_id: String,
    /// Full (40-hex) commit SHA on `refs/heads/main`.
    pub commit: String,
    /// Zero-based position in the session's submission chain.
    pub index: u64,
}

/// Bundle persisted atomically for one gateway turn — the inbound
/// [`Submission`] and every outbound [`Event`] it produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSubmission {
    pub submission: Submission,
    #[serde(default)]
    pub emissions: Vec<Event>,
}

/// Errors surfaced by [`SessionStore`] implementations.
#[derive(Debug, thiserror::Error)]
pub enum SessionStoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("session head moved under us for session {session_id}")]
    HeadConflict { session_id: String },
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Two-layer session persistence contract.
///
/// Implementations MUST make [`append_submission`](Self::append_submission)
/// atomic with respect to concurrent callers targetting the same
/// `session_id`: PartTable rows and the shadow-repo HEAD must both point at
/// the new commit before any subsequent call observes either, and two racing
/// callers must serialize so neither loses a write.
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Append a submission + its emissions to the session. Returns the new
    /// head reference.
    async fn append_submission(
        &self,
        session_id: &str,
        bundle: &StoredSubmission,
    ) -> Result<SubmissionRef, SessionStoreError>;

    /// Return the current head ref for `session_id`, or `None` if the
    /// session has no submissions yet.
    async fn head(&self, session_id: &str) -> Result<Option<SubmissionRef>, SessionStoreError>;

    /// Replay every submission for `session_id` in the order it was appended.
    async fn replay(&self, session_id: &str) -> Result<Vec<Submission>, SessionStoreError>;

    /// Append an envelope with no emissions yet. Used by agent-facing routes
    /// (sera-r1g8) that emit the inbound envelope before dispatching to the
    /// underlying service.
    async fn append_envelope(
        &self,
        session_id: &str,
        envelope: &Submission,
    ) -> Result<SubmissionRef, SessionStoreError> {
        let bundle = StoredSubmission {
            submission: envelope.clone(),
            emissions: Vec::new(),
        };
        self.append_submission(session_id, &bundle).await
    }
}

// ---------------------------------------------------------------------------
// SqliteGitSessionStore
// ---------------------------------------------------------------------------

/// PartTable (SQLite) + shadow-git backed [`SessionStore`].
///
/// Construct via [`SqliteGitSessionStore::open`] for normal use or
/// [`SqliteGitSessionStore::open_with_conn`] when the SQLite connection is
/// shared with other gateway tables.
#[derive(Clone)]
pub struct SqliteGitSessionStore {
    conn: Arc<Mutex<Connection>>,
    /// Root containing per-session bare repos at `<root>/<session_id>/git`.
    sessions_root: PathBuf,
}

impl SqliteGitSessionStore {
    /// Create the PartTable schema. Idempotent.
    pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS session_heads (
                session_id TEXT PRIMARY KEY,
                head_commit TEXT NOT NULL,
                head_index INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
             );

             CREATE TABLE IF NOT EXISTS session_parts (
                session_id TEXT NOT NULL,
                part_index INTEGER NOT NULL,
                commit_sha TEXT NOT NULL,
                submission_id TEXT NOT NULL,
                emission_count INTEGER NOT NULL,
                op_kind TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (session_id, part_index)
             );
             CREATE INDEX IF NOT EXISTS idx_session_parts_session
                ON session_parts(session_id, part_index);
             CREATE INDEX IF NOT EXISTS idx_session_parts_commit
                ON session_parts(commit_sha);",
        )
    }

    /// Open a store rooted at `sessions_root`, creating the root directory and
    /// SQLite schema if missing. `db_path` is a single-file SQLite database.
    pub fn open(
        db_path: impl AsRef<Path>,
        sessions_root: impl Into<PathBuf>,
    ) -> Result<Self, SessionStoreError> {
        let conn = Connection::open(db_path.as_ref())?;
        Self::init_schema(&conn)?;
        let sessions_root = sessions_root.into();
        std::fs::create_dir_all(&sessions_root)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            sessions_root,
        })
    }

    /// Open against an existing shared connection. The caller is responsible
    /// for calling [`init_schema`](Self::init_schema) before first use.
    pub fn open_with_conn(
        conn: Arc<Mutex<Connection>>,
        sessions_root: impl Into<PathBuf>,
    ) -> Result<Self, SessionStoreError> {
        let sessions_root = sessions_root.into();
        std::fs::create_dir_all(&sessions_root)?;
        Ok(Self {
            conn,
            sessions_root,
        })
    }

    fn repo_path(&self, session_id: &str) -> PathBuf {
        self.sessions_root.join(session_id).join("git")
    }

    /// Shared deterministic signature — replay stays byte-for-byte identical
    /// across runs regardless of the operator's git config.
    fn signature<'a>() -> Result<git2::Signature<'a>, git2::Error> {
        // Fixed epoch (2026-01-01T00:00:00Z) so replay is reproducible; the
        // submission's own timestamps live inside the JSON payload.
        git2::Signature::new(
            "sera-gateway",
            "sera-gateway@sera.local",
            &git2::Time::new(1_767_225_600, 0),
        )
    }
}

// Internal helpers that need Repository borrows but are pure enough to keep
// outside the `impl SessionStore` block.

fn write_bundle_tree(
    repo: &git2::Repository,
    bundle: &StoredSubmission,
) -> Result<git2::Oid, SessionStoreError> {
    // Top-level entries: submission.json + emissions/ tree.
    let submission_blob = repo.blob(&serde_json::to_vec_pretty(&bundle.submission)?)?;

    let mut emissions_tb = repo.treebuilder(None)?;
    for (i, event) in bundle.emissions.iter().enumerate() {
        let name = format!("{i:04}.json");
        let blob = repo.blob(&serde_json::to_vec_pretty(event)?)?;
        emissions_tb.insert(&name, blob, git2::FileMode::Blob.into())?;
    }
    let emissions_tree = emissions_tb.write()?;

    let mut root_tb = repo.treebuilder(None)?;
    root_tb.insert(
        "submission.json",
        submission_blob,
        git2::FileMode::Blob.into(),
    )?;
    root_tb.insert("emissions", emissions_tree, git2::FileMode::Tree.into())?;
    Ok(root_tb.write()?)
}

fn op_kind_label(sub: &Submission) -> &'static str {
    use sera_types::envelope::Op;
    match &sub.op {
        Op::UserTurn { .. } => "user_turn",
        Op::Steer { .. } => "steer",
        Op::Interrupt => "interrupt",
        Op::System(_) => "system",
        Op::ApprovalResponse { .. } => "approval_response",
        Op::Register(_) => "register",
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[async_trait]
impl SessionStore for SqliteGitSessionStore {
    async fn append_submission(
        &self,
        session_id: &str,
        bundle: &StoredSubmission,
    ) -> Result<SubmissionRef, SessionStoreError> {
        // Single connection-level lock serialises concurrent writers to the
        // same session (and, acceptably, to other sessions sharing this store;
        // PartTable writes are O(µs) so contention is negligible). This
        // guarantees the git HEAD + SQL head_commit flip happens atomically.
        let conn = self.conn.lock().await;

        // Pin the prior head so we can (a) parent the new commit and (b)
        // detect index collisions if something ever bypassed the mutex.
        let prior: Option<(String, i64)> = conn
            .query_row(
                "SELECT head_commit, head_index FROM session_heads WHERE session_id = ?1",
                params![session_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;

        let next_index: u64 = match &prior {
            Some((_, idx)) => (*idx as u64) + 1,
            None => 0,
        };

        // Write the new commit to the shadow repo. Git IO is blocking; spawn
        // it onto a blocking thread so the async runtime stays responsive.
        let repo_path = self.repo_path(session_id);
        let parent_commit = prior.as_ref().map(|(sha, _)| sha.clone());
        let bundle_owned = bundle.clone();
        let new_commit =
            tokio::task::spawn_blocking(move || -> Result<String, SessionStoreError> {
                // Ensure the bare repo exists.
                let repo = if repo_path.join("HEAD").exists() {
                    git2::Repository::open_bare(&repo_path)?
                } else {
                    std::fs::create_dir_all(&repo_path)?;
                    git2::Repository::init_bare(&repo_path)?
                };

                let tree_oid = write_bundle_tree(&repo, &bundle_owned)?;
                let tree = repo.find_tree(tree_oid)?;
                let sig = SqliteGitSessionStore::signature()?;

                let parents_owned: Vec<git2::Commit> = match parent_commit.as_deref() {
                    Some(sha) => {
                        let oid = git2::Oid::from_str(sha)?;
                        vec![repo.find_commit(oid)?]
                    }
                    None => Vec::new(),
                };
                let parents_ref: Vec<&git2::Commit> = parents_owned.iter().collect();

                let message = format!("submission {}\n", bundle_owned.submission.id);
                let commit_oid = repo.commit(
                    Some("refs/heads/main"),
                    &sig,
                    &sig,
                    &message,
                    &tree,
                    &parents_ref,
                )?;
                Ok(commit_oid.to_string())
            })
            .await
            .map_err(|e| SessionStoreError::Io(std::io::Error::other(e.to_string())))??;

        // Flip PartTable rows — INSERT new part then UPSERT the head. If any
        // step fails SQLite rolls back the transaction and we bubble up.
        let now = now_secs();
        let op_kind = op_kind_label(&bundle.submission).to_string();
        let submission_id = bundle.submission.id.to_string();
        let emission_count = bundle.emissions.len() as i64;

        conn.execute_batch("BEGIN IMMEDIATE")?;
        let tx_result: Result<(), SessionStoreError> = (|| {
            conn.execute(
                "INSERT INTO session_parts
                    (session_id, part_index, commit_sha, submission_id,
                     emission_count, op_kind, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    session_id,
                    next_index as i64,
                    new_commit,
                    submission_id,
                    emission_count,
                    op_kind,
                    now,
                ],
            )?;
            conn.execute(
                "INSERT INTO session_heads
                    (session_id, head_commit, head_index, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(session_id) DO UPDATE SET
                    head_commit = excluded.head_commit,
                    head_index = excluded.head_index,
                    updated_at = excluded.updated_at",
                params![session_id, new_commit, next_index as i64, now],
            )?;
            Ok(())
        })();

        match tx_result {
            Ok(()) => {
                conn.execute_batch("COMMIT")?;
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }

        Ok(SubmissionRef {
            session_id: session_id.to_string(),
            commit: new_commit,
            index: next_index,
        })
    }

    async fn head(&self, session_id: &str) -> Result<Option<SubmissionRef>, SessionStoreError> {
        let conn = self.conn.lock().await;
        let row: Option<(String, i64)> = conn
            .query_row(
                "SELECT head_commit, head_index FROM session_heads WHERE session_id = ?1",
                params![session_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        Ok(row.map(|(commit, index)| SubmissionRef {
            session_id: session_id.to_string(),
            commit,
            index: index as u64,
        }))
    }

    async fn replay(&self, session_id: &str) -> Result<Vec<Submission>, SessionStoreError> {
        // Walk the git history back from HEAD so replay reflects the shadow
        // repo directly — PartTable is only the index; the audit log lives in
        // git.
        let head = self.head(session_id).await?;
        let Some(head) = head else {
            return Ok(Vec::new());
        };
        let repo_path = self.repo_path(session_id);

        tokio::task::spawn_blocking(move || -> Result<Vec<Submission>, SessionStoreError> {
            let repo = git2::Repository::open_bare(&repo_path)?;
            let mut revwalk = repo.revwalk()?;
            revwalk.push(git2::Oid::from_str(&head.commit)?)?;
            // Default order is topological newest-first; we want oldest-first.
            revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;

            let mut out = Vec::with_capacity(head.index as usize + 1);
            for oid in revwalk {
                let oid = oid?;
                let commit = repo.find_commit(oid)?;
                let tree = commit.tree()?;
                let entry = tree
                    .get_name("submission.json")
                    .ok_or_else(|| git2::Error::from_str("submission.json missing"))?;
                let blob = repo.find_blob(entry.id())?;
                let sub: Submission = serde_json::from_slice(blob.content())?;
                out.push(sub);
            }
            Ok(out)
        })
        .await
        .map_err(|e| SessionStoreError::Io(std::io::Error::other(e.to_string())))?
    }
}

// ---------------------------------------------------------------------------
// InMemorySessionStore
// ---------------------------------------------------------------------------

/// In-memory [`SessionStore`] used by the default binary boot path and by
/// every gateway test that does not exercise shadow-git persistence. Stores
/// appended bundles per session in a tokio `Mutex<Vec>`; synthesises a stable
/// 40-hex commit string per append so [`SubmissionRef`] consumers never see
/// an ambiguous short SHA.
#[derive(Default, Clone)]
pub struct InMemorySessionStore {
    inner: Arc<Mutex<std::collections::HashMap<String, Vec<StoredSubmission>>>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return every stored bundle for `session_id` in append order.
    pub async fn all_for(&self, session_id: &str) -> Vec<StoredSubmission> {
        self.inner
            .lock()
            .await
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Number of bundles stored for `session_id`.
    pub async fn len_for(&self, session_id: &str) -> usize {
        self.inner
            .lock()
            .await
            .get(session_id)
            .map(|v| v.len())
            .unwrap_or(0)
    }
}

fn synth_commit(session_id: &str, index: u64) -> String {
    use std::hash::{BuildHasher, Hasher};
    let hasher_state = std::collections::hash_map::RandomState::new();
    let mut h = hasher_state.build_hasher();
    Hasher::write(&mut h, session_id.as_bytes());
    Hasher::write_u64(&mut h, index);
    let lo = h.finish();
    let mut h2 = hasher_state.build_hasher();
    Hasher::write_u64(&mut h2, lo);
    Hasher::write(&mut h2, session_id.as_bytes());
    let hi = h2.finish();
    // 40-hex-char synthetic SHA — not a real git object id, only for identity.
    format!("{hi:016x}{lo:016x}{index:08x}")
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn append_submission(
        &self,
        session_id: &str,
        bundle: &StoredSubmission,
    ) -> Result<SubmissionRef, SessionStoreError> {
        let mut inner = self.inner.lock().await;
        let entry = inner.entry(session_id.to_string()).or_default();
        let index = entry.len() as u64;
        entry.push(bundle.clone());
        Ok(SubmissionRef {
            session_id: session_id.to_string(),
            commit: synth_commit(session_id, index),
            index,
        })
    }

    async fn head(&self, session_id: &str) -> Result<Option<SubmissionRef>, SessionStoreError> {
        let inner = self.inner.lock().await;
        Ok(inner.get(session_id).and_then(|v| {
            if v.is_empty() {
                None
            } else {
                let index = (v.len() - 1) as u64;
                Some(SubmissionRef {
                    session_id: session_id.to_string(),
                    commit: synth_commit(session_id, index),
                    index,
                })
            }
        }))
    }

    async fn replay(&self, session_id: &str) -> Result<Vec<Submission>, SessionStoreError> {
        let inner = self.inner.lock().await;
        Ok(inner
            .get(session_id)
            .map(|v| v.iter().map(|b| b.submission.clone()).collect())
            .unwrap_or_default())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::envelope::{EventMsg, Op, W3cTraceContext};
    use tempfile::TempDir;

    fn user_turn(text: &str) -> Submission {
        Submission {
            id: uuid::Uuid::new_v4(),
            op: Op::UserTurn {
                items: vec![serde_json::json!({
                    "type": "text",
                    "text": text,
                })],
                cwd: None,
                approval_policy: None,
                sandbox_policy: None,
                model_override: None,
                effort: None,
                final_output_schema: None,
            },
            trace: W3cTraceContext::default(),
            change_artifact: None,
            session_key: None,
            parent_session_key: None,
        }
    }

    fn event(submission_id: uuid::Uuid, delta: &str) -> Event {
        Event {
            id: uuid::Uuid::new_v4(),
            submission_id,
            msg: EventMsg::StreamingDelta {
                delta: delta.to_string(),
            },
            trace: W3cTraceContext::default(),
            timestamp: chrono::Utc::now(),
            parent_session_key: None,
        }
    }

    fn fresh_store() -> (SqliteGitSessionStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("parts.sqlite");
        let sessions = dir.path().join("sessions");
        let store = SqliteGitSessionStore::open(&db, &sessions).unwrap();
        (store, dir)
    }

    #[tokio::test]
    async fn init_schema_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        SqliteGitSessionStore::init_schema(&conn).unwrap();
        SqliteGitSessionStore::init_schema(&conn).unwrap();
        SqliteGitSessionStore::init_schema(&conn).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn head_returns_none_for_unknown_session() {
        let (store, _d) = fresh_store();
        assert!(store.head("nope").await.unwrap().is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn append_then_head_returns_latest() {
        let (store, _d) = fresh_store();
        let bundle = StoredSubmission {
            submission: user_turn("hello"),
            emissions: vec![],
        };
        let refa = store.append_submission("s1", &bundle).await.unwrap();
        assert_eq!(refa.session_id, "s1");
        assert_eq!(refa.index, 0);
        assert_eq!(refa.commit.len(), 40);

        let head = store.head("s1").await.unwrap().unwrap();
        assert_eq!(head, refa);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn append_increments_index_and_parents_commit() {
        let (store, _d) = fresh_store();
        let b1 = StoredSubmission {
            submission: user_turn("one"),
            emissions: vec![],
        };
        let b2 = StoredSubmission {
            submission: user_turn("two"),
            emissions: vec![],
        };

        let r1 = store.append_submission("s1", &b1).await.unwrap();
        let r2 = store.append_submission("s1", &b2).await.unwrap();
        assert_eq!(r1.index, 0);
        assert_eq!(r2.index, 1);
        assert_ne!(r1.commit, r2.commit);

        // r2's commit should list r1's commit as its parent in the shadow repo.
        let repo = git2::Repository::open_bare(store.repo_path("s1")).unwrap();
        let c2 = repo
            .find_commit(git2::Oid::from_str(&r2.commit).unwrap())
            .unwrap();
        assert_eq!(c2.parent_count(), 1);
        assert_eq!(c2.parent(0).unwrap().id().to_string(), r1.commit);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn replay_returns_submissions_in_order() {
        let (store, _d) = fresh_store();
        let texts = ["one", "two", "three", "four"];
        let mut appended_ids = Vec::new();
        for t in &texts {
            let sub = user_turn(t);
            appended_ids.push(sub.id);
            let bundle = StoredSubmission {
                submission: sub,
                emissions: vec![],
            };
            store.append_submission("s1", &bundle).await.unwrap();
        }
        let replayed = store.replay("s1").await.unwrap();
        assert_eq!(replayed.len(), 4);
        let replayed_ids: Vec<_> = replayed.iter().map(|s| s.id).collect();
        assert_eq!(replayed_ids, appended_ids);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn replay_is_byte_for_byte_deterministic() {
        let (store, _d) = fresh_store();
        let sub = user_turn("deterministic");
        let sub_id = sub.id;
        let ev = event(sub_id, "hi");
        let bundle = StoredSubmission {
            submission: sub,
            emissions: vec![ev],
        };
        let r1 = store.append_submission("s1", &bundle).await.unwrap();

        // Re-open the store (fresh conn, same on-disk state) — the head commit
        // must be byte-for-byte identical, i.e. the same SHA.
        let db_path = store.sessions_root.parent().unwrap().join("parts.sqlite");
        drop(store);
        let reopened =
            SqliteGitSessionStore::open(&db_path, db_path.with_file_name("sessions")).unwrap();
        let head2 = reopened.head("s1").await.unwrap().unwrap();
        assert_eq!(
            head2.commit, r1.commit,
            "head sha must be stable across reopens"
        );
        let replayed = reopened.replay("s1").await.unwrap();
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].id, sub_id);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_appends_to_same_session_are_serialised() {
        let (store, _d) = fresh_store();
        let store = Arc::new(store);
        let n = 16;
        let mut handles = Vec::new();
        for i in 0..n {
            let s = Arc::clone(&store);
            handles.push(tokio::spawn(async move {
                let bundle = StoredSubmission {
                    submission: user_turn(&format!("msg-{i}")),
                    emissions: vec![],
                };
                s.append_submission("s1", &bundle).await.unwrap()
            }));
        }
        let mut refs = Vec::new();
        for h in handles {
            refs.push(h.await.unwrap());
        }

        // Exactly n distinct indices 0..n appeared, the head matches the
        // highest-index ref, and no two commits collide.
        let mut indices: Vec<u64> = refs.iter().map(|r| r.index).collect();
        indices.sort();
        assert_eq!(indices, (0..n as u64).collect::<Vec<_>>());

        let mut commits: Vec<&str> = refs.iter().map(|r| r.commit.as_str()).collect();
        commits.sort();
        commits.dedup();
        assert_eq!(
            commits.len(),
            n,
            "each submission must have a unique commit"
        );

        let head = store.head("s1").await.unwrap().unwrap();
        assert_eq!(head.index, (n as u64) - 1);

        // Replay should yield n submissions.
        let replayed = store.replay("s1").await.unwrap();
        assert_eq!(replayed.len(), n);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn emissions_land_in_git_tree() {
        let (store, _d) = fresh_store();
        let sub = user_turn("carrying emissions");
        let sub_id = sub.id;
        let bundle = StoredSubmission {
            submission: sub,
            emissions: vec![event(sub_id, "alpha"), event(sub_id, "beta")],
        };
        let r = store.append_submission("s1", &bundle).await.unwrap();

        let repo = git2::Repository::open_bare(store.repo_path("s1")).unwrap();
        let commit = repo
            .find_commit(git2::Oid::from_str(&r.commit).unwrap())
            .unwrap();
        let tree = commit.tree().unwrap();
        let emissions_entry = tree.get_name("emissions").expect("emissions tree present");
        let emissions_tree = repo
            .find_tree(emissions_entry.id())
            .expect("emissions is a tree");
        let names: Vec<String> = emissions_tree
            .iter()
            .map(|e| e.name().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            vec!["0000.json".to_string(), "0001.json".to_string()]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn part_rows_track_session_and_index() {
        let (store, _d) = fresh_store();
        let b1 = StoredSubmission {
            submission: user_turn("a"),
            emissions: vec![],
        };
        let b2 = StoredSubmission {
            submission: user_turn("b"),
            emissions: vec![],
        };
        store.append_submission("alpha", &b1).await.unwrap();
        store.append_submission("beta", &b2).await.unwrap();

        let conn = store.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT session_id, part_index, op_kind
                 FROM session_parts ORDER BY session_id, part_index",
            )
            .unwrap();
        let rows: Vec<(String, i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("alpha".to_string(), 0, "user_turn".to_string()),
                ("beta".to_string(), 0, "user_turn".to_string()),
            ]
        );
    }
}
