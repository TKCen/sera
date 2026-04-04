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
    pub max_tokens_per_hour: i64,
    pub max_tokens_per_day: i64,
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

    /// Check budget for an agent. Returns (allowed, hourly_used, hourly_quota, daily_used, daily_quota).
    pub async fn check_budget(
        pool: &PgPool,
        agent_id: &str,
    ) -> Result<sera_domain::metering::BudgetStatus, DbError> {
        let quota = Self::get_quota(pool, agent_id).await?;

        let (hourly_quota, daily_quota) = match &quota {
            Some(q) => {
                // Quota of 0 means unlimited
                (q.max_tokens_per_hour, q.max_tokens_per_day)
            }
            None => (100_000, 1_000_000), // Defaults from MeteringService
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
