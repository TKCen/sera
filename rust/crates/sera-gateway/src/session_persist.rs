//! Two-layer session persistence — part table + shadow git snapshot.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single part of a session (tool call, text block, reasoning step).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPart {
    pub id: Uuid,
    pub session_key: String,
    pub part_type: PartType,
    pub content: serde_json::Value,
    pub sequence: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Types of session parts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartType {
    TextBlock,
    ToolCall,
    ToolResult,
    ReasoningStep,
    SystemMessage,
}

/// Part table — stores session parts.
/// P0 implementation is in-memory; sqlx streaming is Phase 1.
pub struct PartTable {
    parts: Vec<SessionPart>,
}

impl PartTable {
    pub fn new() -> Self {
        Self { parts: Vec::new() }
    }

    pub fn append(&mut self, part: SessionPart) {
        self.parts.push(part);
    }

    pub fn parts_for_session(&self, session_key: &str) -> Vec<&SessionPart> {
        self.parts
            .iter()
            .filter(|p| p.session_key == session_key)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

impl Default for PartTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Session snapshot — shadow git for workspace file tracking.
/// P0 stub — actual git2 integration is Phase 1.
pub struct SessionSnapshot {
    session_key: String,
    base_path: std::path::PathBuf,
}

impl SessionSnapshot {
    pub fn new(session_key: String, base_path: std::path::PathBuf) -> Self {
        Self {
            session_key,
            base_path,
        }
    }

    /// Track a file change.
    pub fn track(&self, _path: &std::path::Path) -> Result<(), std::io::Error> {
        // P0 stub — git2 integration in Phase 1
        Ok(())
    }

    /// Revert to last snapshot.
    pub fn revert(&self) -> Result<(), std::io::Error> {
        // P0 stub
        Ok(())
    }

    /// Get full diff since last snapshot.
    pub fn diff_full(&self) -> Result<String, std::io::Error> {
        // P0 stub
        Ok(String::new())
    }

    pub fn session_key(&self) -> &str {
        &self.session_key
    }

    pub fn base_path(&self) -> &std::path::Path {
        &self.base_path
    }
}
