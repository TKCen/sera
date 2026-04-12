//! PostgreSQL-backed QueueBackend using sqlx.
//!
//! Jobs are stored in a `sera_queue_jobs` table with lane-based FIFO ordering.
//! Pulled jobs move to `processing` status; ack deletes them; nack resets to `pending`.

use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

use crate::backend::{QueueBackend, QueueError};

pub struct SqlxQueueBackend {
    pool: Arc<PgPool>,
}

impl SqlxQueueBackend {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl QueueBackend for SqlxQueueBackend {
    async fn push(&self, lane: &str, payload: serde_json::Value) -> Result<String, QueueError> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT INTO sera_queue_jobs (id, lane, payload, status, created_at, updated_at)
            VALUES ($1, $2, $3, 'pending', now(), now())
            "#,
        )
        .bind(&id)
        .bind(lane)
        .bind(&payload)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| QueueError::Storage {
            reason: e.to_string(),
        })?;

        Ok(id)
    }

    async fn pull(&self, lane: &str) -> Result<Option<(String, serde_json::Value)>, QueueError> {
        let mut tx = self.pool.begin().await.map_err(|e| QueueError::Storage {
            reason: e.to_string(),
        })?;

        let row: Option<(String, serde_json::Value)> = sqlx::query_as(
            r#"
            SELECT id, payload
            FROM sera_queue_jobs
            WHERE lane = $1 AND status = 'pending'
            ORDER BY created_at ASC
            LIMIT 1
            FOR UPDATE SKIP LOCKED
            "#,
        )
        .bind(lane)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| QueueError::Storage {
            reason: e.to_string(),
        })?;

        match row {
            None => {
                tx.commit().await.map_err(|e| QueueError::Storage {
                    reason: e.to_string(),
                })?;
                Ok(None)
            }
            Some((id, payload)) => {
                sqlx::query(
                    r#"
                    UPDATE sera_queue_jobs
                    SET status = 'processing', updated_at = now()
                    WHERE id = $1
                    "#,
                )
                .bind(&id)
                .execute(&mut *tx)
                .await
                .map_err(|e| QueueError::Storage {
                    reason: e.to_string(),
                })?;

                tx.commit().await.map_err(|e| QueueError::Storage {
                    reason: e.to_string(),
                })?;

                Ok(Some((id, payload)))
            }
        }
    }

    async fn ack(&self, job_id: &str) -> Result<(), QueueError> {
        sqlx::query("DELETE FROM sera_queue_jobs WHERE id = $1")
            .bind(job_id)
            .execute(self.pool.as_ref())
            .await
            .map_err(|e| QueueError::Storage {
                reason: e.to_string(),
            })?;

        Ok(())
    }

    async fn nack(&self, job_id: &str) -> Result<(), QueueError> {
        sqlx::query(
            r#"
            UPDATE sera_queue_jobs
            SET status = 'pending', updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(job_id)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| QueueError::Storage {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn recover_orphans(
        &self,
        stale_threshold: std::time::Duration,
    ) -> Result<usize, QueueError> {
        let threshold_secs = stale_threshold.as_secs_f64();
        let result = sqlx::query(
            r#"
            UPDATE sera_queue_jobs
            SET status = 'pending', updated_at = now()
            WHERE status = 'processing'
              AND updated_at < now() - ($1 || ' seconds')::interval
            "#,
        )
        .bind(threshold_secs)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| QueueError::Storage {
            reason: e.to_string(),
        })?;

        Ok(result.rows_affected() as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests require a live PostgreSQL instance with the sera_queue_jobs table.
    // Run with: DATABASE_URL=postgres://... cargo test -p sera-queue --features apalis
    //
    // The table schema expected:
    //   CREATE TABLE sera_queue_jobs (
    //     id         TEXT PRIMARY KEY,
    //     lane       TEXT NOT NULL,
    //     payload    JSONB NOT NULL,
    //     status     TEXT NOT NULL DEFAULT 'pending',
    //     created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    //     updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    //   );

    #[test]
    fn sqlx_backend_is_send_sync() {
        // Compile-time check: SqlxQueueBackend must be Send + Sync to satisfy QueueBackend.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SqlxQueueBackend>();
    }
}
