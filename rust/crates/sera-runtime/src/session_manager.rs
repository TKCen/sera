//! Session manager — wraps sera-db SQLite for transcript persistence.

use crate::types::ChatMessage;
use sera_db::sqlite::SqliteDb;
use std::path::Path;

/// Manages session state and transcript for the MVS turn loop.
/// Backed by sera-db SQLite for persistence.
pub struct SessionManager {
    db: SqliteDb,
}

#[allow(dead_code)]
impl SessionManager {
    /// Open (or create) a database file at the given path.
    pub fn new(db_path: &str) -> anyhow::Result<Self> {
        let db = SqliteDb::open(Path::new(db_path))
            .map_err(|e| anyhow::anyhow!("Failed to open SQLite DB at {db_path}: {e}"))?;
        Ok(Self { db })
    }

    /// Create a session manager backed by an in-memory database (useful for tests).
    pub fn new_in_memory() -> anyhow::Result<Self> {
        let db = SqliteDb::open_in_memory()
            .map_err(|e| anyhow::anyhow!("Failed to open in-memory SQLite DB: {e}"))?;
        Ok(Self { db })
    }

    /// Get or create the active session for an agent. Returns the session ID.
    pub fn get_or_create_session(&self, agent_id: &str) -> anyhow::Result<String> {
        let row = self
            .db
            .get_or_create_session(agent_id)
            .map_err(|e| anyhow::anyhow!("Failed to get/create session for {agent_id}: {e}"))?;
        Ok(row.id)
    }

    /// Load the full transcript as ChatMessage list for context assembly.
    pub fn load_transcript(&self, session_id: &str) -> anyhow::Result<Vec<ChatMessage>> {
        let rows = self
            .db
            .get_transcript(session_id)
            .map_err(|e| anyhow::anyhow!("Failed to load transcript for {session_id}: {e}"))?;

        let messages = rows
            .into_iter()
            .map(|row| {
                let tool_calls = row.tool_calls.and_then(|tc_json| {
                    serde_json::from_str(&tc_json).ok()
                });

                ChatMessage {
                    role: row.role,
                    content: row.content,
                    tool_calls,
                    tool_call_id: row.tool_call_id,
                    name: None,
                }
            })
            .collect();

        Ok(messages)
    }

    /// Append a message to the transcript.
    pub fn append_message(&self, session_id: &str, msg: &ChatMessage) -> anyhow::Result<()> {
        let tool_calls_json = msg.tool_calls.as_ref().map(|tcs| {
            serde_json::to_string(tcs).unwrap_or_else(|_| "[]".to_string())
        });

        self.db
            .append_transcript(
                session_id,
                &msg.role,
                msg.content.as_deref(),
                tool_calls_json.as_deref(),
                msg.tool_call_id.as_deref(),
            )
            .map_err(|e| anyhow::anyhow!("Failed to append message to {session_id}: {e}"))?;

        Ok(())
    }

    /// Archive the current session and start a fresh one. Returns the new session ID.
    pub fn reset_session(&self, agent_id: &str) -> anyhow::Result<String> {
        // Find the current active session and mark it archived.
        let current = self.db.get_or_create_session(agent_id).map_err(|e| {
            anyhow::anyhow!("Failed to find active session for {agent_id}: {e}")
        })?;

        self.db
            .update_session_state(&current.id, "archived")
            .map_err(|e| anyhow::anyhow!("Failed to archive session {}: {e}", current.id))?;

        // Now get_or_create will create a new one since we archived the active one.
        let new_row = self.db.get_or_create_session(agent_id).map_err(|e| {
            anyhow::anyhow!("Failed to create new session for {agent_id}: {e}")
        })?;

        Ok(new_row.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolCall;

    #[test]
    fn create_and_load_empty_transcript() {
        let sm = SessionManager::new_in_memory().unwrap();
        let sid = sm.get_or_create_session("agent-1").unwrap();
        assert!(!sid.is_empty());

        let transcript = sm.load_transcript(&sid).unwrap();
        assert!(transcript.is_empty());
    }

    #[test]
    fn append_and_load_messages() {
        let sm = SessionManager::new_in_memory().unwrap();
        let sid = sm.get_or_create_session("agent-2").unwrap();

        let user_msg = ChatMessage {
            role: "user".to_string(),
            content: Some("Hello agent".to_string()),
            ..Default::default()
        };
        sm.append_message(&sid, &user_msg).unwrap();

        let assistant_msg = ChatMessage {
            role: "assistant".to_string(),
            content: Some("Hello user".to_string()),
            ..Default::default()
        };
        sm.append_message(&sid, &assistant_msg).unwrap();

        let transcript = sm.load_transcript(&sid).unwrap();
        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[0].role, "user");
        assert_eq!(transcript[0].content.as_deref(), Some("Hello agent"));
        assert_eq!(transcript[1].role, "assistant");
        assert_eq!(transcript[1].content.as_deref(), Some("Hello user"));
    }

    #[test]
    fn append_message_with_tool_calls() {
        let sm = SessionManager::new_in_memory().unwrap();
        let sid = sm.get_or_create_session("agent-3").unwrap();

        let msg = ChatMessage {
            role: "assistant".to_string(),
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: "tc_1".to_string(),
                call_type: "function".to_string(),
                function: crate::types::ToolCallFunction {
                    name: "file_read".to_string(),
                    arguments: r#"{"path":"test.txt"}"#.to_string(),
                },
            }]),
            ..Default::default()
        };
        sm.append_message(&sid, &msg).unwrap();

        let tool_result = ChatMessage {
            role: "tool".to_string(),
            content: Some("file contents here".to_string()),
            tool_call_id: Some("tc_1".to_string()),
            ..Default::default()
        };
        sm.append_message(&sid, &tool_result).unwrap();

        let transcript = sm.load_transcript(&sid).unwrap();
        assert_eq!(transcript.len(), 2);
        assert!(transcript[0].tool_calls.is_some());
        let tcs = transcript[0].tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].function.name, "file_read");
        assert_eq!(transcript[1].tool_call_id.as_deref(), Some("tc_1"));
    }

    #[test]
    fn get_or_create_returns_same_session() {
        let sm = SessionManager::new_in_memory().unwrap();
        let sid1 = sm.get_or_create_session("agent-4").unwrap();
        let sid2 = sm.get_or_create_session("agent-4").unwrap();
        assert_eq!(sid1, sid2);
    }

    #[test]
    fn reset_session_creates_new() {
        let sm = SessionManager::new_in_memory().unwrap();
        let sid1 = sm.get_or_create_session("agent-5").unwrap();

        // Append a message so the transcript is non-empty.
        sm.append_message(
            &sid1,
            &ChatMessage {
                role: "user".to_string(),
                content: Some("old message".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        let sid2 = sm.reset_session("agent-5").unwrap();
        assert_ne!(sid1, sid2);

        // New session should have empty transcript.
        let transcript = sm.load_transcript(&sid2).unwrap();
        assert!(transcript.is_empty());

        // Old session transcript should still exist.
        let old_transcript = sm.load_transcript(&sid1).unwrap();
        assert_eq!(old_transcript.len(), 1);
    }

    #[test]
    fn reset_session_full_cycle() {
        let sm = SessionManager::new_in_memory().unwrap();

        // Create, populate, reset, populate again.
        let sid1 = sm.get_or_create_session("agent-6").unwrap();
        sm.append_message(
            &sid1,
            &ChatMessage {
                role: "user".to_string(),
                content: Some("msg1".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        let sid2 = sm.reset_session("agent-6").unwrap();
        sm.append_message(
            &sid2,
            &ChatMessage {
                role: "user".to_string(),
                content: Some("msg2".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        // Verify isolation.
        let t1 = sm.load_transcript(&sid1).unwrap();
        let t2 = sm.load_transcript(&sid2).unwrap();
        assert_eq!(t1.len(), 1);
        assert_eq!(t1[0].content.as_deref(), Some("msg1"));
        assert_eq!(t2.len(), 1);
        assert_eq!(t2[0].content.as_deref(), Some("msg2"));
    }
}
