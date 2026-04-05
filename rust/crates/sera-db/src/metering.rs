//! Metering repository — token usage tracking and budget enforcement.

use sqlx::PgPool;

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

/// Metering repository for database operations.
pub struct MeteringRepository;

impl MeteringRepository {
    /// Record a token usage event.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_usage(
        pool: &PgPool,
        agent_id: &str,
        circle_id: Option<&str>,
        model: &str,
        prompt_tokens: i64,
        completion_tokens: i64,
        total_tokens: i64,
        cost_usd: Option<f64>,
        latency_ms: Option<i64>,
        status: &str,
    ) -> Result<(), DbError> {
        // Insert into token_usage
        sqlx::query(
            "INSERT INTO token_usage (agent_id, circle_id, model, prompt_tokens, completion_tokens, total_tokens, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, NOW())"
        )
        .bind(agent_id)
        .bind(circle_id)
        .bind(model)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(total_tokens)
        .execute(pool)
        .await?;

        // Insert into usage_events (detailed record)
        sqlx::query(
            "INSERT INTO usage_events (agent_id, model, prompt_tokens, completion_tokens, total_tokens, cost_usd, latency_ms, status, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())"
        )
        .bind(agent_id)
        .bind(model)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(total_tokens)
        .bind(cost_usd)
        .bind(latency_ms)
        .bind(status)
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
    ) -> Result<sera_domain::metering::BudgetStatus, DbError> {
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

        Ok(sera_domain::metering::BudgetStatus {
            allowed,
            hourly_used: hourly_used as u64,
            hourly_quota: hourly_quota as u64,
            daily_used: daily_used as u64,
            daily_quota: daily_quota as u64,
        })
    }
}
