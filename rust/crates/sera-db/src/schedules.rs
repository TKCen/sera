//! Schedules repository — read access to the schedules table.

use sqlx::PgPool;
use crate::error::DbError;

/// Row type for schedules table (with agent name resolved via JOIN).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ScheduleRow {
    pub id: uuid::Uuid,
    pub agent_id: Option<uuid::Uuid>,
    pub agent_instance_id: Option<uuid::Uuid>,
    pub agent_name: Option<String>,
    pub name: String,
    pub cron: Option<String>,
    pub expression: Option<String>,
    pub r#type: Option<String>,
    pub task: serde_json::Value,
    pub source: String,
    pub status: Option<String>,
    pub last_run_at: Option<time::OffsetDateTime>,
    pub last_run_status: Option<String>,
    pub next_run_at: Option<time::OffsetDateTime>,
    pub category: Option<String>,
    pub description: Option<String>,
    pub created_at: Option<time::OffsetDateTime>,
    pub updated_at: Option<time::OffsetDateTime>,
}

pub struct ScheduleRepository;

impl ScheduleRepository {
    /// List all schedules with agent name resolved via JOIN.
    pub async fn list_schedules(pool: &PgPool) -> Result<Vec<ScheduleRow>, DbError> {
        let rows = sqlx::query_as::<_, ScheduleRow>(
            "SELECT s.id, s.agent_id, s.agent_instance_id,
                    COALESCE(s.agent_name, ai.name) as agent_name,
                    s.name, s.cron, s.expression, s.type, s.task, s.source, s.status,
                    s.last_run_at, s.last_run_status, s.next_run_at,
                    s.category, s.description, s.created_at, s.updated_at
             FROM schedules s
             LEFT JOIN agent_instances ai ON ai.id = s.agent_instance_id
             ORDER BY s.created_at DESC"
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Create a new schedule.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_schedule(
        pool: &PgPool,
        id: &str,
        agent_instance_id: Option<&str>,
        agent_name: &str,
        name: &str,
        schedule_type: &str,
        expression: &str,
        task: &serde_json::Value,
        source: &str,
        status: &str,
        category: Option<&str>,
        description: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO schedules (id, agent_instance_id, agent_name, name, type, expression, task,
                                    source, status, category, description, created_at, updated_at)
             VALUES ($1::uuid, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10, $11, NOW(), NOW())"
        )
        .bind(id)
        .bind(agent_instance_id)
        .bind(agent_name)
        .bind(name)
        .bind(schedule_type)
        .bind(expression)
        .bind(task)
        .bind(source)
        .bind(status)
        .bind(category)
        .bind(description)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update a schedule's mutable fields. Returns number of rows affected.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_schedule(
        pool: &PgPool,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
        expression: Option<&str>,
        task: Option<&serde_json::Value>,
        status: Option<&str>,
        category: Option<&str>,
    ) -> Result<u64, DbError> {
        let mut sets = vec!["updated_at = NOW()".to_string()];
        let mut param_idx = 1;
        let mut str_params: Vec<Option<String>> = Vec::new();
        let mut task_val: Option<serde_json::Value> = None;

        if let Some(v) = name {
            param_idx += 1;
            sets.push(format!("name = ${param_idx}"));
            str_params.push(Some(v.to_string()));
        }
        if let Some(v) = description {
            param_idx += 1;
            sets.push(format!("description = ${param_idx}"));
            str_params.push(Some(v.to_string()));
        }
        if let Some(v) = expression {
            param_idx += 1;
            sets.push(format!("expression = ${param_idx}"));
            str_params.push(Some(v.to_string()));
        }
        if let Some(v) = task {
            param_idx += 1;
            sets.push(format!("task = ${param_idx}"));
            task_val = Some(v.clone());
            // placeholder in str_params for ordering
        }
        if let Some(v) = status {
            param_idx += 1;
            sets.push(format!("status = ${param_idx}"));
            str_params.push(Some(v.to_string()));
        }
        if let Some(v) = category {
            param_idx += 1;
            sets.push(format!("category = ${param_idx}"));
            str_params.push(Some(v.to_string()));
        }

        let query = format!(
            "UPDATE schedules SET {} WHERE id::text = $1",
            sets.join(", ")
        );

        // Build query with binds in order
        let mut q = sqlx::query(&query).bind(id);
        // Bind string params first (name, description, expression)
        let mut str_idx = 0;
        if name.is_some() { q = q.bind(&str_params[str_idx]); str_idx += 1; }
        if description.is_some() { q = q.bind(&str_params[str_idx]); str_idx += 1; }
        if expression.is_some() { q = q.bind(&str_params[str_idx]); str_idx += 1; }
        if task_val.is_some() { q = q.bind(&task_val); }
        if status.is_some() { q = q.bind(&str_params[str_idx]); str_idx += 1; }
        if category.is_some() { q = q.bind(&str_params[str_idx]); }

        let result = q.execute(pool).await?;
        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity: "schedule",
                key: "id",
                value: id.to_string(),
            });
        }
        Ok(result.rows_affected())
    }

    /// Get a schedule's source field (to check if manifest-sourced).
    pub async fn get_source(pool: &PgPool, id: &str) -> Result<String, DbError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT source FROM schedules WHERE id::text = $1"
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some((source,)) => Ok(source),
            None => Err(DbError::NotFound {
                entity: "schedule",
                key: "id",
                value: id.to_string(),
            }),
        }
    }

    /// Delete a schedule. Returns error if not found.
    pub async fn delete_schedule(pool: &PgPool, id: &str) -> Result<(), DbError> {
        let result = sqlx::query(
            "DELETE FROM schedules WHERE id::text = $1"
        )
        .bind(id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity: "schedule",
                key: "id",
                value: id.to_string(),
            });
        }
        Ok(())
    }
}
