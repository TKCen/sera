//! Metering service — token usage tracking and budget enforcement.
//!
//! Wraps MeteringRepository with domain logic for budget checking and cost calculation.

use std::sync::Arc;

use sqlx::PgPool;
use thiserror::Error;

use sera_db::metering::MeteringRepository;
use sera_db::DbError;
use sera_domain::metering::BudgetStatus;

/// Metering service error types.
#[derive(Debug, Error)]
pub enum MeteringError {
    #[error("database error: {0}")]
    Db(#[from] DbError),

    #[error("budget exceeded: hourly usage {hourly_used} exceeds limit {hourly_limit}")]
    OverBudgetHourly { hourly_used: i64, hourly_limit: i64 },

    #[error("budget exceeded: daily usage {daily_used} exceeds limit {daily_limit}")]
    OverBudgetDaily { daily_used: i64, daily_limit: i64 },

    #[error("invalid data: {0}")]
    InvalidData(String),
}

/// Usage summary for dashboard display.
#[derive(Debug, Clone)]
pub struct UsageSummary {
    pub total_input: i64,
    pub total_output: i64,
    pub total_cost_usd: f64,
    pub period: String,
}

/// Metering service — orchestrates token usage tracking and budget enforcement.
pub struct MeteringService {
    pool: Arc<PgPool>,
}

impl MeteringService {
    /// Create a new metering service with a database pool.
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Record token usage for an agent.
    pub async fn record_usage(
        &self,
        agent_id: &str,
        input_tokens: i64,
        output_tokens: i64,
        model: &str,
    ) -> Result<(), MeteringError> {
        // Calculate total tokens and approximate cost
        let total_tokens = input_tokens + output_tokens;
        let cost_usd = Self::estimate_cost(model, input_tokens, output_tokens);

        MeteringRepository::record_usage(
            self.pool.as_ref(),
            agent_id,
            None,
            model,
            input_tokens,
            output_tokens,
            total_tokens,
            Some(cost_usd),
            None,
            "success",
        )
        .await
        .map_err(MeteringError::Db)
    }

    /// Check budget status for an agent — returns allowed flag and current usage.
    pub async fn check_budget(&self, agent_id: &str) -> Result<BudgetStatus, MeteringError> {
        MeteringRepository::check_budget(self.pool.as_ref(), agent_id)
            .await
            .map_err(MeteringError::Db)
    }

    /// Enforce budget — returns error if agent exceeds hourly or daily limit.
    pub async fn enforce_budget(&self, agent_id: &str) -> Result<(), MeteringError> {
        let status = self.check_budget(agent_id).await?;

        if !status.allowed {
            // Determine which limit was exceeded
            if status.hourly_quota > 0 && status.hourly_used >= status.hourly_quota {
                return Err(MeteringError::OverBudgetHourly {
                    hourly_used: status.hourly_used as i64,
                    hourly_limit: status.hourly_quota as i64,
                });
            }
            if status.daily_quota > 0 && status.daily_used >= status.daily_quota {
                return Err(MeteringError::OverBudgetDaily {
                    daily_used: status.daily_used as i64,
                    daily_limit: status.daily_quota as i64,
                });
            }
        }

        Ok(())
    }

    /// Get usage summary for a given period (e.g., "today", "this_week").
    pub async fn get_usage_summary(
        &self,
        agent_id: &str,
        period: &str,
    ) -> Result<UsageSummary, MeteringError> {
        // Query usage for the specified period
        let window_hours = match period {
            "today" => 24,
            "this_week" => 7 * 24,
            "this_month" => 30 * 24,
            _ => 24, // default to today
        };

        let total_tokens =
            MeteringRepository::get_usage_in_window(self.pool.as_ref(), agent_id, window_hours)
                .await
                .map_err(MeteringError::Db)?;

        // For now, estimate cost as ~$0.0015 per 1000 tokens (approximate GPT-4 rate)
        let total_cost_usd = (total_tokens as f64 / 1000.0) * 0.0015;

        Ok(UsageSummary {
            total_input: 0, // Not tracked separately in current schema
            total_output: 0,
            total_cost_usd,
            period: period.to_string(),
        })
    }

    /// Estimate cost in USD for a given model and token counts.
    /// Returns approximate cost based on common model pricing.
    fn estimate_cost(model: &str, input_tokens: i64, output_tokens: i64) -> f64 {
        // Approximate pricing (input, output) per 1M tokens in USD
        let (input_rate, output_rate) = match model {
            m if m.contains("gpt-4") => (0.03, 0.06),         // GPT-4: $30/$60 per 1M
            m if m.contains("gpt-3.5") => (0.0015, 0.002),    // GPT-3.5: $1.50/$2 per 1M
            m if m.contains("claude-3-opus") => (0.015, 0.075), // Claude 3 Opus
            m if m.contains("claude-3-sonnet") => (0.003, 0.015), // Claude 3 Sonnet
            m if m.contains("claude-3-haiku") => (0.00025, 0.00125), // Claude 3 Haiku
            _ => (0.001, 0.002), // Default estimate
        };

        let input_cost = (input_tokens as f64 / 1_000_000.0) * input_rate;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * output_rate;
        input_cost + output_cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_estimation_gpt4() {
        // 1000 input, 500 output tokens
        let cost = MeteringService::estimate_cost("gpt-4", 1000, 500);
        let expected = (1000.0 / 1_000_000.0) * 0.03 + (500.0 / 1_000_000.0) * 0.06;
        assert!((cost - expected).abs() < 0.0000001);
    }

    #[test]
    fn cost_estimation_gpt35() {
        let cost = MeteringService::estimate_cost("gpt-3.5-turbo", 5000, 2000);
        let expected = (5000.0 / 1_000_000.0) * 0.0015 + (2000.0 / 1_000_000.0) * 0.002;
        assert!((cost - expected).abs() < 0.000001);
    }

    #[test]
    fn cost_estimation_claude_opus() {
        let cost = MeteringService::estimate_cost("claude-3-opus", 10000, 5000);
        let expected = (10000.0 / 1_000_000.0) * 0.015 + (5000.0 / 1_000_000.0) * 0.075;
        assert!((cost - expected).abs() < 0.000001);
    }

    #[test]
    fn cost_estimation_default() {
        let cost = MeteringService::estimate_cost("unknown-model", 1000, 500);
        let expected = (1000.0 / 1_000_000.0) * 0.001 + (500.0 / 1_000_000.0) * 0.002;
        assert!((cost - expected).abs() < 0.000001);
    }

    #[test]
    fn cost_zero_tokens() {
        let cost = MeteringService::estimate_cost("gpt-4", 0, 0);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn budget_status_creation() {
        let status = BudgetStatus {
            allowed: true,
            hourly_used: 5000,
            hourly_quota: 100_000,
            daily_used: 50_000,
            daily_quota: 1_000_000,
        };
        assert!(status.allowed);
        assert_eq!(status.hourly_used, 5000);
        assert_eq!(status.daily_quota, 1_000_000);
    }
}
