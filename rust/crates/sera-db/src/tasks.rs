//! Task queue repository — agent task enqueue, poll, complete.

use sqlx::PgPool;

use crate::error::DbError;

/// Row type for task_queue table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskRow {
    pub id: uuid::Uuid,
    pub agent_instance_id: uuid::Uuid,
    pub task: String,
    pub context: Option<serde_json::Value>,
    pub status: String,
    pub priority: i32,
    pub retry_count: i32,
    pub max_retries: i32,
    pub created_at: time::OffsetDateTime,
    pub started_at: Option<time::OffsetDateTime>,
    pub completed_at: Option<time::OffsetDateTime>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub exit_reason: Option<String>,
}

pub struct TaskRepository;

impl TaskRepository {
    /// Enqueue a new task for an agent.
    pub async fn enqueue(
        pool: &PgPool,
        agent_instance_id: &str,
        task: &str,
        context: Option<&serde_json::Value>,
        priority: Option<i32>,
    ) -> Result<TaskRow, DbError> {
        let id = uuid::Uuid::new_v4();
        let pri = priority.unwrap_or(100);

        sqlx::query(
            "INSERT INTO task_queue (id, agent_instance_id, task, context, status, priority)
             VALUES ($1, $2::uuid, $3, $4, 'queued', $5)",
        )
        .bind(id)
        .bind(agent_instance_id)
        .bind(task)
        .bind(context)
        .bind(pri)
        .execute(pool)
        .await?;

        Self::get_task(pool, &id.to_string()).await
    }

    /// Poll next queued task for an agent (SKIP LOCKED for concurrency).
    pub async fn poll_next(
        pool: &PgPool,
        agent_instance_id: &str,
    ) -> Result<Option<TaskRow>, DbError> {
        // Atomically claim the next task
        let row = sqlx::query_as::<_, TaskRow>(
            "UPDATE task_queue SET status = 'running', started_at = NOW()
             WHERE id = (
               SELECT id FROM task_queue
               WHERE agent_instance_id = $1::uuid AND status = 'queued'
               ORDER BY priority ASC, created_at ASC
               LIMIT 1
               FOR UPDATE SKIP LOCKED
             )
             RETURNING id, agent_instance_id, task, context, status, priority,
                       retry_count, max_retries, created_at, started_at, completed_at,
                       result, error, exit_reason",
        )
        .bind(agent_instance_id)
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// Submit task result.
    pub async fn submit_result(
        pool: &PgPool,
        task_id: &str,
        result: Option<&serde_json::Value>,
        error: Option<&str>,
        exit_reason: Option<&str>,
    ) -> Result<TaskRow, DbError> {
        let status = if error.is_some() { "failed" } else { "completed" };

        let affected = sqlx::query(
            "UPDATE task_queue SET status = $1, result = $2, error = $3, exit_reason = $4,
                    completed_at = NOW()
             WHERE id = $5::uuid",
        )
        .bind(status)
        .bind(result)
        .bind(error)
        .bind(exit_reason)
        .bind(task_id)
        .execute(pool)
        .await?;

        if affected.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity: "task",
                key: "id",
                value: task_id.to_string(),
            });
        }

        Self::get_task(pool, task_id).await
    }

    /// Get task history for an agent.
    pub async fn get_history(
        pool: &PgPool,
        agent_instance_id: &str,
        limit: i64,
    ) -> Result<Vec<TaskRow>, DbError> {
        let rows = sqlx::query_as::<_, TaskRow>(
            "SELECT id, agent_instance_id, task, context, status, priority,
                    retry_count, max_retries, created_at, started_at, completed_at,
                    result, error, exit_reason
             FROM task_queue WHERE agent_instance_id = $1::uuid
             ORDER BY created_at DESC LIMIT $2",
        )
        .bind(agent_instance_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Get a single task by ID.
    pub async fn get_task(pool: &PgPool, id: &str) -> Result<TaskRow, DbError> {
        sqlx::query_as::<_, TaskRow>(
            "SELECT id, agent_instance_id, task, context, status, priority,
                    retry_count, max_retries, created_at, started_at, completed_at,
                    result, error, exit_reason
             FROM task_queue WHERE id = $1::uuid",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DbError::NotFound {
            entity: "task",
            key: "id",
            value: id.to_string(),
        })
    }
}
