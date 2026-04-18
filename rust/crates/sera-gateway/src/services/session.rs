//! Session service — business logic for chat persistence.
//!
//! Wraps SessionRepository with domain logic for session and message management,
//! including token estimation and lifecycle operations.

use std::sync::Arc;

use sqlx::PgPool;
use thiserror::Error;

use sera_db::sessions::{MessageRow, SessionRepository, SessionRow};
use sera_db::DbError;

/// Session service error types.
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("database error: {0}")]
    Db(#[from] DbError),

    #[error("session not found: {0}")]
    NotFound(String),

    #[error("invalid session data: {0}")]
    InvalidData(String),
}

/// Session service — orchestrates session and message operations.
pub struct SessionService {
    pool: Arc<PgPool>,
}

impl SessionService {
    /// Create a new session service with a database pool.
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Create a new session for an agent.
    pub async fn create_session(
        &self,
        agent_id: &str,
        _circle_id: Option<&str>,
        _metadata: Option<serde_json::Value>,
    ) -> Result<SessionRow, SessionError> {
        let id = uuid::Uuid::new_v4().to_string();
        let session = SessionRepository::create(self.pool.as_ref(), &id, agent_id, None).await?;
        Ok(session)
    }

    /// Get a session by ID.
    pub async fn get_session(&self, id: &str) -> Result<SessionRow, SessionError> {
        SessionRepository::get_by_id(self.pool.as_ref(), id)
            .await
            .map_err(|e| match e {
                DbError::NotFound { entity, key, value } => {
                    SessionError::NotFound(format!("{} with {}={}", entity, key, value))
                }
                other => SessionError::Db(other),
            })
    }

    /// List sessions for an agent with pagination.
    pub async fn list_sessions(
        &self,
        agent_id: &str,
        _limit: i64,
        _offset: i64,
    ) -> Result<Vec<SessionRow>, SessionError> {
        SessionRepository::list_sessions(self.pool.as_ref(), Some(agent_id))
            .await
            .map_err(SessionError::Db)
    }

    /// Delete a session (cascades to messages).
    pub async fn delete_session(&self, id: &str) -> Result<bool, SessionError> {
        SessionRepository::delete(self.pool.as_ref(), id)
            .await
            .map_err(SessionError::Db)
    }

    /// Add a message to a session.
    pub async fn add_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        _model: Option<&str>,
    ) -> Result<MessageRow, SessionError> {
        // Verify session exists before adding message
        let _session = self.get_session(session_id).await?;

        // For now, we insert messages via raw SQL since the repository doesn't have
        // an add_message method yet. This will be replaced with a repository method.
        let id = uuid::Uuid::new_v4();
        let session_uuid = uuid::Uuid::parse_str(session_id)
            .map_err(|e| SessionError::InvalidData(format!("invalid session id: {}", e)))?;

        let message = sqlx::query_as::<_, MessageRow>(
            "INSERT INTO chat_messages (id, session_id, role, content, metadata)
             VALUES ($1, $2, $3, $4, NULL)
             RETURNING id, session_id, role, content, metadata, created_at"
        )
        .bind(id)
        .bind(session_uuid)
        .bind(role)
        .bind(content)
        .fetch_one(self.pool.as_ref())
        .await
        .map_err(|e| SessionError::Db(DbError::Sqlx(e)))?;

        Ok(message)
    }

    /// Get messages for a session, ordered by creation time descending.
    pub async fn get_messages(
        &self,
        session_id: &str,
        _limit: i64,
    ) -> Result<Vec<MessageRow>, SessionError> {
        let messages = SessionRepository::get_messages(self.pool.as_ref(), session_id).await?;
        // Return in descending order (reverse of ASC from repository)
        let mut messages = messages;
        messages.reverse();
        Ok(messages)
    }

    /// Estimate tokens in content using simple char/4 heuristic.
    pub fn estimate_tokens(content: &str) -> i64 {
        (content.len() as i64 + 3) / 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_estimation_basic() {
        assert_eq!(SessionService::estimate_tokens(""), 0);
        assert_eq!(SessionService::estimate_tokens("a"), 1);
        assert_eq!(SessionService::estimate_tokens("ab"), 1);
        assert_eq!(SessionService::estimate_tokens("abc"), 1);
        assert_eq!(SessionService::estimate_tokens("abcd"), 1);
        assert_eq!(SessionService::estimate_tokens("abcde"), 2);
    }

    #[test]
    fn token_estimation_realistic() {
        let short = "Hello";
        assert_eq!(SessionService::estimate_tokens(short), 2);

        let medium = "The quick brown fox jumps over the lazy dog";
        assert_eq!(
            SessionService::estimate_tokens(medium),
            (medium.len() as i64 + 3) / 4
        );

        let long = "a".repeat(1000);
        assert_eq!(SessionService::estimate_tokens(&long), 250);
    }

    #[test]
    fn token_estimation_rounding() {
        // 7 chars: (7+3)/4 = 2
        assert_eq!(SessionService::estimate_tokens("1234567"), 2);
        // 8 chars: (8+3)/4 = 2
        assert_eq!(SessionService::estimate_tokens("12345678"), 2);
        // 9 chars: (9+3)/4 = 3
        assert_eq!(SessionService::estimate_tokens("123456789"), 3);
    }
}
