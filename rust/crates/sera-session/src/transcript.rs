//! Transcript — ContentBlock-based session transcript storage.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use sera_types::ContentBlock;

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

    // --- new tests ---

    #[test]
    fn transcript_clear_empties_entries() {
        let mut t = Transcript::new();
        t.append(make_entry(Role::User));
        t.append(make_entry(Role::Assistant));
        assert_eq!(t.len(), 2);
        t.clear();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
    }

    #[test]
    fn transcript_ordering_preserved() {
        let mut t = Transcript::new();
        let e1 = make_entry(Role::User);
        let e2 = make_entry(Role::Assistant);
        let e3 = make_entry(Role::User);
        let id1 = e1.id;
        let id2 = e2.id;
        let id3 = e3.id;
        t.append(e1);
        t.append(e2);
        t.append(e3);
        let entries = t.entries();
        assert_eq!(entries[0].id, id1);
        assert_eq!(entries[1].id, id2);
        assert_eq!(entries[2].id, id3);
    }

    #[test]
    fn transcript_last_n_returns_tail() {
        let mut t = Transcript::new();
        for _ in 0..5 {
            t.append(make_entry(Role::User));
        }
        let last_entry_id = t.entries().last().unwrap().id;
        let tail = t.last_n(2);
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[1].id, last_entry_id);
    }

    #[test]
    fn transcript_by_role_system_and_tool() {
        let mut t = Transcript::new();
        t.append(make_entry(Role::System));
        t.append(make_entry(Role::Tool));
        t.append(make_entry(Role::Tool));
        assert_eq!(t.by_role(&Role::System).len(), 1);
        assert_eq!(t.by_role(&Role::Tool).len(), 2);
        assert_eq!(t.by_role(&Role::User).len(), 0);
    }

    #[test]
    fn transcript_entry_cause_by_optional() {
        let mut entry = make_entry(Role::User);
        entry.cause_by = Some("turn-xyz".to_string());
        let json = serde_json::to_string(&entry).unwrap();
        let back: TranscriptEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cause_by, Some("turn-xyz".to_string()));

        // Without cause_by: field should be absent in JSON
        let entry_no_cause = make_entry(Role::User);
        let json2 = serde_json::to_string(&entry_no_cause).unwrap();
        assert!(!json2.contains("cause_by"));
        let back2: TranscriptEntry = serde_json::from_str(&json2).unwrap();
        assert!(back2.cause_by.is_none());
    }

    #[test]
    fn content_block_multi_type_entry() {
        // A single TranscriptEntry may carry multiple heterogeneous ContentBlocks.
        let entry = TranscriptEntry {
            id: Uuid::new_v4(),
            role: Role::Assistant,
            blocks: vec![
                ContentBlock::Thinking { thinking: "step 1".to_string() },
                ContentBlock::Text { text: "result".to_string() },
                ContentBlock::ToolUse {
                    id: "tu_1".to_string(),
                    name: "bash".to_string(),
                    input: serde_json::json!({"cmd": "pwd"}),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "tu_1".to_string(),
                    content: "/home".to_string(),
                    is_error: false,
                },
            ],
            timestamp: chrono::Utc::now(),
            cause_by: None,
        };
        assert_eq!(entry.blocks.len(), 4);
        let json = serde_json::to_string(&entry).unwrap();
        let back: TranscriptEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.blocks.len(), 4);
    }

    #[test]
    fn tool_result_is_error_flag() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu_err".to_string(),
            content: "command not found".to_string(),
            is_error: true,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("true"));
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn transcript_default_equals_new() {
        let t1 = Transcript::default();
        let t2 = Transcript::new();
        assert_eq!(t1.len(), t2.len());
        assert!(t1.is_empty());
    }
}
