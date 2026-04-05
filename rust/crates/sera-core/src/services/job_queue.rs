//! Job queue service — orchestration for background job processing.

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tokio::task::JoinHandle;
use uuid::Uuid;
use time::OffsetDateTime;

use sera_db::{DbPool, job_queue::JobQueueRepository};

/// Job queue service for enqueueing and polling background jobs.
pub struct JobQueue {
    pool: DbPool,
    worker_name: String,
    poll_interval: Duration,
    max_batch_size: i32,
}

impl JobQueue {
    /// Create a new job queue service.
    ///
    /// # Arguments
    /// * `pool` — database pool
    /// * `worker_name` — identifier for this worker (e.g., "worker-1", "core-bg-processor")
    /// * `poll_interval` — how often to check for new jobs
    /// * `max_batch_size` — maximum jobs to process per poll cycle
    pub fn new(
        pool: DbPool,
        worker_name: impl Into<String>,
        poll_interval: Duration,
        max_batch_size: i32,
    ) -> Self {
        Self {
            pool,
            worker_name: worker_name.into(),
            poll_interval,
            max_batch_size,
        }
    }

    /// Enqueue a new job to be processed.
    ///
    /// # Arguments
    /// * `job_type` — type identifier for the job
    /// * `payload` — JSON payload for the job
    /// * `scheduled_at` — when the job should be processed
    ///
    /// # Returns
    /// The ID of the newly created job.
    pub async fn enqueue(
        &self,
        job_type: &str,
        payload: serde_json::Value,
        scheduled_at: OffsetDateTime,
    ) -> Result<Uuid, sera_db::DbError> {
        JobQueueRepository::enqueue(self.pool.inner(), job_type, payload, scheduled_at).await
    }

    /// Start the polling loop for background job processing.
    ///
    /// Spawns a tokio task that continuously polls for pending jobs and processes them.
    /// The handler function is called for each job. If it returns successfully, the job is marked
    /// as completed. If it returns an error, the job is marked as failed with the error message.
    ///
    /// # Arguments
    /// * `self_arc` — Arc-wrapped self for task spawning
    /// * `handler` — async function that processes a job and returns a result
    /// * `cancellation_token` — token to signal shutdown
    ///
    /// # Returns
    /// A JoinHandle that can be awaited or aborted.
    ///
    /// # Example
    /// ```ignore
    /// let queue = Arc::new(JobQueue::new(pool, "worker-1", Duration::from_secs(5), 10));
    /// let cancel_token = CancellationToken::new();
    /// let handle = JobQueue::start_polling(
    ///     queue.clone(),
    ///     |job| async move {
    ///         println!("Processing job: {}", job.id);
    ///         Ok(())
    ///     },
    ///     cancel_token.clone(),
    /// );
    /// // Later, trigger shutdown:
    /// cancel_token.cancel();
    /// handle.await.ok();
    /// ```
    pub fn start_polling<H, F>(
        self_arc: Arc<Self>,
        handler: H,
        cancellation_token: CancellationToken,
    ) -> JoinHandle<()>
    where
        H: Fn(sera_db::job_queue::JobRow) -> F + Send + Sync + 'static,
        F: std::future::Future<Output = Result<(), String>> + Send,
    {
        let handler = Arc::new(handler);

        tokio::spawn(async move {
            let mut backoff = Duration::from_millis(100);
            let max_backoff = Duration::from_secs(30);

            loop {
                tokio::select! {
                    _ = cancellation_token.cancelled() => {
                        tracing::info!("Job queue polling cancelled");
                        break;
                    }

                    _ = tokio::time::sleep(self_arc.poll_interval) => {
                        match JobQueueRepository::dequeue(
                            self_arc.pool.inner(),
                            &self_arc.worker_name,
                            self_arc.max_batch_size,
                        )
                        .await
                        {
                            Ok(jobs) => {
                                if jobs.is_empty() {
                                    // Reset backoff when we find jobs
                                    backoff = Duration::from_millis(100);
                                } else {
                                    // Process each job
                                    for job in jobs {
                                        let job_id = job.id;
                                        let job_type = job.job_type.clone();

                                        match handler(job).await {
                                            Ok(()) => {
                                                if let Err(e) = JobQueueRepository::mark_complete(
                                                    self_arc.pool.inner(),
                                                    job_id,
                                                )
                                                .await
                                                {
                                                    tracing::error!(
                                                        "Failed to mark job {job_id} as complete: {e}"
                                                    );
                                                }
                                            }
                                            Err(err) => {
                                                let error_msg = err.to_string();
                                                tracing::warn!(
                                                    "Job {job_id} ({job_type}) failed: {error_msg}"
                                                );
                                                if let Err(e) = JobQueueRepository::mark_failed(
                                                    self_arc.pool.inner(),
                                                    job_id,
                                                    &error_msg,
                                                )
                                                .await
                                                {
                                                    tracing::error!(
                                                        "Failed to mark job {job_id} as failed: {e}"
                                                    );
                                                }
                                            }
                                        }
                                    }

                                    // Reset backoff on success
                                    backoff = Duration::from_millis(100);
                                }
                            }
                            Err(e) => {
                                tracing::error!("Error dequeuing jobs: {e}");
                                // Exponential backoff on error
                                tokio::time::sleep(backoff).await;
                                backoff = backoff.saturating_mul(2).min(max_backoff);
                            }
                        }

                        // Periodically clean up stale jobs
                        if let Err(e) = JobQueueRepository::cleanup_stale(
                            self_arc.pool.inner(),
                            300, // 5 minutes
                        )
                        .await
                        {
                            tracing::error!("Error cleaning up stale jobs: {e}");
                        }
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_queue_creation() {
        // Note: Cannot create a real DbPool without a database.
        // This test verifies the API compiles correctly.
        // Full integration tests would require DATABASE_URL.

        // The JobQueue struct itself is testable through its public interface.
        // Integration tests should use a real PostgreSQL instance.
    }

    #[test]
    fn test_duration_calculations() {
        // Verify backoff calculations don't panic
        let mut backoff = Duration::from_millis(100);
        let max_backoff = Duration::from_secs(30);

        for _ in 0..20 {
            backoff = backoff.saturating_mul(2).min(max_backoff);
            assert!(backoff <= max_backoff);
        }
    }
}
