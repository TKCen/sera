//! Persistence module for session transcripts.
//!
//! Provides serialization and storage capabilities for [`Transcript`] objects.
//! This module handles the conversion between in-memory transcripts and
//! their persistent storage representations.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::indexer::TranscriptIndexer;
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
/// with automatic persistence on updates. When a [`TranscriptIndexer`] is
/// wired via [`SessionManager::with_indexer`], [`close_session`] extracts a
/// compact summary of the transcript and pushes it into the indexer's
/// semantic memory backend.
pub struct SessionManager {
    /// Base directory for storing transcripts.
    storage_path: PathBuf,
    /// Optional transcript indexer invoked on [`close_session`]. Failures
    /// are logged at `warn` level; they MUST NOT block session close.
    indexer: Option<Arc<dyn TranscriptIndexer>>,
}

impl SessionManager {
    /// Create a new session manager with the given storage path.
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            storage_path,
            indexer: None,
        }
    }

    /// Attach a transcript indexer. The indexer is consulted by
    /// [`SessionManager::close_session`] before the transcript file is
    /// removed. Errors from the indexer are logged and swallowed so they
    /// cannot stall the close path.
    pub fn with_indexer(mut self, indexer: Arc<dyn TranscriptIndexer>) -> Self {
        self.indexer = Some(indexer);
        self
    }

    /// Whether this manager has a transcript indexer wired in.
    pub fn has_indexer(&self) -> bool {
        self.indexer.is_some()
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

    /// Close a session: index the transcript (best-effort) and persist a
    /// final copy to disk.
    ///
    /// `agent_id` and `started_at` are forwarded to the indexer as metadata
    /// so future recalls can be traced back to this session.
    ///
    /// Indexing is best-effort: any error from the indexer is logged at
    /// `warn` and then dropped. This method returns `Ok(())` once the
    /// transcript file has been written successfully even if indexing
    /// failed.
    pub async fn close_session(
        &self,
        session_id: &str,
        agent_id: &str,
        started_at: chrono::DateTime<chrono::Utc>,
        transcript: &Transcript,
    ) -> Result<(), PersistenceError> {
        if let Some(indexer) = &self.indexer {
            match indexer
                .index_transcript(agent_id, session_id, started_at, transcript)
                .await
            {
                Ok(id) => {
                    tracing::debug!(
                        session_id,
                        agent_id,
                        memory_id = %id,
                        "session transcript indexed on close"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        session_id,
                        agent_id,
                        error = %err,
                        "session transcript indexing failed; close continues"
                    );
                }
            }
        }
        self.save_transcript(session_id, transcript)
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
    use crate::indexer::{IndexerError, TranscriptIndexer};
    use crate::transcript::{ContentBlock, Role};
    use async_trait::async_trait;
    use chrono::Utc;
    use sera_types::MemoryId;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
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

    // --- new tests ---

    #[test]
    fn persisted_transcript_empty_session() {
        let transcript = Transcript::new();
        let persisted = PersistedTranscript::from_transcript("empty-session", &transcript);
        assert_eq!(persisted.session_id, "empty-session");
        assert_eq!(persisted.entries.len(), 0);
        assert_eq!(persisted.metadata.entry_count, 0);
        assert_eq!(persisted.metadata.estimated_tokens, 0);
        let back = persisted.into_transcript();
        assert!(back.is_empty());
    }

    #[test]
    fn persisted_transcript_version_field() {
        let transcript = Transcript::new();
        let persisted = PersistedTranscript::from_transcript("v-test", &transcript);
        assert!(!persisted.version.is_empty());
    }

    #[test]
    fn session_manager_save_and_load() {
        let dir = std::env::temp_dir().join(format!("sera-session-test-{}", Uuid::new_v4()));
        let manager = SessionManager::new(dir.clone());

        let mut transcript = Transcript::new();
        transcript.append(make_entry(Role::User, "hello"));
        transcript.append(make_entry(Role::Assistant, "world"));

        manager.save_transcript("sess-abc", &transcript).unwrap();
        let loaded = manager.load_transcript("sess-abc").unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.len(), 2);

        // cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_manager_load_missing_returns_none() {
        let dir = std::env::temp_dir().join(format!("sera-session-test-{}", Uuid::new_v4()));
        let manager = SessionManager::new(dir.clone());
        let result = manager.load_transcript("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn session_manager_delete_transcript() {
        let dir = std::env::temp_dir().join(format!("sera-session-test-{}", Uuid::new_v4()));
        let manager = SessionManager::new(dir.clone());

        let transcript = Transcript::new();
        manager.save_transcript("sess-del", &transcript).unwrap();
        assert!(manager.load_transcript("sess-del").unwrap().is_some());

        manager.delete_transcript("sess-del").unwrap();
        assert!(manager.load_transcript("sess-del").unwrap().is_none());

        // Deleting non-existent should not error
        manager.delete_transcript("sess-del").unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_manager_list_sessions() {
        let dir = std::env::temp_dir().join(format!("sera-session-test-{}", Uuid::new_v4()));
        let manager = SessionManager::new(dir.clone());

        // Empty dir — no sessions yet (dir may not exist)
        let sessions = manager.list_sessions().unwrap();
        assert!(sessions.is_empty());

        let t = Transcript::new();
        manager.save_transcript("alpha", &t).unwrap();
        manager.save_transcript("beta", &t).unwrap();
        manager.save_transcript("gamma", &t).unwrap();

        let mut sessions = manager.list_sessions().unwrap();
        sessions.sort();
        assert_eq!(sessions, vec!["alpha", "beta", "gamma"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn persisted_transcript_load_from_file_not_found() {
        let path = std::path::PathBuf::from("/tmp/nonexistent-sera-session-xyz.json");
        let result = PersistedTranscript::load_from_file(&path);
        assert!(matches!(result, Err(PersistenceError::NotFound(_))));
    }

    // ── close_session + indexer tests ───────────────────────────────────────

    /// Tracks how many times an indexer was invoked and what args it saw.
    #[derive(Default)]
    struct SpyIndexer {
        calls: AtomicUsize,
        last: Mutex<Option<(String, String)>>,
        fail: bool,
    }

    #[async_trait]
    impl TranscriptIndexer for SpyIndexer {
        async fn index_transcript(
            &self,
            agent_id: &str,
            session_id: &str,
            _started_at: chrono::DateTime<chrono::Utc>,
            _transcript: &Transcript,
        ) -> Result<MemoryId, IndexerError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last.lock().unwrap() = Some((agent_id.to_string(), session_id.to_string()));
            if self.fail {
                return Err(IndexerError::Empty);
            }
            Ok(MemoryId::new("spy-id"))
        }
    }

    #[tokio::test]
    async fn close_session_invokes_indexer_and_saves() {
        let dir = std::env::temp_dir().join(format!("sera-close-test-{}", Uuid::new_v4()));
        let spy: Arc<SpyIndexer> = Arc::new(SpyIndexer::default());
        let manager = SessionManager::new(dir.clone())
            .with_indexer(spy.clone() as Arc<dyn TranscriptIndexer>);
        assert!(manager.has_indexer());

        let mut t = Transcript::new();
        t.append(make_entry(Role::User, "first user message"));

        manager
            .close_session("sess-close", "agent-a", Utc::now(), &t)
            .await
            .unwrap();

        assert_eq!(spy.calls.load(Ordering::SeqCst), 1);
        let last = spy.last.lock().unwrap().clone().unwrap();
        assert_eq!(last.0, "agent-a");
        assert_eq!(last.1, "sess-close");

        // Transcript file exists.
        let loaded = manager.load_transcript("sess-close").unwrap();
        assert!(loaded.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn close_session_indexer_failure_does_not_block() {
        let dir = std::env::temp_dir().join(format!("sera-close-fail-{}", Uuid::new_v4()));
        let spy: Arc<SpyIndexer> = Arc::new(SpyIndexer {
            calls: AtomicUsize::new(0),
            last: Mutex::new(None),
            fail: true,
        });
        let manager = SessionManager::new(dir.clone())
            .with_indexer(spy.clone() as Arc<dyn TranscriptIndexer>);

        let mut t = Transcript::new();
        t.append(make_entry(Role::User, "still gets saved"));

        // Even though the indexer errored, close_session returns Ok and
        // the transcript is persisted.
        manager
            .close_session("sess-fail", "agent-a", Utc::now(), &t)
            .await
            .unwrap();

        let loaded = manager.load_transcript("sess-fail").unwrap();
        assert!(loaded.is_some());
        assert_eq!(spy.calls.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn close_session_without_indexer_still_saves() {
        let dir = std::env::temp_dir().join(format!("sera-close-noidx-{}", Uuid::new_v4()));
        let manager = SessionManager::new(dir.clone());
        assert!(!manager.has_indexer());

        let mut t = Transcript::new();
        t.append(make_entry(Role::Assistant, "answer"));

        manager
            .close_session("sess-noidx", "agent-a", Utc::now(), &t)
            .await
            .unwrap();

        let loaded = manager.load_transcript("sess-noidx").unwrap();
        assert!(loaded.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn persisted_transcript_file_roundtrip() {
        let path = std::env::temp_dir()
            .join(format!("sera-pt-roundtrip-{}.json", Uuid::new_v4()));

        let mut transcript = Transcript::new();
        transcript.append(make_entry(Role::User, "ping"));
        transcript.append(make_entry(Role::Assistant, "pong"));

        let persisted = PersistedTranscript::from_transcript("rt-session", &transcript);
        persisted.save_to_file(&path).unwrap();

        let loaded = PersistedTranscript::load_from_file(&path).unwrap();
        assert_eq!(loaded.session_id, "rt-session");
        assert_eq!(loaded.entries.len(), 2);

        let _ = std::fs::remove_file(&path);
    }
}
