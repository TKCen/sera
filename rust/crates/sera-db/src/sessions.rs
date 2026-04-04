//! Sessions repository — read access to the chat_sessions table.

use sqlx::PgPool;
use crate::error::DbError;

/// Row type for chat_sessions table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionRow {
    pub id: uuid::Uuid,
    pub agent_name: String,
    pub agent_instance_id: Option<uuid::Uuid>,
    pub title: String,
    pub message_count: Option<i32>,
    pub created_at: Option<time::OffsetDateTime>,
    pub updated_at: Option<time::OffsetDateTime>,
}

pub struct SessionRepository;

impl SessionRepository {
    pub async fn list_sessions(
        pool: &PgPool,
        agent_name: Option<&str>,
    ) -> Result<Vec<SessionRow>, DbError> {
        let rows = if let Some(name) = agent_name {
            sqlx::query_as::<_, SessionRow>(
                "SELECT id, agent_name, agent_instance_id, title, message_count, created_at, updated_at
                 FROM chat_sessions WHERE agent_name = $1
                 ORDER BY updated_at DESC LIMIT 100"
            )
            .bind(name)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, SessionRow>(
                "SELECT id, agent_name, agent_instance_id, title, message_count, created_at, updated_at
                 FROM chat_sessions
                 ORDER BY updated_at DESC LIMIT 100"
            )
            .fetch_all(pool)
            .await?
        };
        Ok(rows)
    }
}
