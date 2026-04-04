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

/// Row type for chat_messages table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MessageRow {
    pub id: uuid::Uuid,
    pub session_id: uuid::Uuid,
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<serde_json::Value>,
    pub created_at: Option<time::OffsetDateTime>,
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

    /// Get a session by ID.
    pub async fn get_by_id(pool: &PgPool, id: &str) -> Result<SessionRow, DbError> {
        sqlx::query_as::<_, SessionRow>(
            "SELECT id, agent_name, agent_instance_id, title, message_count, created_at, updated_at
             FROM chat_sessions WHERE id = $1::uuid"
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DbError::NotFound {
            entity: "session",
            key: "id",
            value: id.to_string(),
        })
    }

    /// Get messages for a session.
    pub async fn get_messages(pool: &PgPool, session_id: &str) -> Result<Vec<MessageRow>, DbError> {
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT id, session_id, role, content, tool_calls, created_at
             FROM chat_messages WHERE session_id = $1::uuid
             ORDER BY created_at ASC"
        )
        .bind(session_id)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Create a new session.
    pub async fn create(
        pool: &PgPool,
        id: &str,
        agent_name: &str,
        title: Option<&str>,
    ) -> Result<SessionRow, DbError> {
        sqlx::query(
            "INSERT INTO chat_sessions (id, agent_name, title)
             VALUES ($1::uuid, $2, COALESCE($3, 'New Chat'))"
        )
        .bind(id)
        .bind(agent_name)
        .bind(title)
        .execute(pool)
        .await?;

        Self::get_by_id(pool, id).await
    }

    /// Update session title.
    pub async fn update_title(pool: &PgPool, id: &str, title: &str) -> Result<SessionRow, DbError> {
        let result = sqlx::query(
            "UPDATE chat_sessions SET title = $1, updated_at = NOW() WHERE id = $2::uuid"
        )
        .bind(title)
        .bind(id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity: "session",
                key: "id",
                value: id.to_string(),
            });
        }

        Self::get_by_id(pool, id).await
    }

    /// Delete a session (messages cascade).
    pub async fn delete(pool: &PgPool, id: &str) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM chat_sessions WHERE id = $1::uuid")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
