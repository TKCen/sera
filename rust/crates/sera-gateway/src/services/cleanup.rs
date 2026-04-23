//! Cleanup service — container lifecycle and TTL enforcement for terminated agents.

use sera_db::error::DbError;
use sera_tools::sandbox::{SandboxHandle, SandboxProvider};
use sqlx::PgPool;
use std::sync::Arc;

/// Represents an agent eligible for cleanup.
#[derive(Debug, Clone)]
pub struct ExpiredAgent {
    pub id: String,
    pub name: String,
    pub container_id: Option<String>,
}

/// Cleanup operation summary.
#[derive(Debug, Clone)]
pub struct CleanupSummary {
    pub agents_cleaned: i64,
    pub keys_cleaned: i64,
    pub sessions_cleaned: i64,
}

/// Cleanup service errors.
#[derive(Debug, thiserror::Error)]
pub enum CleanupError {
    #[error("database error: {0}")]
    Db(#[from] DbError),
    #[error("sandbox error: {0}")]
    Sandbox(#[from] sera_tools::sandbox::SandboxError),
}

/// Service for cleaning up expired resources — terminated agents, expired keys/sessions.
pub struct CleanupService {
    pool: Arc<PgPool>,
    sandbox: Option<Arc<dyn SandboxProvider>>,
    terminated_retention_secs: u64,
}

impl CleanupService {
    /// Create a new CleanupService.
    ///
    /// `terminated_retention_secs` — how long to keep terminated agents before cleanup.
    /// Defaults to 3600 (1 hour) if not specified.
    pub fn new(
        pool: Arc<PgPool>,
        sandbox: Option<Arc<dyn SandboxProvider>>,
        terminated_retention_secs: Option<u64>,
    ) -> Self {
        Self {
            pool,
            sandbox,
            terminated_retention_secs: terminated_retention_secs.unwrap_or(3600),
        }
    }

    /// Find agents eligible for cleanup (status = 'terminated' and past retention window).
    pub async fn find_expired_agents(&self) -> Result<Vec<ExpiredAgent>, CleanupError> {
        let cutoff = format!("{}s", self.terminated_retention_secs);

        let rows = sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT id::text, name, container_id
             FROM agent_instances
             WHERE status = 'terminated'
             AND updated_at < NOW() - CAST($1 AS interval)
             ORDER BY updated_at ASC",
        )
        .bind(&cutoff)
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| CleanupError::Db(DbError::Sqlx(e)))?;

        let agents = rows
            .into_iter()
            .map(|(id, name, container_id)| ExpiredAgent {
                id,
                name,
                container_id,
            })
            .collect();

        Ok(agents)
    }

    /// Clean up expired API keys (expires_at < NOW()).
    pub async fn cleanup_expired_keys(&self) -> Result<i64, CleanupError> {
        let result = sqlx::query(
            "DELETE FROM api_keys
             WHERE expires_at IS NOT NULL AND expires_at < NOW()",
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| CleanupError::Db(DbError::Sqlx(e)))?;

        Ok(result.rows_affected() as i64)
    }

    /// Clean up expired sessions (expires_at IS NOT NULL AND expires_at < NOW()).
    pub async fn cleanup_expired_sessions(&self) -> Result<i64, CleanupError> {
        let result = sqlx::query(
            "DELETE FROM chat_sessions
             WHERE expires_at IS NOT NULL AND expires_at < NOW()",
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| CleanupError::Db(DbError::Sqlx(e)))?;

        Ok(result.rows_affected() as i64)
    }

    /// Run full cleanup cycle: remove expired containers, clean up keys and sessions.
    pub async fn run_cleanup(&self) -> Result<CleanupSummary, CleanupError> {
        let mut agents_cleaned: i64 = 0;

        // Clean up expired agents and their containers
        if let Some(sandbox) = &self.sandbox {
            let expired = self.find_expired_agents().await?;

            for agent in expired {
                if let Some(container_id) = &agent.container_id {
                    match sandbox.destroy(&SandboxHandle(container_id.clone())).await {
                        Ok(_) => {
                            agents_cleaned += 1;
                            tracing::info!(
                                agent_id = %agent.id,
                                agent_name = %agent.name,
                                container_id = %container_id,
                                "Cleaned up expired agent container"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                agent_id = %agent.id,
                                container_id = %container_id,
                                error = %e,
                                "Failed to stop container for expired agent"
                            );
                        }
                    }
                }

                // Mark agent as cleaned in DB
                if let Err(e) = sqlx::query("DELETE FROM agent_instances WHERE id::text = $1")
                    .bind(&agent.id)
                    .execute(self.pool.as_ref())
                    .await
                {
                    tracing::warn!(
                        agent_id = %agent.id,
                        error = %e,
                        "Failed to delete expired agent from database"
                    );
                }
            }
        }

        // Clean up expired keys and sessions
        let keys_cleaned = self.cleanup_expired_keys().await?;
        let sessions_cleaned = self.cleanup_expired_sessions().await?;

        tracing::info!(
            agents_cleaned,
            keys_cleaned,
            sessions_cleaned,
            "Cleanup cycle complete"
        );

        Ok(CleanupSummary {
            agents_cleaned,
            keys_cleaned,
            sessions_cleaned,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expired_agent_creation() {
        let agent = ExpiredAgent {
            id: "test-id".to_string(),
            name: "test-agent".to_string(),
            container_id: Some("abc123".to_string()),
        };

        assert_eq!(agent.id, "test-id");
        assert_eq!(agent.name, "test-agent");
        assert_eq!(agent.container_id, Some("abc123".to_string()));
    }

    #[test]
    fn test_cleanup_summary_creation() {
        let summary = CleanupSummary {
            agents_cleaned: 5,
            keys_cleaned: 10,
            sessions_cleaned: 20,
        };

        assert_eq!(summary.agents_cleaned, 5);
        assert_eq!(summary.keys_cleaned, 10);
        assert_eq!(summary.sessions_cleaned, 20);
    }
}
