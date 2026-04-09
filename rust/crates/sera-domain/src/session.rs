//! Session types — conversation state management.
//!
//! MVS scope: two states only (Active, Archived) per mvs-review-plan §6.3.
//! No hook-driven transitions, no WaitingForApproval, no Compacting, no Suspended.
//! Session key format: "agent:{agent_id}:main" (single session per agent for MVS).

use serde::{Deserialize, Serialize};

/// Session state — the lifecycle of a conversation.
///
/// MVS uses only Active and Archived. The full state machine
/// (Created, WaitingForApproval, Compacting, Suspended, Destroyed)
/// is POST-MVS per SPEC-gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    Active,
    Archived,
}

/// An agent chat session with transcript persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    /// The agent instance this session belongs to.
    pub agent_id: String,
    /// Session routing key: "agent:{agent_id}:main" for MVS.
    pub session_key: String,
    pub state: SessionState,
    /// Principal ID of the session owner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl Session {
    /// Build the MVS session key for an agent.
    /// Format: "agent:{agent_id}:main" — single session per agent.
    pub fn mvs_session_key(agent_id: &str) -> String {
        format!("agent:{agent_id}:main")
    }

    /// Create a new active session for an agent.
    pub fn new(agent_id: &str) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.to_string(),
            session_key: Self::mvs_session_key(agent_id),
            state: SessionState::Active,
            principal_id: None,
            metadata: None,
            created_at: now.clone(),
            updated_at: Some(now),
        }
    }

    /// Archive this session (reset action).
    pub fn archive(&mut self) {
        self.state = SessionState::Archived;
        self.updated_at = Some(chrono::Utc::now().to_rfc3339());
    }

    pub fn is_active(&self) -> bool {
        self.state == SessionState::Active
    }
}

/// A single entry in the session transcript, persisted to SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_state_serde() {
        let json = serde_json::to_string(&SessionState::Active).unwrap();
        assert_eq!(json, "\"active\"");

        let parsed: SessionState = serde_json::from_str("\"archived\"").unwrap();
        assert_eq!(parsed, SessionState::Archived);
    }

    #[test]
    fn mvs_session_key_format() {
        assert_eq!(Session::mvs_session_key("sera"), "agent:sera:main");
    }

    #[test]
    fn new_session_is_active() {
        let session = Session::new("sera");
        assert!(session.is_active());
        assert_eq!(session.state, SessionState::Active);
        assert_eq!(session.session_key, "agent:sera:main");
    }

    #[test]
    fn archive_session() {
        let mut session = Session::new("sera");
        assert!(session.is_active());

        session.archive();
        assert!(!session.is_active());
        assert_eq!(session.state, SessionState::Archived);
    }

    #[test]
    fn session_roundtrip() {
        let session = Session::new("test-agent");
        let json = serde_json::to_string(&session).unwrap();
        let parsed: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.agent_id, "test-agent");
        assert_eq!(parsed.state, SessionState::Active);
    }
}
