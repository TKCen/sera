//! Metering repository — token usage tracking and budget enforcement.
//!
//! Dual-backend (sera-mwb4): see [`MeteringStore`] for the trait, with
//! [`PgMeteringStore`] (sqlx on Postgres) and [`SqliteMeteringStore`]
//! (rusqlite on SQLite) implementations.

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{params, Connection};
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::error::DbError;

/// Row type for usage aggregation queries.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UsageAggRow {
    pub total_tokens: Option<i64>,
}

/// Row type for token_quotas table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct QuotaRow {
    pub agent_id: String,
    pub max_tokens_per_hour: i32,
    pub max_tokens_per_day: i32,
}

/// Row type for daily usage aggregation.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DailyUsageRow {
    pub date: time::Date,
    pub total_tokens: i64,
}

/// Row type for agent ranking.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AgentRankingRow {
    pub agent_id: String,
    pub total_tokens: i64,
}

/// Input for recording a token usage event.
pub struct RecordUsageInput<'a> {
    pub agent_id: &'a str,
    pub circle_id: Option<&'a str>,
    pub model: &'a str,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub cost_usd: Option<f64>,
    pub latency_ms: Option<i64>,
    pub status: &'a str,
}

/// Metering repository for database operations.
pub struct MeteringRepository;

impl MeteringRepository {
    /// Record a token usage event.
    pub async fn record_usage(pool: &PgPool, input: RecordUsageInput<'_>) -> Result<(), DbError> {
        // Insert into token_usage
        sqlx::query(
            "INSERT INTO token_usage (agent_id, circle_id, model, prompt_tokens, completion_tokens, total_tokens, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, NOW())"
        )
        .bind(input.agent_id)
        .bind(input.circle_id)
        .bind(input.model)
        .bind(input.prompt_tokens)
        .bind(input.completion_tokens)
        .bind(input.total_tokens)
        .execute(pool)
        .await?;

        // Insert into usage_events (detailed record)
        sqlx::query(
            "INSERT INTO usage_events (agent_id, model, prompt_tokens, completion_tokens, total_tokens, cost_usd, latency_ms, status, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())"
        )
        .bind(input.agent_id)
        .bind(input.model)
        .bind(input.prompt_tokens)
        .bind(input.completion_tokens)
        .bind(input.total_tokens)
        .bind(input.cost_usd)
        .bind(input.latency_ms)
        .bind(input.status)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Get total token usage for an agent within a time window.
    pub async fn get_usage_in_window(
        pool: &PgPool,
        agent_id: &str,
        window_hours: i32,
    ) -> Result<i64, DbError> {
        let row = sqlx::query_as::<_, UsageAggRow>(
            "SELECT COALESCE(SUM(total_tokens), 0) as total_tokens
             FROM token_usage
             WHERE agent_id = $1 AND created_at > NOW() - make_interval(hours => $2)"
        )
        .bind(agent_id)
        .bind(window_hours)
        .fetch_one(pool)
        .await?;
        Ok(row.total_tokens.unwrap_or(0))
    }

    /// Get quota for an agent.
    pub async fn get_quota(pool: &PgPool, agent_id: &str) -> Result<Option<QuotaRow>, DbError> {
        let row = sqlx::query_as::<_, QuotaRow>(
            "SELECT agent_id, max_tokens_per_hour, max_tokens_per_day
             FROM token_quotas WHERE agent_id = $1"
        )
        .bind(agent_id)
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// Upsert token quota for an agent.
    /// Uses COALESCE so that passing None for a field preserves the existing value.
    pub async fn upsert_quota(
        pool: &PgPool,
        agent_id: &str,
        max_tokens_per_hour: Option<i64>,
        max_tokens_per_day: Option<i64>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO token_quotas (agent_id, max_tokens_per_hour, max_tokens_per_day, source, updated_at)
             VALUES ($1, COALESCE($2, 100000), COALESCE($3, 1000000), 'operator', NOW())
             ON CONFLICT (agent_id)
             DO UPDATE SET
               max_tokens_per_hour = COALESCE($2, token_quotas.max_tokens_per_hour),
               max_tokens_per_day  = COALESCE($3, token_quotas.max_tokens_per_day),
               source = 'operator',
               updated_at = NOW()"
        )
        .bind(agent_id)
        .bind(max_tokens_per_hour)
        .bind(max_tokens_per_day)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Reset usage counters for an agent by deleting their token_usage rows.
    pub async fn reset_usage(pool: &PgPool, agent_id: &str) -> Result<u64, DbError> {
        let result = sqlx::query("DELETE FROM token_usage WHERE agent_id = $1")
            .bind(agent_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Global usage totals grouped by day (last 7 days).
    pub async fn global_daily_usage(pool: &PgPool) -> Result<Vec<DailyUsageRow>, DbError> {
        let rows = sqlx::query_as::<_, DailyUsageRow>(
            "SELECT DATE_TRUNC('day', created_at)::date AS date,
                    COALESCE(SUM(total_tokens), 0) AS total_tokens
             FROM token_usage
             WHERE created_at >= NOW() - INTERVAL '7 days'
             GROUP BY DATE_TRUNC('day', created_at)::date
             ORDER BY date ASC",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Per-agent token rankings (total tokens per agent, descending).
    pub async fn agent_rankings(pool: &PgPool) -> Result<Vec<AgentRankingRow>, DbError> {
        let rows = sqlx::query_as::<_, AgentRankingRow>(
            "SELECT agent_id, COALESCE(SUM(total_tokens), 0) AS total_tokens
             FROM token_usage
             GROUP BY agent_id
             ORDER BY total_tokens DESC",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Single agent usage grouped by day (last 7 days).
    pub async fn agent_daily_usage(
        pool: &PgPool,
        agent_id: &str,
    ) -> Result<Vec<DailyUsageRow>, DbError> {
        let rows = sqlx::query_as::<_, DailyUsageRow>(
            "SELECT DATE_TRUNC('day', created_at)::date AS date,
                    COALESCE(SUM(total_tokens), 0) AS total_tokens
             FROM token_usage
             WHERE agent_id = $1 AND created_at >= NOW() - INTERVAL '7 days'
             GROUP BY DATE_TRUNC('day', created_at)::date
             ORDER BY date ASC",
        )
        .bind(agent_id)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Metering summary — today's totals across all agents.
    pub async fn today_summary(pool: &PgPool) -> Result<UsageAggRow, DbError> {
        let row = sqlx::query_as::<_, UsageAggRow>(
            "SELECT COALESCE(SUM(total_tokens), 0) AS total_tokens
             FROM token_usage
             WHERE created_at >= DATE_TRUNC('day', NOW())",
        )
        .fetch_one(pool)
        .await?;
        Ok(row)
    }

    /// Check budget for an agent. Returns (allowed, hourly_used, hourly_quota, daily_used, daily_quota).
    pub async fn check_budget(
        pool: &PgPool,
        agent_id: &str,
    ) -> Result<sera_types::metering::BudgetStatus, DbError> {
        let quota = Self::get_quota(pool, agent_id).await?;

        let (hourly_quota, daily_quota) = match &quota {
            Some(q) => {
                // Quota of 0 means unlimited
                (q.max_tokens_per_hour as i64, q.max_tokens_per_day as i64)
            }
            None => (100_000i64, 1_000_000i64), // Defaults from MeteringService
        };

        let hourly_used = Self::get_usage_in_window(pool, agent_id, 1).await?;
        let daily_used = Self::get_usage_in_window(pool, agent_id, 24).await?;

        let allowed = (hourly_quota == 0 || hourly_used < hourly_quota)
            && (daily_quota == 0 || daily_used < daily_quota);

        Ok(sera_types::metering::BudgetStatus {
            allowed,
            hourly_used: hourly_used as u64,
            hourly_quota: hourly_quota as u64,
            daily_used: daily_used as u64,
            daily_quota: daily_quota as u64,
        })
    }
}

// ---------------------------------------------------------------------------
// Dual-backend trait (sera-mwb4)
// ---------------------------------------------------------------------------

/// Common metering surface shared by Postgres and SQLite backends.
#[async_trait]
pub trait MeteringStore: Send + Sync + std::fmt::Debug {
    /// Record a single usage event.
    async fn record_usage(&self, input: RecordUsageInput<'_>) -> Result<(), DbError>;

    /// Total tokens consumed by `agent_id` in the trailing `window_hours` window.
    async fn get_usage_in_window(
        &self,
        agent_id: &str,
        window_hours: i32,
    ) -> Result<i64, DbError>;

    /// Fetch the quota row for `agent_id`, or `None` if none is set.
    async fn get_quota(&self, agent_id: &str) -> Result<Option<QuotaRow>, DbError>;

    /// Upsert per-agent quota. `None` fields preserve the existing value.
    async fn upsert_quota(
        &self,
        agent_id: &str,
        max_tokens_per_hour: Option<i64>,
        max_tokens_per_day: Option<i64>,
    ) -> Result<(), DbError>;

    /// Delete all usage rows for `agent_id`. Returns number of rows deleted.
    async fn reset_usage(&self, agent_id: &str) -> Result<u64, DbError>;

    /// Check budget and return a fully-populated [`sera_types::metering::BudgetStatus`].
    async fn check_budget(
        &self,
        agent_id: &str,
    ) -> Result<sera_types::metering::BudgetStatus, DbError>;
}

/// Postgres implementation of [`MeteringStore`] — delegates to the existing
/// [`MeteringRepository`] so enterprise deployments keep the legacy surface.
#[derive(Debug, Clone)]
pub struct PgMeteringStore {
    pool: PgPool,
}

impl PgMeteringStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MeteringStore for PgMeteringStore {
    async fn record_usage(&self, input: RecordUsageInput<'_>) -> Result<(), DbError> {
        MeteringRepository::record_usage(&self.pool, input).await
    }

    async fn get_usage_in_window(
        &self,
        agent_id: &str,
        window_hours: i32,
    ) -> Result<i64, DbError> {
        MeteringRepository::get_usage_in_window(&self.pool, agent_id, window_hours).await
    }

    async fn get_quota(&self, agent_id: &str) -> Result<Option<QuotaRow>, DbError> {
        MeteringRepository::get_quota(&self.pool, agent_id).await
    }

    async fn upsert_quota(
        &self,
        agent_id: &str,
        max_tokens_per_hour: Option<i64>,
        max_tokens_per_day: Option<i64>,
    ) -> Result<(), DbError> {
        MeteringRepository::upsert_quota(
            &self.pool,
            agent_id,
            max_tokens_per_hour,
            max_tokens_per_day,
        )
        .await
    }

    async fn reset_usage(&self, agent_id: &str) -> Result<u64, DbError> {
        MeteringRepository::reset_usage(&self.pool, agent_id).await
    }

    async fn check_budget(
        &self,
        agent_id: &str,
    ) -> Result<sera_types::metering::BudgetStatus, DbError> {
        MeteringRepository::check_budget(&self.pool, agent_id).await
    }
}

// ---------------------------------------------------------------------------
// SQLite implementation (sera-mwb4)
// ---------------------------------------------------------------------------

/// SQLite-backed metering store.
///
/// Schema (`token_usage`, `usage_events`, `token_quotas`) is created via
/// [`Self::init_schema`]. Uses rusqlite's `strftime` / `datetime('now')` for
/// time maths rather than Postgres' `make_interval` / `DATE_TRUNC`.
#[derive(Debug, Clone)]
pub struct SqliteMeteringStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteMeteringStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS token_usage (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                agent_id         TEXT NOT NULL,
                circle_id        TEXT,
                model            TEXT NOT NULL,
                prompt_tokens    INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens     INTEGER NOT NULL DEFAULT 0,
                created_at       TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_token_usage_agent ON token_usage(agent_id, created_at);

            CREATE TABLE IF NOT EXISTS usage_events (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                agent_id         TEXT NOT NULL,
                model            TEXT NOT NULL,
                prompt_tokens    INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens     INTEGER NOT NULL DEFAULT 0,
                cost_usd         REAL,
                latency_ms       INTEGER,
                status           TEXT NOT NULL,
                created_at       TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_usage_events_agent ON usage_events(agent_id, created_at);

            CREATE TABLE IF NOT EXISTS token_quotas (
                agent_id             TEXT PRIMARY KEY,
                max_tokens_per_hour  INTEGER NOT NULL DEFAULT 100000,
                max_tokens_per_day   INTEGER NOT NULL DEFAULT 1000000,
                source               TEXT NOT NULL DEFAULT 'operator',
                updated_at           TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
    }
}

#[async_trait]
impl MeteringStore for SqliteMeteringStore {
    async fn record_usage(&self, input: RecordUsageInput<'_>) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO token_usage (agent_id, circle_id, model, prompt_tokens, completion_tokens, total_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                input.agent_id,
                input.circle_id,
                input.model,
                input.prompt_tokens,
                input.completion_tokens,
                input.total_tokens
            ],
        )
        .map_err(|e| DbError::Integrity(format!("sqlite token_usage insert: {e}")))?;

        conn.execute(
            "INSERT INTO usage_events (agent_id, model, prompt_tokens, completion_tokens, total_tokens, cost_usd, latency_ms, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                input.agent_id,
                input.model,
                input.prompt_tokens,
                input.completion_tokens,
                input.total_tokens,
                input.cost_usd,
                input.latency_ms,
                input.status
            ],
        )
        .map_err(|e| DbError::Integrity(format!("sqlite usage_events insert: {e}")))?;
        Ok(())
    }

    async fn get_usage_in_window(
        &self,
        agent_id: &str,
        window_hours: i32,
    ) -> Result<i64, DbError> {
        let conn = self.conn.lock().await;
        // rusqlite supports `datetime('now', '-<n> hours')` modifier.
        let modifier = format!("-{window_hours} hours");
        let total: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0)
                 FROM token_usage
                 WHERE agent_id = ?1 AND created_at > datetime('now', ?2)",
                params![agent_id, modifier],
                |row| row.get(0),
            )
            .map_err(|e| DbError::Integrity(format!("sqlite sum usage: {e}")))?;
        Ok(total)
    }

    async fn get_quota(&self, agent_id: &str) -> Result<Option<QuotaRow>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT agent_id, max_tokens_per_hour, max_tokens_per_day
                 FROM token_quotas WHERE agent_id = ?1",
            )
            .map_err(|e| DbError::Integrity(format!("sqlite prepare: {e}")))?;
        let row = stmt
            .query_row(params![agent_id], |row| {
                Ok(QuotaRow {
                    agent_id: row.get(0)?,
                    max_tokens_per_hour: row.get(1)?,
                    max_tokens_per_day: row.get(2)?,
                })
            })
            .ok();
        Ok(row)
    }

    async fn upsert_quota(
        &self,
        agent_id: &str,
        max_tokens_per_hour: Option<i64>,
        max_tokens_per_day: Option<i64>,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        // We intentionally expose the default (100_000 / 1_000_000) matching the
        // Postgres branch when no row exists yet. On conflict, COALESCE-style
        // preservation: use the provided value or keep the old one.
        conn.execute(
            "INSERT INTO token_quotas (agent_id, max_tokens_per_hour, max_tokens_per_day, source, updated_at)
             VALUES (?1, COALESCE(?2, 100000), COALESCE(?3, 1000000), 'operator', datetime('now'))
             ON CONFLICT(agent_id) DO UPDATE SET
                max_tokens_per_hour = COALESCE(?2, token_quotas.max_tokens_per_hour),
                max_tokens_per_day  = COALESCE(?3, token_quotas.max_tokens_per_day),
                source = 'operator',
                updated_at = datetime('now')",
            params![agent_id, max_tokens_per_hour, max_tokens_per_day],
        )
        .map_err(|e| DbError::Integrity(format!("sqlite upsert quota: {e}")))?;
        Ok(())
    }

    async fn reset_usage(&self, agent_id: &str) -> Result<u64, DbError> {
        let conn = self.conn.lock().await;
        let n = conn
            .execute(
                "DELETE FROM token_usage WHERE agent_id = ?1",
                params![agent_id],
            )
            .map_err(|e| DbError::Integrity(format!("sqlite reset usage: {e}")))?;
        Ok(n as u64)
    }

    async fn check_budget(
        &self,
        agent_id: &str,
    ) -> Result<sera_types::metering::BudgetStatus, DbError> {
        let quota = self.get_quota(agent_id).await?;
        let (hourly_quota, daily_quota) = match &quota {
            Some(q) => (q.max_tokens_per_hour as i64, q.max_tokens_per_day as i64),
            None => (100_000i64, 1_000_000i64),
        };

        let hourly_used = self.get_usage_in_window(agent_id, 1).await?;
        let daily_used = self.get_usage_in_window(agent_id, 24).await?;

        let allowed = (hourly_quota == 0 || hourly_used < hourly_quota)
            && (daily_quota == 0 || daily_used < daily_quota);

        Ok(sera_types::metering::BudgetStatus {
            allowed,
            hourly_used: hourly_used as u64,
            hourly_quota: hourly_quota as u64,
            daily_used: daily_used as u64,
            daily_quota: daily_quota as u64,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn new_store() -> SqliteMeteringStore {
        let conn = Connection::open_in_memory().unwrap();
        SqliteMeteringStore::init_schema(&conn).unwrap();
        SqliteMeteringStore::new(Arc::new(Mutex::new(conn)))
    }

    #[tokio::test]
    async fn record_and_window_sum() {
        let store = new_store();
        store
            .record_usage(RecordUsageInput {
                agent_id: "a1",
                circle_id: None,
                model: "m",
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
                cost_usd: Some(0.01),
                latency_ms: Some(100),
                status: "ok",
            })
            .await
            .unwrap();
        store
            .record_usage(RecordUsageInput {
                agent_id: "a1",
                circle_id: None,
                model: "m",
                prompt_tokens: 5,
                completion_tokens: 5,
                total_tokens: 10,
                cost_usd: None,
                latency_ms: None,
                status: "ok",
            })
            .await
            .unwrap();
        let hourly = store.get_usage_in_window("a1", 1).await.unwrap();
        assert_eq!(hourly, 40);
    }

    #[tokio::test]
    async fn upsert_and_get_quota_roundtrip() {
        let store = new_store();
        store.upsert_quota("a1", Some(500), Some(5000)).await.unwrap();
        let q = store.get_quota("a1").await.unwrap().unwrap();
        assert_eq!(q.agent_id, "a1");
        assert_eq!(q.max_tokens_per_hour, 500);
        assert_eq!(q.max_tokens_per_day, 5000);
    }

    #[tokio::test]
    async fn upsert_preserves_with_none() {
        let store = new_store();
        store.upsert_quota("a1", Some(100), Some(1000)).await.unwrap();
        // Update only the daily quota.
        store.upsert_quota("a1", None, Some(2000)).await.unwrap();
        let q = store.get_quota("a1").await.unwrap().unwrap();
        assert_eq!(q.max_tokens_per_hour, 100);
        assert_eq!(q.max_tokens_per_day, 2000);
    }

    #[tokio::test]
    async fn tenant_isolation_across_agents() {
        let store = new_store();
        for a in ["tenant-a", "tenant-b"] {
            store
                .record_usage(RecordUsageInput {
                    agent_id: a,
                    circle_id: None,
                    model: "m",
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 50,
                    cost_usd: None,
                    latency_ms: None,
                    status: "ok",
                })
                .await
                .unwrap();
        }
        assert_eq!(store.get_usage_in_window("tenant-a", 24).await.unwrap(), 50);
        assert_eq!(store.get_usage_in_window("tenant-b", 24).await.unwrap(), 50);
        assert_eq!(store.get_usage_in_window("nobody", 24).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn reset_usage_removes_rows() {
        let store = new_store();
        store
            .record_usage(RecordUsageInput {
                agent_id: "a1",
                circle_id: None,
                model: "m",
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 100,
                cost_usd: None,
                latency_ms: None,
                status: "ok",
            })
            .await
            .unwrap();
        let n = store.reset_usage("a1").await.unwrap();
        assert_eq!(n, 1);
        assert_eq!(store.get_usage_in_window("a1", 24).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn check_budget_defaults() {
        let store = new_store();
        let status = store.check_budget("new-agent").await.unwrap();
        assert!(status.allowed);
        assert_eq!(status.hourly_quota, 100_000);
        assert_eq!(status.daily_quota, 1_000_000);
    }

    #[tokio::test]
    async fn check_budget_blocks_over_quota() {
        let store = new_store();
        store.upsert_quota("a1", Some(50), Some(500)).await.unwrap();
        store
            .record_usage(RecordUsageInput {
                agent_id: "a1",
                circle_id: None,
                model: "m",
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 100, // already over hourly quota of 50
                cost_usd: None,
                latency_ms: None,
                status: "ok",
            })
            .await
            .unwrap();
        let status = store.check_budget("a1").await.unwrap();
        assert!(!status.allowed);
    }
}
