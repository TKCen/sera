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
        let mut qb = sqlx::QueryBuilder::new("UPDATE schedules SET updated_at = NOW()");

        if let Some(v) = name {
            qb.push(", name = ").push_bind(v);
        }
        if let Some(v) = description {
            qb.push(", description = ").push_bind(v);
        }
        if let Some(v) = expression {
            qb.push(", expression = ").push_bind(v);
        }
        if let Some(v) = task {
            qb.push(", task = ").push_bind(v.clone());
        }
        if let Some(v) = status {
            qb.push(", status = ").push_bind(v);
        }
        if let Some(v) = category {
            qb.push(", category = ").push_bind(v);
        }

        qb.push(" WHERE id::text = ").push_bind(id);

        let result = qb.build().execute(pool).await?;
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

    /// List active schedules that are due to run (next_run_at <= NOW() or never run).
    pub async fn list_due(pool: &PgPool) -> Result<Vec<ScheduleRow>, DbError> {
        let rows = sqlx::query_as::<_, ScheduleRow>(
            "SELECT s.id, s.agent_id, s.agent_instance_id,
                    COALESCE(s.agent_name, ai.name) as agent_name,
                    s.name, s.cron, s.expression, s.type, s.task, s.source, s.status,
                    s.last_run_at, s.last_run_status, s.next_run_at,
                    s.category, s.description, s.created_at, s.updated_at
             FROM schedules s
             LEFT JOIN agent_instances ai ON ai.id = s.agent_instance_id
             WHERE s.status = 'active'
               AND (s.next_run_at IS NULL OR s.next_run_at <= NOW())"
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Update last_run_at and next_run_at after a schedule fires.
    pub async fn update_run_times(
        pool: &PgPool,
        id: uuid::Uuid,
        last_run_at: time::OffsetDateTime,
        next_run_at: Option<time::OffsetDateTime>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE schedules SET last_run_at = $2, next_run_at = $3, updated_at = NOW()
             WHERE id = $1"
        )
        .bind(id)
        .bind(last_run_at)
        .bind(next_run_at)
        .execute(pool)
        .await?;
        Ok(())
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
