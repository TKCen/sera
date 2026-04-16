//! Persistence module for session transcripts.
//!
//! Provides serialization and storage capabilities for [`Transcript`] objects.
//! This module handles the conversion between in-memory transcripts and
//! their persistent storage representations.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::transcript::{Transcript, TranscriptEntry};

/// Errors that can occur during transcript persistence operations.
#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("transcript not found: {0}")]
    NotFound(String),
}

/// A persisted transcript stored as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedTranscript {
    /// Version for forward compatibility.
    pub version: String,
    /// Session identifier.
    pub session_id: String,
    /// All transcript entries.
    pub entries: Vec<TranscriptEntry>,
    /// Metadata about the transcript.
    pub metadata: TranscriptMetadata,
}

/// Metadata about a persisted transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMetadata {
    /// When the transcript was first created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When the transcript was last updated.
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Total number of entries.
    pub entry_count: usize,
    /// Estimated token count.
    pub estimated_tokens: u32,
}

impl PersistedTranscript {
    /// Create a persisted transcript from a session ID and in-memory transcript.
    pub fn from_transcript(session_id: &str, transcript: &Transcript) -> Self {
        let now = chrono::Utc::now();
        let entry_count = transcript.len();
        let estimated_tokens = entry_count as u32 * 256; // Rough estimate

        Self {
            version: "1.0.0".to_string(),
            session_id: session_id.to_string(),
            entries: transcript.entries().to_vec(),
            metadata: TranscriptMetadata {
                created_at: now,
                updated_at: now,
                entry_count,
                estimated_tokens,
            },
        }
    }

    /// Convert back to an in-memory transcript.
    pub fn into_transcript(self) -> Transcript {
        let mut transcript = Transcript::new();
        for entry in self.entries {
            transcript.append(entry);
        }
        transcript
    }

    /// Load a persisted transcript from a JSON file.
    pub fn load_from_file(path: &PathBuf) -> Result<Self, PersistenceError> {
        if !path.exists() {
            return Err(PersistenceError::NotFound(
                path.display().to_string(),
            ));
        }
        let content = std::fs::read_to_string(path)?;
        let persisted: PersistedTranscript = serde_json::from_str(&content)?;
        Ok(persisted)
    }

    /// Save a persisted transcript to a JSON file.
    pub fn save_to_file(&self, path: &PathBuf) -> Result<(), PersistenceError> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

/// A session manager that handles transcript persistence.
///
/// Provides a higher-level interface for managing session transcripts
/// with automatic persistence on updates.
pub struct SessionManager {
    /// Base directory for storing transcripts.
    storage_path: PathBuf,
}

impl SessionManager {
    /// Create a new session manager with the given storage path.
    pub fn new(storage_path: PathBuf) -> Self {
        Self { storage_path }
    }

    /// Get the path for a session's transcript file.
    fn transcript_path(&self, session_id: &str) -> PathBuf {
        self.storage_path.join(format!("{}.json", session_id))
    }

    /// Load a transcript for a session.
    pub fn load_transcript(
        &self,
        session_id: &str,
    ) -> Result<Option<Transcript>, PersistenceError> {
        let path = self.transcript_path(session_id);
        if !path.exists() {
            return Ok(None);
        }
        let persisted = PersistedTranscript::load_from_file(&path)?;
        Ok(Some(persisted.into_transcript()))
    }

    /// Save a transcript for a session.
    pub fn save_transcript(
        &self,
        session_id: &str,
        transcript: &Transcript,
    ) -> Result<(), PersistenceError> {
        // Ensure the storage directory exists
        if !self.storage_path.exists() {
            std::fs::create_dir_all(&self.storage_path)?;
        }

        let path = self.transcript_path(session_id);
        let persisted = PersistedTranscript::from_transcript(session_id, transcript);
        persisted.save_to_file(&path)?;
        Ok(())
    }

    /// Delete a session's transcript.
    pub fn delete_transcript(&self, session_id: &str) -> Result<(), PersistenceError> {
        let path = self.transcript_path(session_id);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// List all session IDs that have stored transcripts.
    pub fn list_sessions(&self) -> Result<Vec<String>, PersistenceError> {
        if !self.storage_path.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        for entry in std::fs::read_dir(&self.storage_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file()
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                sessions.push(stem.to_string());
            }
        }
        sessions.sort();
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{ContentBlock, Role};
    use uuid::Uuid;

    fn make_entry(role: Role, text: &str) -> TranscriptEntry {
        TranscriptEntry {
            id: Uuid::new_v4(),
            role,
            blocks: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            timestamp: chrono::Utc::now(),
            cause_by: None,
        }
    }

    #[test]
    fn persisted_transcript_roundtrip() {
        let mut transcript = Transcript::new();
        transcript.append(make_entry(Role::User, "Hello"));
        transcript.append(make_entry(Role::Assistant, "Hi there!"));

        let persisted = PersistedTranscript::from_transcript("session-123", &transcript);
        assert_eq!(persisted.session_id, "session-123");
        assert_eq!(persisted.entries.len(), 2);
        assert_eq!(persisted.metadata.entry_count, 2);

        let back = persisted.into_transcript();
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn transcript_serde_roundtrip() {
        let mut transcript = Transcript::new();
        transcript.append(make_entry(Role::User, "Test message"));
        transcript.append(make_entry(Role::Tool, "Tool result"));

        let json = serde_json::to_string(&transcript).unwrap();
        let back: Transcript = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 2);
    }
}
