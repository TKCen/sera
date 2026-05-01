//! Cascade revocation for capability-token delegation chains (sera-2q6w).
//!
//! `capability_token_revocations` records every revoked capability-token
//! instance keyed by [`CapabilityToken::instance_id`] (sera-s64j) along with
//! the parent's instance_id captured at revocation time. The `parent_id`
//! column is indexed so the cascade walk down the delegation tree is cheap.
//!
//! # Surface
//!
//! - [`RevocationStore`] — async trait with three operations:
//!   - `revoke_cascade(instance_id, parent_id, tenant_id, reason, revoked_by)`
//!     records `instance_id` and walks the table-side delegation graph
//!     downward, returning the count of (instance_id) rows in the now-revoked
//!     subtree (including the explicitly-revoked root).
//!   - `is_revoked(instance_id, tenant_id)` — leaf lookup consulted by the
//!     [`sera_auth::EvolveTokenSigner`] verifier.
//!   - `record(...)` — register a single revocation row idempotently
//!     (used by tests and by the cascade walker to materialize descendants).
//! - [`InMemoryRevocationStore`] — `Mutex<HashMap>` backend. Used in unit
//!   tests and in deployments without a persistent DB.
//! - [`SqliteRevocationStore`] — rusqlite backend. Cascade walks descendants
//!   via Rust-side BFS with a visited set so cycles in `parent_id` cannot
//!   loop the walker. Hard-capped at 50 hops to mirror the Postgres cap.
//! - [`PostgresRevocationStore`] — sqlx backend. Cascade walks via
//!   `WITH RECURSIVE … CYCLE` (Postgres-14+ standard SQL) with the same
//!   50-hop ceiling.
//!
//! Cycle resilience is two independent mechanisms (visited set / SQL CYCLE
//! plus the depth cap) so a malformed graph cannot wedge the walker.
//!
//! Tenant isolation: every operation is scoped by `tenant_id` so a revocation
//! recorded under tenant A is invisible from tenant B's perspective. The PK
//! is `(tenant_id, instance_id)` and the cascade walk only follows
//! parent_id edges within the same tenant.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{params, Connection};
use sqlx::PgPool;
use tokio::sync::Mutex as AsyncMutex;

use crate::error::DbError;

/// Hard ceiling on cascade walk depth. Mirrors the ZeroID 50-hop cap and is a
/// belt-and-suspenders defense alongside the per-backend visited-set / SQL
/// CYCLE clause.
pub const CASCADE_MAX_DEPTH: u32 = 50;

/// One revocation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevocationRow {
    pub tenant_id: String,
    pub instance_id: String,
    pub parent_id: Option<String>,
    pub reason: String,
    pub revoked_by: String,
    pub revoked_at_ms: i64,
}

/// Async revocation store.
///
/// Implementations must be safe to share across the gateway's `Arc<dyn …>`
/// state pattern.
#[async_trait]
pub trait RevocationStore: Send + Sync + std::fmt::Debug {
    /// Idempotently insert a single revocation row. No tree walk.
    ///
    /// Returns `true` when a new row was inserted, `false` when the
    /// `(tenant_id, instance_id)` pair already existed.
    async fn record(
        &self,
        tenant_id: &str,
        instance_id: &str,
        parent_id: Option<&str>,
        reason: &str,
        revoked_by: &str,
    ) -> Result<bool, DbError>;

    /// Revoke `instance_id` and every descendant reachable via `parent_id`
    /// edges in the revocation table itself. Returns the count of distinct
    /// instance_ids in the resulting subtree (including the root).
    ///
    /// The walk is cycle-safe (visited set / SQL CYCLE) and depth-capped at
    /// [`CASCADE_MAX_DEPTH`].
    async fn revoke_cascade(
        &self,
        tenant_id: &str,
        instance_id: &str,
        parent_id: Option<&str>,
        reason: &str,
        revoked_by: &str,
    ) -> Result<u32, DbError>;

    /// Return `true` when `instance_id` has a revocation row in this tenant.
    async fn is_revoked(&self, tenant_id: &str, instance_id: &str) -> Result<bool, DbError>;
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Emit a structured observability event for a cascade revocation.
///
/// Wired through `tracing::info!` so it lands in the gateway's OTel exporter.
/// Hash-chained audit emission is deliberately out of scope here; the event
/// name is stable so downstream subscribers can tap it.
fn emit_cascade_event(
    tenant_id: &str,
    instance_id: &str,
    revoked_by: &str,
    reason: &str,
    cascaded: u32,
) {
    tracing::info!(
        target: "sera_db::capability_revocation",
        event = "capability_token.revocation_cascade",
        tenant_id = %tenant_id,
        instance_id = %instance_id,
        revoked_by = %revoked_by,
        reason = %reason,
        cascaded = cascaded,
        "cascade revocation"
    );
}

// ── In-memory backend ──────────────────────────────────────────────────────

/// In-memory revocation store. Keyed by `(tenant_id, instance_id)`.
#[derive(Debug, Default)]
pub struct InMemoryRevocationStore {
    rows: AsyncMutex<HashMap<(String, String), RevocationRow>>,
}

impl InMemoryRevocationStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn cascade_walk(
        rows: &HashMap<(String, String), RevocationRow>,
        tenant_id: &str,
        seed: &str,
    ) -> u32 {
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(seed.to_string());
        let mut frontier = vec![seed.to_string()];
        let mut depth: u32 = 0;
        while !frontier.is_empty() && depth < CASCADE_MAX_DEPTH {
            let mut next = Vec::new();
            for parent in &frontier {
                for ((row_tenant, row_instance), row) in rows.iter() {
                    if row_tenant != tenant_id {
                        continue;
                    }
                    if row.parent_id.as_deref() != Some(parent.as_str()) {
                        continue;
                    }
                    if visited.insert(row_instance.clone()) {
                        next.push(row_instance.clone());
                    }
                }
            }
            frontier = next;
            depth += 1;
        }
        visited.len() as u32
    }
}

#[async_trait]
impl RevocationStore for InMemoryRevocationStore {
    async fn record(
        &self,
        tenant_id: &str,
        instance_id: &str,
        parent_id: Option<&str>,
        reason: &str,
        revoked_by: &str,
    ) -> Result<bool, DbError> {
        let mut rows = self.rows.lock().await;
        let key = (tenant_id.to_string(), instance_id.to_string());
        if rows.contains_key(&key) {
            return Ok(false);
        }
        rows.insert(
            key,
            RevocationRow {
                tenant_id: tenant_id.to_string(),
                instance_id: instance_id.to_string(),
                parent_id: parent_id.map(str::to_string),
                reason: reason.to_string(),
                revoked_by: revoked_by.to_string(),
                revoked_at_ms: now_ms(),
            },
        );
        Ok(true)
    }

    async fn revoke_cascade(
        &self,
        tenant_id: &str,
        instance_id: &str,
        parent_id: Option<&str>,
        reason: &str,
        revoked_by: &str,
    ) -> Result<u32, DbError> {
        let mut rows = self.rows.lock().await;
        rows.entry((tenant_id.to_string(), instance_id.to_string()))
            .or_insert(RevocationRow {
                tenant_id: tenant_id.to_string(),
                instance_id: instance_id.to_string(),
                parent_id: parent_id.map(str::to_string),
                reason: reason.to_string(),
                revoked_by: revoked_by.to_string(),
                revoked_at_ms: now_ms(),
            });
        let count = Self::cascade_walk(&rows, tenant_id, instance_id);
        drop(rows);
        emit_cascade_event(tenant_id, instance_id, revoked_by, reason, count);
        Ok(count)
    }

    async fn is_revoked(&self, tenant_id: &str, instance_id: &str) -> Result<bool, DbError> {
        let rows = self.rows.lock().await;
        Ok(rows.contains_key(&(tenant_id.to_string(), instance_id.to_string())))
    }
}

// ── SQLite backend ────────────────────────────────────────────────────────

/// SQLite-backed revocation store.
#[derive(Debug, Clone)]
pub struct SqliteRevocationStore {
    conn: Arc<AsyncMutex<Connection>>,
}

impl SqliteRevocationStore {
    pub fn new(conn: Arc<AsyncMutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Create the `capability_token_revocations` table idempotently.
    pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS capability_token_revocations (
                tenant_id    TEXT    NOT NULL,
                instance_id  TEXT    NOT NULL,
                parent_id    TEXT,
                reason       TEXT    NOT NULL,
                revoked_by   TEXT    NOT NULL,
                revoked_at   INTEGER NOT NULL,
                PRIMARY KEY (tenant_id, instance_id)
            );
            CREATE INDEX IF NOT EXISTS idx_capability_token_revocations_parent_id
                ON capability_token_revocations (tenant_id, parent_id);",
        )?;
        Ok(())
    }

    fn cascade_walk(
        conn: &Connection,
        tenant_id: &str,
        seed: &str,
    ) -> rusqlite::Result<u32> {
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(seed.to_string());
        let mut frontier = vec![seed.to_string()];
        let mut depth: u32 = 0;
        while !frontier.is_empty() && depth < CASCADE_MAX_DEPTH {
            let mut next: Vec<String> = Vec::new();
            for parent in &frontier {
                let mut stmt = conn.prepare(
                    "SELECT instance_id FROM capability_token_revocations
                     WHERE tenant_id = ?1 AND parent_id = ?2",
                )?;
                let rows = stmt.query_map(params![tenant_id, parent], |row| {
                    row.get::<_, String>(0)
                })?;
                for row in rows {
                    let child = row?;
                    if visited.insert(child.clone()) {
                        next.push(child);
                    }
                }
            }
            frontier = next;
            depth += 1;
        }
        Ok(visited.len() as u32)
    }
}

#[async_trait]
impl RevocationStore for SqliteRevocationStore {
    async fn record(
        &self,
        tenant_id: &str,
        instance_id: &str,
        parent_id: Option<&str>,
        reason: &str,
        revoked_by: &str,
    ) -> Result<bool, DbError> {
        let conn = self.conn.lock().await;
        let changed = conn
            .execute(
                "INSERT OR IGNORE INTO capability_token_revocations
                    (tenant_id, instance_id, parent_id, reason, revoked_by, revoked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    tenant_id,
                    instance_id,
                    parent_id,
                    reason,
                    revoked_by,
                    now_ms()
                ],
            )
            .map_err(|e| DbError::Integrity(format!("sqlite insert: {e}")))?;
        Ok(changed > 0)
    }

    async fn revoke_cascade(
        &self,
        tenant_id: &str,
        instance_id: &str,
        parent_id: Option<&str>,
        reason: &str,
        revoked_by: &str,
    ) -> Result<u32, DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT OR IGNORE INTO capability_token_revocations
                (tenant_id, instance_id, parent_id, reason, revoked_by, revoked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                tenant_id,
                instance_id,
                parent_id,
                reason,
                revoked_by,
                now_ms()
            ],
        )
        .map_err(|e| DbError::Integrity(format!("sqlite insert: {e}")))?;
        let count = Self::cascade_walk(&conn, tenant_id, instance_id)
            .map_err(|e| DbError::Integrity(format!("sqlite cascade walk: {e}")))?;
        drop(conn);
        emit_cascade_event(tenant_id, instance_id, revoked_by, reason, count);
        Ok(count)
    }

    async fn is_revoked(&self, tenant_id: &str, instance_id: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().await;
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM capability_token_revocations
                    WHERE tenant_id = ?1 AND instance_id = ?2
                )",
                params![tenant_id, instance_id],
                |row| row.get(0),
            )
            .map_err(|e| DbError::Integrity(format!("sqlite is_revoked: {e}")))?;
        Ok(exists)
    }
}

// ── Postgres backend ──────────────────────────────────────────────────────

/// Postgres DDL for `capability_token_revocations`. Kept as a const so
/// integration tests and deployment migrations can apply it verbatim.
pub const POSTGRES_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS capability_token_revocations (
    tenant_id    TEXT        NOT NULL,
    instance_id  TEXT        NOT NULL,
    parent_id    TEXT,
    reason       TEXT        NOT NULL,
    revoked_by   TEXT        NOT NULL,
    revoked_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, instance_id)
);
CREATE INDEX IF NOT EXISTS idx_capability_token_revocations_parent_id
    ON capability_token_revocations (tenant_id, parent_id);
"#;

const POSTGRES_CASCADE_DESCENDANTS_SQL: &str = r#"
WITH RECURSIVE descendants AS (
    SELECT instance_id, parent_id, 1 AS depth
    FROM capability_token_revocations
    WHERE tenant_id = $1 AND instance_id = $2
  UNION ALL
    SELECT c.instance_id, c.parent_id, d.depth + 1
    FROM capability_token_revocations c
    JOIN descendants d ON c.parent_id = d.instance_id
    WHERE c.tenant_id = $1 AND d.depth <= $3
) CYCLE instance_id SET is_cycle USING path
SELECT COUNT(DISTINCT instance_id)::bigint FROM descendants
"#;

/// Postgres-backed revocation store. Cascade walks via `WITH RECURSIVE …
/// CYCLE` so a malformed parent_id graph is structurally cycle-safe.
#[derive(Debug, Clone)]
pub struct PostgresRevocationStore {
    pool: PgPool,
}

impl PostgresRevocationStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn from_db_pool(db: &crate::DbPool) -> Self {
        Self::new(db.inner().clone())
    }
}

#[async_trait]
impl RevocationStore for PostgresRevocationStore {
    async fn record(
        &self,
        tenant_id: &str,
        instance_id: &str,
        parent_id: Option<&str>,
        reason: &str,
        revoked_by: &str,
    ) -> Result<bool, DbError> {
        let row: Option<(String,)> = sqlx::query_as(
            r#"
            INSERT INTO capability_token_revocations
                (tenant_id, instance_id, parent_id, reason, revoked_by)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (tenant_id, instance_id) DO NOTHING
            RETURNING instance_id
            "#,
        )
        .bind(tenant_id)
        .bind(instance_id)
        .bind(parent_id)
        .bind(reason)
        .bind(revoked_by)
        .fetch_optional(&self.pool)
        .await
        .map_err(DbError::Sqlx)?;
        Ok(row.is_some())
    }

    async fn revoke_cascade(
        &self,
        tenant_id: &str,
        instance_id: &str,
        parent_id: Option<&str>,
        reason: &str,
        revoked_by: &str,
    ) -> Result<u32, DbError> {
        sqlx::query(
            r#"
            INSERT INTO capability_token_revocations
                (tenant_id, instance_id, parent_id, reason, revoked_by)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (tenant_id, instance_id) DO NOTHING
            "#,
        )
        .bind(tenant_id)
        .bind(instance_id)
        .bind(parent_id)
        .bind(reason)
        .bind(revoked_by)
        .execute(&self.pool)
        .await
        .map_err(DbError::Sqlx)?;

        // Recursive CTE walk down the parent_id graph. CYCLE guards against
        // malformed cycles; the depth cap is the second mechanism.
        let row: (i64,) = sqlx::query_as(POSTGRES_CASCADE_DESCENDANTS_SQL)
            .bind(tenant_id)
            .bind(instance_id)
            .bind(i64::from(CASCADE_MAX_DEPTH))
            .fetch_one(&self.pool)
            .await
            .map_err(DbError::Sqlx)?;

        let count = row.0.max(0) as u32;
        emit_cascade_event(tenant_id, instance_id, revoked_by, reason, count);
        Ok(count)
    }

    async fn is_revoked(&self, tenant_id: &str, instance_id: &str) -> Result<bool, DbError> {
        let row: Option<(bool,)> = sqlx::query_as(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM capability_token_revocations
                WHERE tenant_id = $1 AND instance_id = $2
            )
            "#,
        )
        .bind(tenant_id)
        .bind(instance_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(DbError::Sqlx)?;
        Ok(row.map(|(b,)| b).unwrap_or(false))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite_store() -> SqliteRevocationStore {
        let conn = Connection::open_in_memory().unwrap();
        SqliteRevocationStore::init_schema(&conn).unwrap();
        SqliteRevocationStore::new(Arc::new(AsyncMutex::new(conn)))
    }

    async fn seed_chain<S: RevocationStore>(
        store: &S,
        tenant: &str,
        ids_in_order: &[&str],
        reason: &str,
        revoked_by: &str,
    ) {
        // ids_in_order = [root, child, grandchild, …]
        for (idx, id) in ids_in_order.iter().enumerate() {
            let parent = if idx == 0 {
                None
            } else {
                Some(ids_in_order[idx - 1])
            };
            store
                .record(tenant, id, parent, reason, revoked_by)
                .await
                .unwrap();
        }
    }

    // ─── In-memory backend ──────────────────────────────────────────────

    #[tokio::test]
    async fn inmem_record_is_idempotent() {
        let s = InMemoryRevocationStore::new();
        assert!(s.record("t", "a", None, "r", "p").await.unwrap());
        assert!(!s.record("t", "a", None, "r", "p").await.unwrap());
    }

    #[tokio::test]
    async fn inmem_is_revoked_round_trips() {
        let s = InMemoryRevocationStore::new();
        s.record("t", "a", None, "r", "p").await.unwrap();
        assert!(s.is_revoked("t", "a").await.unwrap());
        assert!(!s.is_revoked("t", "missing").await.unwrap());
    }

    #[tokio::test]
    async fn inmem_cascade_depth_1() {
        // root → child. Revoking root counts child as cascaded.
        let s = InMemoryRevocationStore::new();
        seed_chain(&s, "t", &["root", "child"], "r", "p").await;
        let n = s.revoke_cascade("t", "root", None, "audit", "op")
            .await
            .unwrap();
        assert_eq!(n, 2);
    }

    #[tokio::test]
    async fn inmem_cascade_depth_2() {
        let s = InMemoryRevocationStore::new();
        seed_chain(&s, "t", &["a", "b", "c"], "r", "p").await;
        let n = s.revoke_cascade("t", "a", None, "audit", "op")
            .await
            .unwrap();
        assert_eq!(n, 3);
    }

    #[tokio::test]
    async fn inmem_cascade_depth_3() {
        let s = InMemoryRevocationStore::new();
        seed_chain(&s, "t", &["a", "b", "c", "d"], "r", "p").await;
        let n = s.revoke_cascade("t", "a", None, "audit", "op")
            .await
            .unwrap();
        assert_eq!(n, 4);
    }

    #[tokio::test]
    async fn inmem_cascade_cycle_resilient() {
        // Inject a cycle: a → b → a.
        let s = InMemoryRevocationStore::new();
        s.record("t", "a", Some("b"), "r", "p").await.unwrap();
        s.record("t", "b", Some("a"), "r", "p").await.unwrap();
        let n = s.revoke_cascade("t", "a", Some("b"), "audit", "op")
            .await
            .unwrap();
        // Visited-set caps the walk at the two cycle members.
        assert_eq!(n, 2);
    }

    #[tokio::test]
    async fn inmem_cascade_tenants_are_isolated() {
        let s = InMemoryRevocationStore::new();
        // tenant-a has a 3-deep chain; tenant-b shares the same instance_ids
        // but a single root only — the cascade in tenant-a must not count
        // tenant-b's row.
        seed_chain(&s, "tenant-a", &["a", "b", "c"], "r", "p").await;
        s.record("tenant-b", "a", None, "r", "p").await.unwrap();
        let n_a = s.revoke_cascade("tenant-a", "a", None, "audit", "op")
            .await
            .unwrap();
        assert_eq!(n_a, 3);
        let n_b = s.revoke_cascade("tenant-b", "a", None, "audit", "op")
            .await
            .unwrap();
        assert_eq!(n_b, 1);
        // is_revoked is also tenant-scoped.
        assert!(s.is_revoked("tenant-a", "b").await.unwrap());
        assert!(!s.is_revoked("tenant-b", "b").await.unwrap());
    }

    // ─── SQLite backend ─────────────────────────────────────────────────

    #[tokio::test]
    async fn sqlite_record_is_idempotent() {
        let s = sqlite_store();
        assert!(s.record("t", "a", None, "r", "p").await.unwrap());
        assert!(!s.record("t", "a", None, "r", "p").await.unwrap());
    }

    #[tokio::test]
    async fn sqlite_is_revoked_round_trips() {
        let s = sqlite_store();
        s.record("t", "a", None, "r", "p").await.unwrap();
        assert!(s.is_revoked("t", "a").await.unwrap());
        assert!(!s.is_revoked("t", "nope").await.unwrap());
    }

    #[tokio::test]
    async fn sqlite_cascade_depth_1_2_3() {
        for chain in [
            &["a", "b"][..],
            &["a", "b", "c"][..],
            &["a", "b", "c", "d"][..],
        ] {
            let s = sqlite_store();
            seed_chain(&s, "t", chain, "r", "p").await;
            let n = s.revoke_cascade("t", "a", None, "audit", "op")
                .await
                .unwrap();
            assert_eq!(n as usize, chain.len());
        }
    }

    #[tokio::test]
    async fn sqlite_cascade_cycle_resilient() {
        let s = sqlite_store();
        s.record("t", "a", Some("b"), "r", "p").await.unwrap();
        s.record("t", "b", Some("a"), "r", "p").await.unwrap();
        let n = s.revoke_cascade("t", "a", Some("b"), "audit", "op")
            .await
            .unwrap();
        assert_eq!(n, 2);
    }

    #[tokio::test]
    async fn sqlite_cascade_tenants_are_isolated() {
        let s = sqlite_store();
        seed_chain(&s, "tenant-a", &["a", "b", "c"], "r", "p").await;
        s.record("tenant-b", "a", None, "r", "p").await.unwrap();
        let n_a = s.revoke_cascade("tenant-a", "a", None, "audit", "op")
            .await
            .unwrap();
        assert_eq!(n_a, 3);
        let n_b = s.revoke_cascade("tenant-b", "a", None, "audit", "op")
            .await
            .unwrap();
        assert_eq!(n_b, 1);
        assert!(s.is_revoked("tenant-a", "b").await.unwrap());
        assert!(!s.is_revoked("tenant-b", "b").await.unwrap());
    }

    #[tokio::test]
    async fn sqlite_init_schema_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        SqliteRevocationStore::init_schema(&conn).unwrap();
        SqliteRevocationStore::init_schema(&conn).unwrap();
        SqliteRevocationStore::init_schema(&conn).unwrap();
    }

    #[tokio::test]
    async fn sqlite_index_on_parent_id_exists() {
        let conn = Connection::open_in_memory().unwrap();
        SqliteRevocationStore::init_schema(&conn).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='index' AND name='idx_capability_token_revocations_parent_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "parent_id index must exist for cascade walk");
    }

    // ─── Postgres SQL shape (no DB; CI runs SQLite path for behavior) ───

    #[test]
    fn postgres_ddl_indexes_parent_id() {
        // Bead AC explicitly requires the parent_id index. Pin its presence
        // in the DDL constant since CI cannot run a real Postgres.
        assert!(
            POSTGRES_DDL.contains("idx_capability_token_revocations_parent_id"),
            "POSTGRES_DDL must declare the parent_id index"
        );
        assert!(
            POSTGRES_DDL.contains("PRIMARY KEY (tenant_id, instance_id)"),
            "PK must be (tenant_id, instance_id) for cross-tenant isolation"
        );
        assert!(POSTGRES_DDL.contains("CREATE TABLE IF NOT EXISTS capability_token_revocations"));
    }

    #[test]
    fn postgres_cascade_walk_uses_full_edge_depth_budget() {
        assert!(
            POSTGRES_CASCADE_DESCENDANTS_SQL.contains("d.depth <= $3"),
            "Postgres cascade must include children at the configured max-hop boundary"
        );
    }

    #[test]
    fn sqlite_ddl_indexes_parent_id() {
        // Same shape pin as POSTGRES_DDL but rendered through the live
        // rusqlite session.
        let conn = Connection::open_in_memory().unwrap();
        SqliteRevocationStore::init_schema(&conn).unwrap();
        let parent_id_index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='index' AND name='idx_capability_token_revocations_parent_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(parent_id_index_count, 1);
        let pk_pragma: Vec<(i64, String, i64)> = conn
            .prepare("PRAGMA table_info(capability_token_revocations)")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let pk_cols: Vec<&str> = pk_pragma
            .iter()
            .filter(|(_, _, pk)| *pk > 0)
            .map(|(_, name, _)| name.as_str())
            .collect();
        assert!(pk_cols.contains(&"tenant_id"));
        assert!(pk_cols.contains(&"instance_id"));
    }

    #[tokio::test]
    async fn cascade_walk_respects_depth_cap() {
        // Build a single-thread chain longer than CASCADE_MAX_DEPTH; the
        // walker must stop at the cap rather than spinning forever.
        let s = InMemoryRevocationStore::new();
        let chain: Vec<String> =
            (0..(CASCADE_MAX_DEPTH + 5)).map(|i| format!("n-{i}")).collect();
        let chain_refs: Vec<&str> = chain.iter().map(String::as_str).collect();
        seed_chain(&s, "t", &chain_refs, "r", "p").await;
        let n = s.revoke_cascade("t", chain_refs[0], None, "audit", "op")
            .await
            .unwrap();
        // BFS from depth 0 reaches up to CASCADE_MAX_DEPTH levels of children
        // beyond the seed → CASCADE_MAX_DEPTH + 1 nodes counted.
        assert_eq!(n, CASCADE_MAX_DEPTH + 1);
    }
}
