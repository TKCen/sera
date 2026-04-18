//! Session persistence integration — connects Transcript to persistence layer.
//!
//! This module bridges the in-memory Transcript from `sera-session` with the
//! `SessionPersist` trait from `sera-gateway`.

use crate::session_persist::{PersistedSession, SessionPersist};
use sera_session::transcript::Transcript;
use std::sync::Arc;

/// Errors from session persistence operations.
#[derive(Debug, thiserror::Error)]
pub enum SessionPersistenceError {
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("persistence error: {0}")]
    PersistenceError(String),
}

/// In-memory transcript storage that delegates to a SessionPersist backend.
pub struct TranscriptPersistence {
    backend: Arc<dyn SessionPersist>,
}

impl TranscriptPersistence {
    /// Create a new TranscriptPersistence with the given backend.
    pub fn new(backend: Arc<dyn SessionPersist>) -> Self {
        Self { backend }
    }

    /// Save a transcript to persistent storage.
    pub async fn save_transcript(
        &self,
        session_key: &str,
        transcript: &Transcript,
    ) -> Result<(), SessionPersistenceError> {
        let data = serde_json::to_value(transcript)
            .map_err(|e| SessionPersistenceError::PersistenceError(e.to_string()))?;

        let session = PersistedSession {
            session_key: session_key.to_string(),
            data,
            saved_at: time::OffsetDateTime::now_utc(),
        };

        self.backend
            .save(&session)
            .await
            .map_err(|e| SessionPersistenceError::PersistenceError(e.to_string()))
    }

    /// Load a transcript from persistent storage.
    pub async fn load_transcript(
        &self,
        session_key: &str,
    ) -> Result<Option<Transcript>, SessionPersistenceError> {
        let session = self
            .backend
            .load(session_key)
            .await
            .map_err(|e| SessionPersistenceError::PersistenceError(e.to_string()))?;

        match session {
            Some(s) => {
                let transcript: Transcript = serde_json::from_value(s.data)
                    .map_err(|e| SessionPersistenceError::PersistenceError(e.to_string()))?;
                Ok(Some(transcript))
            }
            None => Ok(None),
        }
    }

    /// Delete a transcript from persistent storage.
    pub async fn delete_transcript(
        &self,
        session_key: &str,
    ) -> Result<bool, SessionPersistenceError> {
        self.backend
            .delete(session_key)
            .await
            .map_err(|e| SessionPersistenceError::PersistenceError(e.to_string()))
    }
}
