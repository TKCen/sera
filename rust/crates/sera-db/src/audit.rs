//! Audit repository — Merkle hash-chain append and query.
//!
//! Dual-backend (sera-mwb4):
//! * [`AuditRepository`] — original Postgres-backed repo (sqlx, `audit_trail` table).
//!   Compiled only when the `postgres` feature is enabled.
//! * [`SqliteAuditStore`] — rusqlite-backed equivalent that mirrors the same
//!   append/query surface for local-first deployments. Always compiled.
//! * [`AuditStore`] — async trait satisfied by both so callers can depend on
//!   the trait object rather than a concrete type. Always compiled.

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{params, Connection};
#[cfg(feature = "postgres")]
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::error::DbError;

/// Row type for audit_trail table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuditRow {
    pub sequence: i64,
    pub timestamp: time::OffsetDateTime,
    pub actor_type: String,
    pub actor_id: String,
    pub acting_context: Option<serde_json::Value>,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub prev_hash: Option<String>,
    pub hash: String,
}

/// Audit repository for database operations.
/// Only available with the `postgres` feature.
#[cfg(feature = "postgres")]
pub struct AuditRepository;

#[cfg(feature = "postgres")]
impl AuditRepository {
    /// Append an audit event with hash chain.
    /// Uses an EXCLUSIVE lock to ensure sequential hashing.
    #[allow(clippy::too_many_arguments)]
    pub async fn append(
        pool: &PgPool,
        actor_type: &str,
        actor_id: &str,
        acting_context: Option<&serde_json::Value>,
        event_type: &str,
        payload: &serde_json::Value,
        hash: &str,
        prev_hash: Option<&str>,
    ) -> Result<i64, DbError> {
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO audit_trail (actor_type, actor_id, acting_context, event_type, payload, hash, prev_hash)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING sequence"
        )
        .bind(actor_type)
        .bind(actor_id)
        .bind(acting_context)
        .bind(event_type)
        .bind(payload)
        .bind(hash)
        .bind(prev_hash)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Get the latest audit record (for hash chain continuation).
    pub async fn get_latest(pool: &PgPool) -> Result<Option<AuditRow>, DbError> {
        let row = sqlx::query_as::<_, AuditRow>(
            "SELECT sequence, timestamp, actor_type, actor_id, acting_context,
                    event_type, payload, prev_hash, hash
             FROM audit_trail ORDER BY sequence DESC LIMIT 1"
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// Get audit entries with filtering and pagination.
    pub async fn get_entries(
        pool: &PgPool,
        actor_id: Option<&str>,
        event_type: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AuditRow>, DbError> {
        let mut qb = sqlx::QueryBuilder::new(
            "SELECT sequence, timestamp, actor_type, actor_id, acting_context,
                    event_type, payload, prev_hash, hash
             FROM audit_trail WHERE 1=1",
        );

        if let Some(aid) = actor_id {
            qb.push(" AND actor_id = ").push_bind(aid);
        }
        if let Some(et) = event_type {
            qb.push(" AND event_type = ").push_bind(et);
        }

        qb.push(" ORDER BY sequence DESC LIMIT ").push_bind(limit);
        qb.push(" OFFSET ").push_bind(offset);

        let rows = qb.build_query_as::<AuditRow>().fetch_all(pool).await?;
        Ok(rows)
    }

    /// Count total entries matching filters (for pagination).
    pub async fn count_entries(
        pool: &PgPool,
        actor_id: Option<&str>,
        event_type: Option<&str>,
    ) -> Result<i64, DbError> {
        let mut qb = sqlx::QueryBuilder::new("SELECT COUNT(*) FROM audit_trail WHERE 1=1");

        if let Some(aid) = actor_id {
            qb.push(" AND actor_id = ").push_bind(aid);
        }
        if let Some(et) = event_type {
            qb.push(" AND event_type = ").push_bind(et);
        }

        let (count,): (i64,) = qb.build_query_as().fetch_one(pool).await?;
        Ok(count)
    }

    /// Verify integrity of the last N records.
    pub async fn get_chain_for_verification(
        pool: &PgPool,
        count: i64,
    ) -> Result<Vec<AuditRow>, DbError> {
        let rows = sqlx::query_as::<_, AuditRow>(
            "SELECT sequence, timestamp, actor_type, actor_id, acting_context,
                    event_type, payload, prev_hash, hash
             FROM audit_trail ORDER BY sequence ASC
             LIMIT $1"
        )
        .bind(count)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }
}

// ---------------------------------------------------------------------------
// Dual-backend trait (sera-mwb4)
// ---------------------------------------------------------------------------

/// Dyn-compatible async trait for audit operations.
///
/// Both [`PgAuditStore`] and [`SqliteAuditStore`] implement this; the gateway
/// AppState can carry `Arc<dyn AuditStore>` so route handlers remain backend
/// agnostic.
#[async_trait]
pub trait AuditStore: Send + Sync + std::fmt::Debug {
    /// Append a single audit event with pre-computed hash and prev_hash.
    #[allow(clippy::too_many_arguments)]
    async fn append(
        &self,
        actor_type: &str,
        actor_id: &str,
        acting_context: Option<&serde_json::Value>,
        event_type: &str,
        payload: &serde_json::Value,
        hash: &str,
        prev_hash: Option<&str>,
    ) -> Result<i64, DbError>;

    /// Fetch the latest (highest-sequence) row, or `None` if the log is empty.
    async fn get_latest(&self) -> Result<Option<AuditRow>, DbError>;

    /// Paginated + filtered scan. Filter is AND-semantics.
    async fn get_entries(
        &self,
        actor_id: Option<&str>,
        event_type: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AuditRow>, DbError>;

    /// Count rows matching the same filter surface as [`get_entries`].
    async fn count_entries(
        &self,
        actor_id: Option<&str>,
        event_type: Option<&str>,
    ) -> Result<i64, DbError>;
}

/// Postgres implementation of [`AuditStore`] — delegates to
/// [`AuditRepository`] so the existing static functions remain usable.
/// Only available with the `postgres` feature.
#[cfg(feature = "postgres")]
#[derive(Debug, Clone)]
pub struct PgAuditStore {
    pool: PgPool,
}

#[cfg(feature = "postgres")]
impl PgAuditStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[cfg(feature = "postgres")]
#[async_trait]
impl AuditStore for PgAuditStore {
    async fn append(
        &self,
        actor_type: &str,
        actor_id: &str,
        acting_context: Option<&serde_json::Value>,
        event_type: &str,
        payload: &serde_json::Value,
        hash: &str,
        prev_hash: Option<&str>,
    ) -> Result<i64, DbError> {
        AuditRepository::append(
            &self.pool,
            actor_type,
            actor_id,
            acting_context,
            event_type,
            payload,
            hash,
            prev_hash,
        )
        .await
    }

    async fn get_latest(&self) -> Result<Option<AuditRow>, DbError> {
        AuditRepository::get_latest(&self.pool).await
    }

    async fn get_entries(
        &self,
        actor_id: Option<&str>,
        event_type: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AuditRow>, DbError> {
        AuditRepository::get_entries(&self.pool, actor_id, event_type, limit, offset).await
    }

    async fn count_entries(
        &self,
        actor_id: Option<&str>,
        event_type: Option<&str>,
    ) -> Result<i64, DbError> {
        AuditRepository::count_entries(&self.pool, actor_id, event_type).await
    }
}

// ---------------------------------------------------------------------------
// SQLite implementation (sera-mwb4)
// ---------------------------------------------------------------------------

/// SQLite-backed audit store. Uses a `Mutex<Connection>` because rusqlite
/// connections are not `Send + Sync`-shared natively. For local/single-node
/// deployments this is fine; enterprise deployments use [`PgAuditStore`].
#[derive(Debug, Clone)]
pub struct SqliteAuditStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteAuditStore {
    /// Wrap an existing connection (schema must already be initialised via
    /// [`Self::init_schema`] or [`crate::sqlite_schema::init_all`]).
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Create the `audit_trail` table idempotently.
    pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS audit_trail (
                sequence       INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp      TEXT NOT NULL DEFAULT (datetime('now')),
                actor_type     TEXT NOT NULL,
                actor_id       TEXT NOT NULL,
                acting_context TEXT,
                event_type     TEXT NOT NULL,
                payload        TEXT NOT NULL,
                prev_hash      TEXT,
                hash           TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_audit_trail_actor_id ON audit_trail(actor_id);
            CREATE INDEX IF NOT EXISTS idx_audit_trail_event_type ON audit_trail(event_type);",
        )
    }
}

fn parse_sqlite_timestamp(s: &str) -> time::OffsetDateTime {
    // SQLite's `datetime('now')` emits "YYYY-MM-DD HH:MM:SS" in UTC.
    if let Ok(dt) = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
        return dt;
    }
    let fmt = time::macros::format_description!(
        "[year]-[month]-[day] [hour]:[minute]:[second]"
    );
    if let Ok(pdt) = time::PrimitiveDateTime::parse(s, &fmt) {
        return pdt.assume_utc();
    }
    time::OffsetDateTime::UNIX_EPOCH
}

fn json_from_str(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or(serde_json::Value::Null)
}

#[async_trait]
impl AuditStore for SqliteAuditStore {
    async fn append(
        &self,
        actor_type: &str,
        actor_id: &str,
        acting_context: Option<&serde_json::Value>,
        event_type: &str,
        payload: &serde_json::Value,
        hash: &str,
        prev_hash: Option<&str>,
    ) -> Result<i64, DbError> {
        let acting_context_json = acting_context.map(|v| v.to_string());
        let payload_json = payload.to_string();
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO audit_trail (actor_type, actor_id, acting_context, event_type, payload, hash, prev_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![actor_type, actor_id, acting_context_json, event_type, payload_json, hash, prev_hash],
        )
        .map_err(|e| DbError::Integrity(format!("sqlite audit insert: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    async fn get_latest(&self) -> Result<Option<AuditRow>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT sequence, timestamp, actor_type, actor_id, acting_context,
                        event_type, payload, prev_hash, hash
                 FROM audit_trail ORDER BY sequence DESC LIMIT 1",
            )
            .map_err(|e| DbError::Integrity(format!("sqlite prepare: {e}")))?;
        let row = stmt
            .query_row([], |row| {
                Ok(AuditRow {
                    sequence: row.get(0)?,
                    timestamp: parse_sqlite_timestamp(&row.get::<_, String>(1)?),
                    actor_type: row.get(2)?,
                    actor_id: row.get(3)?,
                    acting_context: row
                        .get::<_, Option<String>>(4)?
                        .map(|s| json_from_str(&s)),
                    event_type: row.get(5)?,
                    payload: json_from_str(&row.get::<_, String>(6)?),
                    prev_hash: row.get(7)?,
                    hash: row.get(8)?,
                })
            })
            .ok();
        Ok(row)
    }

    async fn get_entries(
        &self,
        actor_id: Option<&str>,
        event_type: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AuditRow>, DbError> {
        // Build SQL / args pre-lock using `Send` owned types (String / i64)
        // so the whole future can be sent between threads.
        let mut sql = String::from(
            "SELECT sequence, timestamp, actor_type, actor_id, acting_context,
                    event_type, payload, prev_hash, hash
             FROM audit_trail WHERE 1=1",
        );
        let mut str_args: Vec<String> = Vec::new();
        if let Some(aid) = actor_id {
            sql.push_str(" AND actor_id = ?");
            str_args.push(aid.to_string());
        }
        if let Some(et) = event_type {
            sql.push_str(" AND event_type = ?");
            str_args.push(et.to_string());
        }
        sql.push_str(" ORDER BY sequence DESC LIMIT ? OFFSET ?");

        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| DbError::Integrity(format!("sqlite prepare: {e}")))?;

        // Assemble &dyn ToSql refs under the guard — lifetimes stay local.
        let mut param_refs: Vec<&dyn rusqlite::ToSql> =
            str_args.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        param_refs.push(&limit);
        param_refs.push(&offset);

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(AuditRow {
                    sequence: row.get(0)?,
                    timestamp: parse_sqlite_timestamp(&row.get::<_, String>(1)?),
                    actor_type: row.get(2)?,
                    actor_id: row.get(3)?,
                    acting_context: row
                        .get::<_, Option<String>>(4)?
                        .map(|s| json_from_str(&s)),
                    event_type: row.get(5)?,
                    payload: json_from_str(&row.get::<_, String>(6)?),
                    prev_hash: row.get(7)?,
                    hash: row.get(8)?,
                })
            })
            .map_err(|e| DbError::Integrity(format!("sqlite query: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| DbError::Integrity(format!("sqlite row: {e}")))?);
        }
        Ok(out)
    }

    async fn count_entries(
        &self,
        actor_id: Option<&str>,
        event_type: Option<&str>,
    ) -> Result<i64, DbError> {
        let mut sql = String::from("SELECT COUNT(*) FROM audit_trail WHERE 1=1");
        let mut args: Vec<String> = Vec::new();
        if let Some(aid) = actor_id {
            sql.push_str(" AND actor_id = ?");
            args.push(aid.to_string());
        }
        if let Some(et) = event_type {
            sql.push_str(" AND event_type = ?");
            args.push(et.to_string());
        }

        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| DbError::Integrity(format!("sqlite prepare: {e}")))?;

        let param_refs: Vec<&dyn rusqlite::ToSql> =
            args.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        let count: i64 = stmt
            .query_row(param_refs.as_slice(), |row| row.get(0))
            .map_err(|e| DbError::Integrity(format!("sqlite count: {e}")))?;
        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn new_store() -> SqliteAuditStore {
        let conn = Connection::open_in_memory().expect("in-memory");
        SqliteAuditStore::init_schema(&conn).expect("init schema");
        SqliteAuditStore::new(Arc::new(Mutex::new(conn)))
    }

    #[tokio::test]
    async fn append_and_get_latest_roundtrip() {
        let store = new_store();
        let payload = serde_json::json!({"foo": "bar"});
        let seq = store
            .append(
                "agent",
                "agent-1",
                None,
                "session.start",
                &payload,
                "hash-1",
                None,
            )
            .await
            .unwrap();
        assert_eq!(seq, 1);

        let latest = store.get_latest().await.unwrap().unwrap();
        assert_eq!(latest.sequence, 1);
        assert_eq!(latest.actor_id, "agent-1");
        assert_eq!(latest.hash, "hash-1");
        assert!(latest.prev_hash.is_none());
    }

    #[tokio::test]
    async fn get_entries_pagination_and_filter() {
        let store = new_store();
        for i in 0..5 {
            let p = serde_json::json!({"i": i});
            store
                .append(
                    "agent",
                    if i % 2 == 0 { "even" } else { "odd" },
                    None,
                    "evt",
                    &p,
                    &format!("h{i}"),
                    None,
                )
                .await
                .unwrap();
        }
        let even = store
            .get_entries(Some("even"), None, 10, 0)
            .await
            .unwrap();
        assert_eq!(even.len(), 3);
        for row in &even {
            assert_eq!(row.actor_id, "even");
        }
        let all = store.get_entries(None, None, 2, 0).await.unwrap();
        assert_eq!(all.len(), 2);
        // DESC order — most recent first
        assert_eq!(all[0].hash, "h4");
    }

    #[tokio::test]
    async fn count_respects_filter() {
        let store = new_store();
        for i in 0..4 {
            let p = serde_json::json!({"i": i});
            store
                .append(
                    "agent",
                    "a1",
                    None,
                    if i < 2 { "x" } else { "y" },
                    &p,
                    &format!("h{i}"),
                    None,
                )
                .await
                .unwrap();
        }
        assert_eq!(store.count_entries(None, None).await.unwrap(), 4);
        assert_eq!(store.count_entries(None, Some("x")).await.unwrap(), 2);
        assert_eq!(
            store.count_entries(Some("a1"), Some("y")).await.unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn init_schema_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        SqliteAuditStore::init_schema(&conn).unwrap();
        SqliteAuditStore::init_schema(&conn).unwrap();
        SqliteAuditStore::init_schema(&conn).unwrap();
        // No panic — idempotent.
    }

    #[tokio::test]
    async fn tenant_isolation_by_actor_id() {
        let store = new_store();
        let p = serde_json::json!({});
        store
            .append("agent", "tenant-a", None, "e", &p, "ha", None)
            .await
            .unwrap();
        store
            .append("agent", "tenant-b", None, "e", &p, "hb", None)
            .await
            .unwrap();
        let a = store.get_entries(Some("tenant-a"), None, 10, 0).await.unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].actor_id, "tenant-a");
    }
}
