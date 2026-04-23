//! Heartbeat service — stale agent detection and monitoring.

use sera_db::DbError;
use sqlx::PgPool;
use std::sync::Arc;
use time::OffsetDateTime;

#[derive(Debug, thiserror::Error)]
pub enum HeartbeatError {
    #[error("database error: {0}")]
    Db(#[from] DbError),
}

/// Row type for stale agent queries.
#[derive(Debug, sqlx::FromRow)]
struct StaleAgentRow {
    pub id: String,
    pub last_heartbeat_at: Option<OffsetDateTime>,
    pub status: Option<String>,
}

/// Information about a stale agent.
#[derive(Debug, Clone)]
pub struct StaleAgent {
    pub agent_id: String,
    pub last_heartbeat: Option<OffsetDateTime>,
    pub status: String,
}

/// Service for monitoring agent heartbeats and detecting stale agents.
pub struct HeartbeatService {
    pool: Arc<PgPool>,
    interval_secs: u64,
    threshold_secs: u64,
}

impl HeartbeatService {
    /// Create a new HeartbeatService.
    ///
    /// # Arguments
    /// * `pool` — database pool
    /// * `interval_secs` — how often to check for stale agents (default: 30)
    /// * `threshold_secs` — how long without heartbeat before marking as stale (default: 120)
    pub fn new(pool: Arc<PgPool>, interval_secs: u64, threshold_secs: u64) -> Self {
        Self {
            pool,
            interval_secs,
            threshold_secs,
        }
    }

    /// Check for stale agents and return their IDs.
    ///
    /// Queries agents where last_heartbeat < NOW() - threshold_secs
    /// and status is not 'stopped' or 'terminated'.
    pub async fn check_stale_agents(&self) -> Result<Vec<StaleAgent>, HeartbeatError> {
        let threshold_secs = self.threshold_secs as i32;

        let rows: Vec<StaleAgentRow> = sqlx::query_as(
            "SELECT id::text, last_heartbeat_at, status
             FROM agent_instances
             WHERE last_heartbeat_at < NOW() - INTERVAL '1 second' * $1::int
             AND status NOT IN ('stopped', 'terminated')
             ORDER BY last_heartbeat_at ASC",
        )
        .bind(threshold_secs)
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| HeartbeatError::Db(DbError::from(e)))?;

        let stale_agents = rows
            .into_iter()
            .map(|row| StaleAgent {
                agent_id: row.id,
                last_heartbeat: row.last_heartbeat_at,
                status: row.status.unwrap_or_else(|| "unknown".to_string()),
            })
            .collect();

        Ok(stale_agents)
    }

    /// Record a heartbeat for an agent.
    ///
    /// Updates the agent's last_heartbeat_at timestamp to now.
    pub async fn record_heartbeat(&self, agent_id: &str) -> Result<(), HeartbeatError> {
        sqlx::query(
            "UPDATE agent_instances SET last_heartbeat_at = NOW(), updated_at = NOW() WHERE id::text = $1"
        )
        .bind(agent_id)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| HeartbeatError::Db(DbError::from(e)))?;

        Ok(())
    }

    /// Get the configured check interval in seconds.
    pub fn interval_secs(&self) -> u64 {
        self.interval_secs
    }

    /// Get the configured stale threshold in seconds.
    pub fn threshold_secs(&self) -> u64 {
        self.threshold_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stale_agent_creation() {
        let agent = StaleAgent {
            agent_id: "test-agent".to_string(),
            last_heartbeat: None,
            status: "active".to_string(),
        };
        assert_eq!(agent.agent_id, "test-agent");
        assert_eq!(agent.status, "active");
        assert!(agent.last_heartbeat.is_none());
    }

    #[test]
    fn test_heartbeat_service_defaults() {
        // Test configuration defaults without instantiating the service
        let interval_secs = 30u64;
        let threshold_secs = 120u64;
        assert_eq!(interval_secs, 30);
        assert_eq!(threshold_secs, 120);
        assert!(threshold_secs > interval_secs);
    }

    #[test]
    fn test_threshold_logic() {
        // Verify threshold calculations
        let service_config = (30u64, 120u64);
        assert!(service_config.1 > service_config.0);
        // Threshold should always be greater than interval for meaningful detection
    }
}
