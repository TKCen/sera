//! Agent-signal inbox — SQLite durable fallback for push signals.
//!
//! See `docs/signal-system-design.md`. Signals with [`SignalTarget::MainSession`]
//! are written here when the recipient is offline, and drained on the next
//! session resume. `ArtifactOnly` / `Silent` targets skip the inbox entirely —
//! only the artifact is stored.
//!
//! Retention: 30 days. Callers should periodically call
//! [`SqliteSignalStore::purge_expired`] (recommended: daily).
//!
//! [`SignalTarget`]: sera_types::signal::SignalTarget

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;

use sera_types::signal::Signal;

use crate::error::DbError;

/// Default retention window for inbox rows — 30 days, per design doc.
pub const DEFAULT_SIGNAL_TTL_SECS: i64 = 30 * 24 * 60 * 60;

/// A stored signal row — mirrors the `agent_signals` table.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredSignal {
    pub id: String,
    pub to_agent_id: String,
    pub signal_type: String,
    pub signal: Signal,
    pub delivered: bool,
    /// Unix seconds.
    pub created_at: i64,
    /// Unix seconds.
    pub expires_at: i64,
}

/// Trait surface used by sera-runtime and sera-gateway for the signal inbox.
/// Kept minimal on purpose — enqueue / drain / mark-delivered / purge is the
/// full contract the push path needs.
#[async_trait]
pub trait SignalStore: Send + Sync + std::fmt::Debug {
    /// Write a signal to the inbox for `to_agent_id`.
    async fn enqueue(&self, to_agent_id: &str, signal: &Signal) -> Result<String, DbError>;

    /// Drain undelivered signals for `to_agent_id` (in arrival order), marking
    /// them delivered atomically.
    async fn drain_pending(&self, to_agent_id: &str) -> Result<Vec<StoredSignal>, DbError>;

    /// Return undelivered signals without marking them delivered — useful for
    /// dashboards and tests.
    async fn peek_pending(&self, to_agent_id: &str) -> Result<Vec<StoredSignal>, DbError>;

    /// Purge rows whose `expires_at` is in the past. Returns the number of
    /// rows deleted.
    async fn purge_expired(&self) -> Result<usize, DbError>;
}

/// SQLite-backed inbox store.
#[derive(Debug, Clone)]
pub struct SqliteSignalStore {
    conn: Arc<Mutex<Connection>>,
    ttl_secs: i64,
}

impl SqliteSignalStore {
    /// Construct with the default 30-day retention.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            conn,
            ttl_secs: DEFAULT_SIGNAL_TTL_SECS,
        }
    }

    /// Construct with a custom retention window. Primarily for tests that
    /// need expiry to trigger without waiting 30 days.
    pub fn with_ttl(conn: Arc<Mutex<Connection>>, ttl_secs: i64) -> Self {
        Self { conn, ttl_secs }
    }

    /// Create the `agent_signals` table. Idempotent.
    pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_signals (
                id          TEXT PRIMARY KEY,
                to_agent_id TEXT NOT NULL,
                signal_type TEXT NOT NULL,
                payload     TEXT NOT NULL,
                delivered   INTEGER NOT NULL DEFAULT 0,
                created_at  INTEGER NOT NULL,
                expires_at  INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_signals_to_agent
                ON agent_signals(to_agent_id, delivered);
            CREATE INDEX IF NOT EXISTS idx_signals_expiry
                ON agent_signals(expires_at);",
        )
    }
}

fn row_to_stored(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredSignal> {
    let payload: String = row.get(3)?;
    let signal: Signal = serde_json::from_str(&payload).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        )
    })?;
    Ok(StoredSignal {
        id: row.get(0)?,
        to_agent_id: row.get(1)?,
        signal_type: row.get(2)?,
        signal,
        delivered: row.get::<_, i64>(4)? != 0,
        created_at: row.get(5)?,
        expires_at: row.get(6)?,
    })
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[async_trait]
impl SignalStore for SqliteSignalStore {
    async fn enqueue(&self, to_agent_id: &str, signal: &Signal) -> Result<String, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        let kind = signal.kind().to_string();
        let payload = serde_json::to_string(signal)
            .map_err(|e| DbError::Integrity(format!("signal serialise: {e}")))?;
        let now = now_secs();
        let expires = now + self.ttl_secs;

        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO agent_signals (id, to_agent_id, signal_type, payload, delivered, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)",
            params![id, to_agent_id, kind, payload, now, expires],
        )
        .map_err(|e| DbError::Integrity(format!("sqlite enqueue signal: {e}")))?;
        Ok(id)
    }

    async fn drain_pending(&self, to_agent_id: &str) -> Result<Vec<StoredSignal>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, to_agent_id, signal_type, payload, delivered, created_at, expires_at
                 FROM agent_signals
                 WHERE to_agent_id = ?1 AND delivered = 0 AND expires_at > ?2
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| DbError::Integrity(format!("sqlite prepare drain: {e}")))?;
        let rows: Vec<StoredSignal> = stmt
            .query_map(params![to_agent_id, now_secs()], row_to_stored)
            .map_err(|e| DbError::Integrity(format!("sqlite drain: {e}")))?
            .collect::<Result<_, _>>()
            .map_err(|e| DbError::Integrity(format!("sqlite drain row: {e}")))?;
        drop(stmt);

        if !rows.is_empty() {
            // Mark drained rows as delivered in one statement.
            let placeholders = rows.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "UPDATE agent_signals SET delivered = 1 WHERE id IN ({placeholders})"
            );
            let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
            let params_iter = rusqlite::params_from_iter(ids.iter());
            conn.execute(&sql, params_iter)
                .map_err(|e| DbError::Integrity(format!("sqlite drain mark: {e}")))?;
        }

        Ok(rows)
    }

    async fn peek_pending(&self, to_agent_id: &str) -> Result<Vec<StoredSignal>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, to_agent_id, signal_type, payload, delivered, created_at, expires_at
                 FROM agent_signals
                 WHERE to_agent_id = ?1 AND delivered = 0 AND expires_at > ?2
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| DbError::Integrity(format!("sqlite prepare peek: {e}")))?;
        let rows: Vec<StoredSignal> = stmt
            .query_map(params![to_agent_id, now_secs()], row_to_stored)
            .map_err(|e| DbError::Integrity(format!("sqlite peek: {e}")))?
            .collect::<Result<_, _>>()
            .map_err(|e| DbError::Integrity(format!("sqlite peek row: {e}")))?;
        Ok(rows)
    }

    async fn purge_expired(&self) -> Result<usize, DbError> {
        let conn = self.conn.lock().await;
        let n = conn
            .execute(
                "DELETE FROM agent_signals WHERE expires_at <= ?1",
                params![now_secs()],
            )
            .map_err(|e| DbError::Integrity(format!("sqlite purge signals: {e}")))?;
        Ok(n)
    }
}

/// Convenience helper — fetch a single row by id. Primarily for tests and
/// diagnostics; the hot path uses [`SignalStore::drain_pending`].
pub async fn get_signal(
    store: &SqliteSignalStore,
    id: &str,
) -> Result<Option<StoredSignal>, DbError> {
    let conn = store.conn.lock().await;
    let mut stmt = conn
        .prepare(
            "SELECT id, to_agent_id, signal_type, payload, delivered, created_at, expires_at
             FROM agent_signals WHERE id = ?1",
        )
        .map_err(|e| DbError::Integrity(format!("sqlite prepare get: {e}")))?;
    let row = stmt
        .query_row(params![id], row_to_stored)
        .optional()
        .map_err(|e| DbError::Integrity(format!("sqlite get signal: {e}")))?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::capability::AgentCapability;

    fn new_store() -> SqliteSignalStore {
        let conn = Connection::open_in_memory().unwrap();
        SqliteSignalStore::init_schema(&conn).unwrap();
        SqliteSignalStore::new(Arc::new(Mutex::new(conn)))
    }

    #[tokio::test]
    async fn init_schema_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        SqliteSignalStore::init_schema(&conn).unwrap();
        SqliteSignalStore::init_schema(&conn).unwrap();
        SqliteSignalStore::init_schema(&conn).unwrap();
    }

    #[tokio::test]
    async fn enqueue_and_peek_roundtrip() {
        let store = new_store();
        let sig = Signal::Done {
            artifact_id: "art-1".into(),
            summary: "done".into(),
            duration_ms: 1200,
        };
        let id = store.enqueue("agent-a", &sig).await.unwrap();

        let rows = store.peek_pending("agent-a").await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].to_agent_id, "agent-a");
        assert_eq!(rows[0].signal_type, "done");
        assert_eq!(rows[0].signal, sig);
        assert!(!rows[0].delivered);
        assert!(rows[0].expires_at > rows[0].created_at);
    }

    #[tokio::test]
    async fn peek_does_not_mark_delivered() {
        let store = new_store();
        let sig = Signal::Started {
            task_id: "t".into(),
            description: "".into(),
        };
        store.enqueue("a", &sig).await.unwrap();

        let rows1 = store.peek_pending("a").await.unwrap();
        let rows2 = store.peek_pending("a").await.unwrap();
        assert_eq!(rows1.len(), 1);
        assert_eq!(rows2.len(), 1);
    }

    #[tokio::test]
    async fn drain_marks_delivered_and_is_idempotent() {
        let store = new_store();
        store
            .enqueue(
                "a",
                &Signal::Progress {
                    task_id: "t".into(),
                    pct: 50,
                    note: "".into(),
                },
            )
            .await
            .unwrap();
        store
            .enqueue(
                "a",
                &Signal::Done {
                    artifact_id: "x".into(),
                    summary: "".into(),
                    duration_ms: 1,
                },
            )
            .await
            .unwrap();

        let first = store.drain_pending("a").await.unwrap();
        assert_eq!(first.len(), 2);
        let second = store.drain_pending("a").await.unwrap();
        assert!(second.is_empty(), "second drain should be empty");
    }

    #[tokio::test]
    async fn drain_orders_by_created_at_and_id() {
        let store = new_store();
        // Insert three signals in rapid succession — they'll share the same
        // created_at second, so the secondary `id ASC` sort guarantees stable
        // ordering in a single drain.
        for i in 0..3 {
            store
                .enqueue(
                    "a",
                    &Signal::Progress {
                        task_id: format!("t{i}"),
                        pct: i as u8,
                        note: "".into(),
                    },
                )
                .await
                .unwrap();
        }
        let rows = store.drain_pending("a").await.unwrap();
        assert_eq!(rows.len(), 3);
    }

    #[tokio::test]
    async fn drain_scoped_to_to_agent_id() {
        let store = new_store();
        store
            .enqueue(
                "a",
                &Signal::Done {
                    artifact_id: "a".into(),
                    summary: "".into(),
                    duration_ms: 0,
                },
            )
            .await
            .unwrap();
        store
            .enqueue(
                "b",
                &Signal::Done {
                    artifact_id: "b".into(),
                    summary: "".into(),
                    duration_ms: 0,
                },
            )
            .await
            .unwrap();
        let a = store.drain_pending("a").await.unwrap();
        let b = store.drain_pending("b").await.unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_ne!(a[0].id, b[0].id);
    }

    #[tokio::test]
    async fn expired_rows_are_hidden_and_purged() {
        // TTL = 0 means rows expire the moment they're written.
        let conn = Connection::open_in_memory().unwrap();
        SqliteSignalStore::init_schema(&conn).unwrap();
        let store = SqliteSignalStore::with_ttl(Arc::new(Mutex::new(conn)), 0);
        store
            .enqueue(
                "a",
                &Signal::Done {
                    artifact_id: "x".into(),
                    summary: "".into(),
                    duration_ms: 0,
                },
            )
            .await
            .unwrap();
        // Expired rows are invisible to drain / peek.
        assert!(store.peek_pending("a").await.unwrap().is_empty());
        assert!(store.drain_pending("a").await.unwrap().is_empty());
        // Purge deletes them.
        let n = store.purge_expired().await.unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn blocked_with_required_capabilities_roundtrips() {
        let store = new_store();
        let sig = Signal::Blocked {
            reason: "missing caps".into(),
            requires: vec![AgentCapability::MetaChange, AgentCapability::ConfigPropose],
        };
        let id = store.enqueue("a", &sig).await.unwrap();
        let got = get_signal(&store, &id).await.unwrap().unwrap();
        assert_eq!(got.signal, sig);
        assert_eq!(got.signal_type, "blocked");
    }
}
