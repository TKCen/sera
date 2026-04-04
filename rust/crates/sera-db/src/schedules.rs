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
}
