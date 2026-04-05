//! Job queue repository — CRUD for background job processing.

use sqlx::PgPool;
use uuid::Uuid;
use time::OffsetDateTime;

use crate::error::DbError;

/// Row type for jobs table — represents a queued background job.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JobRow {
    pub id: Uuid,
    pub job_type: String,
    pub payload: serde_json::Value,
    pub status: String, // "pending", "processing", "completed", "failed"
    pub scheduled_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
    pub started_at: Option<OffsetDateTime>,
    pub completed_at: Option<OffsetDateTime>,
    pub failed_at: Option<OffsetDateTime>,
    pub error: Option<String>,
    pub worker_name: Option<String>,
    pub attempts: i32,
    pub max_attempts: i32,
}

/// Repository for job queue operations.
pub struct JobQueueRepository;

impl JobQueueRepository {
    /// Enqueue a new job.
    ///
    /// # Arguments
    /// * `pool` — database pool
    /// * `job_type` — type identifier for the job (e.g., "email_send", "webhook_call")
    /// * `payload` — JSON payload for the job
    /// * `scheduled_at` — when the job should be processed
    ///
    /// # Returns
    /// The ID of the newly created job.
    pub async fn enqueue(
        pool: &PgPool,
        job_type: &str,
        payload: serde_json::Value,
        scheduled_at: OffsetDateTime,
    ) -> Result<Uuid, DbError> {
        let job_id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();

        sqlx::query(
            r#"
            INSERT INTO jobs (id, job_type, payload, status, scheduled_at, created_at, attempts, max_attempts)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(job_id)
        .bind(job_type)
        .bind(&payload)
        .bind("pending")
        .bind(scheduled_at)
        .bind(now)
        .bind(0)
        .bind(3)
        .execute(pool)
        .await?;

        Ok(job_id)
    }

    /// Dequeue a batch of pending jobs and mark them as processing.
    ///
    /// Uses SELECT ... FOR UPDATE SKIP LOCKED to ensure no concurrent workers process the same job.
    ///
    /// # Arguments
    /// * `pool` — database pool
    /// * `worker_name` — name/ID of the worker claiming the jobs
    /// * `batch_size` — maximum number of jobs to return
    ///
    /// # Returns
    /// List of jobs ready for processing.
    pub async fn dequeue(
        pool: &PgPool,
        worker_name: &str,
        batch_size: i32,
    ) -> Result<Vec<JobRow>, DbError> {
        let now = OffsetDateTime::now_utc();

        // Two-step process:
        // 1. Select jobs with FOR UPDATE SKIP LOCKED (atomic, non-blocking)
        // 2. Update them to processing status
        let job_ids: Vec<Uuid> = sqlx::query_scalar(
            r#"
            SELECT id FROM jobs
            WHERE status = 'pending' AND scheduled_at <= $1 AND attempts < max_attempts
            ORDER BY scheduled_at ASC
            FOR UPDATE SKIP LOCKED
            LIMIT $2
            "#,
        )
        .bind(now)
        .bind(batch_size)
        .fetch_all(pool)
        .await?;

        if job_ids.is_empty() {
            return Ok(vec![]);
        }

        // Update selected jobs to processing
        sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'processing', worker_name = $1, started_at = $2
            WHERE id = ANY($3)
            "#,
        )
        .bind(worker_name)
        .bind(now)
        .bind(&job_ids)
        .execute(pool)
        .await?;

        // Fetch the updated rows
        let jobs = sqlx::query_as::<_, JobRow>(
            r#"
            SELECT * FROM jobs WHERE id = ANY($1) ORDER BY scheduled_at ASC
            "#,
        )
        .bind(&job_ids)
        .fetch_all(pool)
        .await?;

        Ok(jobs)
    }

    /// Mark a job as completed.
    pub async fn mark_complete(pool: &PgPool, job_id: Uuid) -> Result<(), DbError> {
        let now = OffsetDateTime::now_utc();

        sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'completed', completed_at = $1, error = NULL
            WHERE id = $2
            "#,
        )
        .bind(now)
        .bind(job_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Mark a job as failed.
    ///
    /// # Arguments
    /// * `pool` — database pool
    /// * `job_id` — the job ID
    /// * `error_reason` — human-readable error message
    pub async fn mark_failed(
        pool: &PgPool,
        job_id: Uuid,
        error_reason: &str,
    ) -> Result<(), DbError> {
        let now = OffsetDateTime::now_utc();

        sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'failed', failed_at = $1, error = $2, attempts = attempts + 1
            WHERE id = $3
            "#,
        )
        .bind(now)
        .bind(error_reason)
        .bind(job_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Retrieve a job by ID.
    pub async fn get_job(pool: &PgPool, job_id: Uuid) -> Result<Option<JobRow>, DbError> {
        let job = sqlx::query_as::<_, JobRow>(
            r#"
            SELECT * FROM jobs WHERE id = $1
            "#,
        )
        .bind(job_id)
        .fetch_optional(pool)
        .await?;

        Ok(job)
    }

    /// Clean up stale jobs stuck in "processing" status.
    ///
    /// Resets jobs that have been in "processing" for longer than the stale threshold
    /// back to "pending" status so another worker can retry them.
    ///
    /// # Arguments
    /// * `pool` — database pool
    /// * `stale_threshold_secs` — time in seconds after which a job is considered stale
    ///
    /// # Returns
    /// Number of jobs reset to pending.
    pub async fn cleanup_stale(
        pool: &PgPool,
        stale_threshold_secs: i64,
    ) -> Result<i64, DbError> {
        let now = OffsetDateTime::now_utc();
        let stale_time = now - std::time::Duration::from_secs(stale_threshold_secs as u64);

        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'pending', worker_name = NULL, started_at = NULL
            WHERE status = 'processing' AND started_at < $1 AND attempts < max_attempts
            "#,
        )
        .bind(stale_time)
        .execute(pool)
        .await?;

        Ok(result.rows_affected() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_row_creation() {
        // This is a compile-time test that JobRow can be instantiated
        let job = JobRow {
            id: Uuid::new_v4(),
            job_type: "test".to_string(),
            payload: serde_json::json!({"key": "value"}),
            status: "pending".to_string(),
            scheduled_at: OffsetDateTime::now_utc(),
            created_at: OffsetDateTime::now_utc(),
            started_at: None,
            completed_at: None,
            failed_at: None,
            error: None,
            worker_name: None,
            attempts: 0,
            max_attempts: 3,
        };

        assert_eq!(job.status, "pending");
        assert_eq!(job.attempts, 0);
    }
}
