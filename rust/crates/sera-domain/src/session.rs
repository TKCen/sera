//! Session types — conversation state management.
//!
//! MVS scope: two states only (Active, Archived) per mvs-review-plan §6.3.
//! No hook-driven transitions, no WaitingForApproval, no Compacting, no Suspended.
//! Session key format: "agent:{agent_id}:main" (single session per agent for MVS).

use serde::{Deserialize, Serialize};

/// Session state — the lifecycle of a conversation.
/// SPEC-gateway §6: full state machine with hook-driven transitions.
///
/// State transitions:
/// - Created → Active, Destroyed
/// - Active → WaitingForApproval, Compacting, Suspended, Archived, Destroyed
/// - WaitingForApproval → Active, Archived, Destroyed
/// - Compacting → Active, Destroyed
/// - Suspended → Active, Archived, Destroyed
/// - Archived → Destroyed
/// - Destroyed → (terminal)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    /// Initial state before first turn.
    Created,
    /// Session is actively processing turns.
    Active,
    /// Blocked on HITL approval (SPEC-hitl-approval).
    #[serde(rename = "waiting_for_approval")]
    WaitingForApproval,
    /// Context compaction in progress.
    Compacting,
    /// Temporarily paused by operator or system.
    Suspended,
    /// Conversation ended, transcript preserved.
    Archived,
    /// Permanently deleted, no recovery.
    Destroyed,
}

impl SessionState {
    /// Check if a transition from this state to `target` is valid.
    pub fn can_transition_to(self, target: SessionState) -> bool {
        matches!(
            (self, target),
            // Created can activate or be destroyed
            (SessionState::Created, SessionState::Active)
            | (SessionState::Created, SessionState::Destroyed)
            // Active is the hub — can go anywhere except Created
            | (SessionState::Active, SessionState::WaitingForApproval)
            | (SessionState::Active, SessionState::Compacting)
            | (SessionState::Active, SessionState::Suspended)
            | (SessionState::Active, SessionState::Archived)
            | (SessionState::Active, SessionState::Destroyed)
            // WaitingForApproval resolves back to Active, or ends
            | (SessionState::WaitingForApproval, SessionState::Active)
            | (SessionState::WaitingForApproval, SessionState::Archived)
            | (SessionState::WaitingForApproval, SessionState::Destroyed)
            // Compacting returns to Active or fails
            | (SessionState::Compacting, SessionState::Active)
            | (SessionState::Compacting, SessionState::Destroyed)
            // Suspended can resume or end
            | (SessionState::Suspended, SessionState::Active)
            | (SessionState::Suspended, SessionState::Archived)
            | (SessionState::Suspended, SessionState::Destroyed)
            // Archived can only be destroyed
            | (SessionState::Archived, SessionState::Destroyed)
            // Destroyed is terminal — no transitions
        )
    }

    /// Whether this state allows processing new turns.
    pub fn is_runnable(self) -> bool {
        self == SessionState::Active
    }

    /// Whether this is a terminal state.
    pub fn is_terminal(self) -> bool {
        self == SessionState::Destroyed
    }
}

/// Session scoping strategy — determines how session keys are constructed.
/// SPEC-gateway §6.3: 5 strategies for different isolation levels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionScope {
    /// Single session per agent: "agent:{agent_id}:main"
    Main,
    /// Per-channel: "agent:{agent_id}:channel:{channel_id}"
    PerChannel,
    /// Per-channel-peer: "agent:{agent_id}:channel:{channel_id}:peer:{peer_id}"
    PerChannelPeer,
    /// Full isolation: "agent:{agent_id}:account:{account}:channel:{channel_id}:peer:{peer_id}"
    PerAccountChannelPeer,
    /// Thread-level: "agent:{agent_id}:channel:{channel_id}:thread:{thread_id}"
    PerThread,
}

/// Parameters needed to construct a session key for a given scope.
#[derive(Debug, Clone)]
pub struct SessionScopeParams {
    pub agent_id: String,
    pub channel_id: Option<String>,
    pub peer_id: Option<String>,
    pub account_id: Option<String>,
    pub thread_id: Option<String>,
}

impl SessionScope {
    /// Build a session key string from scope + parameters.
    pub fn build_key(&self, params: &SessionScopeParams) -> String {
        match self {
            SessionScope::Main => format!("agent:{}:main", params.agent_id),
            SessionScope::PerChannel => {
                let ch = params.channel_id.as_deref().unwrap_or("default");
                format!("agent:{}:channel:{}", params.agent_id, ch)
            }
            SessionScope::PerChannelPeer => {
                let ch = params.channel_id.as_deref().unwrap_or("default");
                let peer = params.peer_id.as_deref().unwrap_or("unknown");
                format!("agent:{}:channel:{}:peer:{}", params.agent_id, ch, peer)
            }
            SessionScope::PerAccountChannelPeer => {
                let acct = params.account_id.as_deref().unwrap_or("default");
                let ch = params.channel_id.as_deref().unwrap_or("default");
                let peer = params.peer_id.as_deref().unwrap_or("unknown");
                format!(
                    "agent:{}:account:{}:channel:{}:peer:{}",
                    params.agent_id, acct, ch, peer
                )
            }
            SessionScope::PerThread => {
                let ch = params.channel_id.as_deref().unwrap_or("default");
                let thread = params.thread_id.as_deref().unwrap_or("main");
                format!("agent:{}:channel:{}:thread:{}", params.agent_id, ch, thread)
            }
        }
    }
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

    /// Create a new session in Created state.
    pub fn new(agent_id: &str) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.to_string(),
            session_key: Self::mvs_session_key(agent_id),
            state: SessionState::Created,
            principal_id: None,
            metadata: None,
            created_at: now.clone(),
            updated_at: Some(now),
        }
    }

    /// Create a new session with a specific scope.
    pub fn with_scope(agent_id: &str, scope: &SessionScope, params: &SessionScopeParams) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.to_string(),
            session_key: scope.build_key(params),
            state: SessionState::Created,
            principal_id: None,
            metadata: None,
            created_at: now.clone(),
            updated_at: Some(now),
        }
    }

    fn touch(&mut self) {
        self.updated_at = Some(chrono::Utc::now().to_rfc3339());
    }

    /// Transition Created → Active. Returns false if transition is invalid.
    pub fn activate(&mut self) -> bool {
        if self.state.can_transition_to(SessionState::Active) {
            self.state = SessionState::Active;
            self.touch();
            true
        } else {
            false
        }
    }

    /// Archive this session (reset action).
    pub fn archive(&mut self) -> bool {
        if self.state.can_transition_to(SessionState::Archived) {
            self.state = SessionState::Archived;
            self.touch();
            true
        } else {
            false
        }
    }

    /// Suspend this session.
    pub fn suspend(&mut self) -> bool {
        if self.state.can_transition_to(SessionState::Suspended) {
            self.state = SessionState::Suspended;
            self.touch();
            true
        } else {
            false
        }
    }

    /// Resume a suspended session → Active.
    pub fn resume(&mut self) -> bool {
        if self.state == SessionState::Suspended
            && self.state.can_transition_to(SessionState::Active)
        {
            self.state = SessionState::Active;
            self.touch();
            true
        } else {
            false
        }
    }

    /// Permanently destroy this session.
    pub fn destroy(&mut self) -> bool {
        if self.state.can_transition_to(SessionState::Destroyed) {
            self.state = SessionState::Destroyed;
            self.touch();
            true
        } else {
            false
        }
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
    fn session_state_all_variants_serde() {
        let variants = vec![
            (SessionState::Created, "created"),
            (SessionState::Active, "active"),
            (SessionState::WaitingForApproval, "waiting_for_approval"),
            (SessionState::Compacting, "compacting"),
            (SessionState::Suspended, "suspended"),
            (SessionState::Archived, "archived"),
            (SessionState::Destroyed, "destroyed"),
        ];
        for (state, expected) in variants {
            let json = serde_json::to_string(&state).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
            let parsed: SessionState = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, state);
        }
    }

    #[test]
    fn mvs_session_key_format() {
        assert_eq!(Session::mvs_session_key("sera"), "agent:sera:main");
    }

    #[test]
    fn new_session_is_created() {
        let session = Session::new("sera");
        assert_eq!(session.state, SessionState::Created);
        assert!(!session.is_active());
        assert_eq!(session.session_key, "agent:sera:main");
    }

    #[test]
    fn activate_session() {
        let mut session = Session::new("sera");
        assert!(session.activate());
        assert!(session.is_active());
        assert_eq!(session.state, SessionState::Active);
    }

    #[test]
    fn archive_session() {
        let mut session = Session::new("sera");
        session.activate();
        assert!(session.archive());
        assert!(!session.is_active());
        assert_eq!(session.state, SessionState::Archived);
    }

    #[test]
    fn suspend_and_resume() {
        let mut session = Session::new("sera");
        session.activate();
        assert!(session.suspend());
        assert_eq!(session.state, SessionState::Suspended);
        assert!(session.resume());
        assert!(session.is_active());
    }

    #[test]
    fn destroy_from_any_state() {
        for start in &[
            SessionState::Created,
            SessionState::Active,
            SessionState::WaitingForApproval,
            SessionState::Compacting,
            SessionState::Suspended,
            SessionState::Archived,
        ] {
            let mut session = Session::new("sera");
            session.state = *start;
            assert!(session.destroy(), "should destroy from {start:?}");
            assert_eq!(session.state, SessionState::Destroyed);
        }
    }

    #[test]
    fn destroyed_is_terminal() {
        let mut session = Session::new("sera");
        session.state = SessionState::Destroyed;
        assert!(session.state.is_terminal());
        assert!(!session.activate());
        assert!(!session.archive());
        assert!(!session.suspend());
        assert!(!session.destroy());
    }

    #[test]
    fn invalid_transitions_rejected() {
        // Created cannot go to Suspended directly
        assert!(!SessionState::Created.can_transition_to(SessionState::Suspended));
        // Archived cannot go back to Active
        assert!(!SessionState::Archived.can_transition_to(SessionState::Active));
        // Compacting cannot suspend
        assert!(!SessionState::Compacting.can_transition_to(SessionState::Suspended));
    }

    #[test]
    fn valid_transitions_accepted() {
        assert!(SessionState::Created.can_transition_to(SessionState::Active));
        assert!(SessionState::Active.can_transition_to(SessionState::WaitingForApproval));
        assert!(SessionState::Active.can_transition_to(SessionState::Compacting));
        assert!(SessionState::WaitingForApproval.can_transition_to(SessionState::Active));
        assert!(SessionState::Suspended.can_transition_to(SessionState::Active));
    }

    #[test]
    fn session_roundtrip() {
        let mut session = Session::new("test-agent");
        session.activate();
        let json = serde_json::to_string(&session).unwrap();
        let parsed: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.agent_id, "test-agent");
        assert_eq!(parsed.state, SessionState::Active);
    }

    #[test]
    fn session_scope_main() {
        let params = SessionScopeParams {
            agent_id: "sera".to_string(),
            channel_id: None,
            peer_id: None,
            account_id: None,
            thread_id: None,
        };
        assert_eq!(SessionScope::Main.build_key(&params), "agent:sera:main");
    }

    #[test]
    fn session_scope_per_channel() {
        let params = SessionScopeParams {
            agent_id: "sera".to_string(),
            channel_id: Some("discord-general".to_string()),
            peer_id: None,
            account_id: None,
            thread_id: None,
        };
        assert_eq!(
            SessionScope::PerChannel.build_key(&params),
            "agent:sera:channel:discord-general"
        );
    }

    #[test]
    fn session_scope_per_channel_peer() {
        let params = SessionScopeParams {
            agent_id: "sera".to_string(),
            channel_id: Some("discord-general".to_string()),
            peer_id: Some("user-123".to_string()),
            account_id: None,
            thread_id: None,
        };
        assert_eq!(
            SessionScope::PerChannelPeer.build_key(&params),
            "agent:sera:channel:discord-general:peer:user-123"
        );
    }

    #[test]
    fn session_scope_per_account_channel_peer() {
        let params = SessionScopeParams {
            agent_id: "sera".to_string(),
            channel_id: Some("ch1".to_string()),
            peer_id: Some("peer1".to_string()),
            account_id: Some("acct1".to_string()),
            thread_id: None,
        };
        assert_eq!(
            SessionScope::PerAccountChannelPeer.build_key(&params),
            "agent:sera:account:acct1:channel:ch1:peer:peer1"
        );
    }

    #[test]
    fn session_scope_per_thread() {
        let params = SessionScopeParams {
            agent_id: "sera".to_string(),
            channel_id: Some("ch1".to_string()),
            peer_id: None,
            account_id: None,
            thread_id: Some("thread-42".to_string()),
        };
        assert_eq!(
            SessionScope::PerThread.build_key(&params),
            "agent:sera:channel:ch1:thread:thread-42"
        );
    }

    #[test]
    fn session_scope_serde() {
        let json = serde_json::to_string(&SessionScope::PerChannelPeer).unwrap();
        assert_eq!(json, "\"per_channel_peer\"");
        let parsed: SessionScope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, SessionScope::PerChannelPeer);
    }

    #[test]
    fn session_with_scope() {
        let params = SessionScopeParams {
            agent_id: "sera".to_string(),
            channel_id: Some("discord".to_string()),
            peer_id: Some("user1".to_string()),
            account_id: None,
            thread_id: None,
        };
        let session = Session::with_scope("sera", &SessionScope::PerChannelPeer, &params);
        assert_eq!(session.session_key, "agent:sera:channel:discord:peer:user1");
        assert_eq!(session.state, SessionState::Created);
    }

    #[test]
    fn is_runnable() {
        assert!(!SessionState::Created.is_runnable());
        assert!(SessionState::Active.is_runnable());
        assert!(!SessionState::WaitingForApproval.is_runnable());
        assert!(!SessionState::Suspended.is_runnable());
        assert!(!SessionState::Archived.is_runnable());
        assert!(!SessionState::Destroyed.is_runnable());
    }
}
