//! Transcript — ContentBlock-based session transcript storage.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A content block in the transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    Image { media_type: String, data: String },
    Thinking { thinking: String },
}

/// Role of a transcript entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

/// A single entry in the transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub id: Uuid,
    pub role: Role,
    pub blocks: Vec<ContentBlock>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause_by: Option<String>,
}

/// Session transcript — ordered list of entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Transcript {
    entries: Vec<TranscriptEntry>,
}

impl Transcript {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, entry: TranscriptEntry) {
        self.entries.push(entry);
    }

    pub fn entries(&self) -> &[TranscriptEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get entries for a specific role.
    pub fn by_role(&self, role: &Role) -> Vec<&TranscriptEntry> {
        self.entries.iter().filter(|e| &e.role == role).collect()
    }

    /// Get the last N entries.
    pub fn last_n(&self, n: usize) -> &[TranscriptEntry] {
        let start = self.entries.len().saturating_sub(n);
        &self.entries[start..]
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(role: Role) -> TranscriptEntry {
        TranscriptEntry {
            id: Uuid::new_v4(),
            role,
            blocks: vec![ContentBlock::Text { text: "hello".to_string() }],
            timestamp: chrono::Utc::now(),
            cause_by: None,
        }
    }

    #[test]
    fn transcript_append_and_len() {
        let mut t = Transcript::new();
        t.append(make_entry(Role::User));
        t.append(make_entry(Role::Assistant));
        t.append(make_entry(Role::System));
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn transcript_by_role() {
        let mut t = Transcript::new();
        t.append(make_entry(Role::User));
        t.append(make_entry(Role::Assistant));
        t.append(make_entry(Role::User));
        let user_entries = t.by_role(&Role::User);
        assert_eq!(user_entries.len(), 2);
        let assistant_entries = t.by_role(&Role::Assistant);
        assert_eq!(assistant_entries.len(), 1);
        let tool_entries = t.by_role(&Role::Tool);
        assert_eq!(tool_entries.len(), 0);
    }

    #[test]
    fn transcript_last_n() {
        let mut t = Transcript::new();
        for _ in 0..5 {
            t.append(make_entry(Role::User));
        }
        assert_eq!(t.last_n(3).len(), 3);
        assert_eq!(t.last_n(10).len(), 5); // saturating
        assert_eq!(t.last_n(0).len(), 0);
    }

    #[test]
    fn content_block_serde_roundtrip() {
        let blocks = vec![
            ContentBlock::Text { text: "hello world".to_string() },
            ContentBlock::ToolUse {
                id: "tu_001".to_string(),
                name: "bash".to_string(),
                input: serde_json::json!({"cmd": "ls"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "tu_001".to_string(),
                content: "file.txt".to_string(),
                is_error: false,
            },
            ContentBlock::Image {
                media_type: "image/png".to_string(),
                data: "base64data".to_string(),
            },
            ContentBlock::Thinking {
                thinking: "I need to think about this".to_string(),
            },
        ];
        for block in &blocks {
            let json = serde_json::to_string(block).unwrap();
            let back: ContentBlock = serde_json::from_str(&json).unwrap();
            // Re-serialize and compare JSON to avoid PartialEq requirement on Value
            let json2 = serde_json::to_string(&back).unwrap();
            assert_eq!(json, json2);
        }
    }

    #[test]
    fn transcript_entry_serde_roundtrip() {
        let entry = TranscriptEntry {
            id: Uuid::new_v4(),
            role: Role::Assistant,
            blocks: vec![
                ContentBlock::Text { text: "I can help with that.".to_string() },
                ContentBlock::ToolUse {
                    id: "tu_abc".to_string(),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "/etc/hosts"}),
                },
            ],
            timestamp: chrono::Utc::now(),
            cause_by: Some("turn-001".to_string()),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: TranscriptEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry.id, back.id);
        assert_eq!(entry.role, back.role);
        assert_eq!(entry.blocks.len(), back.blocks.len());
        assert_eq!(entry.cause_by, back.cause_by);
    }

    #[test]
    fn empty_transcript() {
        let t = Transcript::new();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
        assert_eq!(t.entries().len(), 0);
        assert_eq!(t.last_n(5).len(), 0);
    }
}
