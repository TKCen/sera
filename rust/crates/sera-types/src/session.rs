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
/// - Created → Active, Spawning, Shadow, Destroyed
/// - Spawning → Created (failure rollback), Destroyed (failure destroy)
/// - Active → WaitingForApproval, Compacting, Suspended, Archived, Destroyed, TrustRequired, ReadyForPrompt, Paused
/// - WaitingForApproval → Active, Archived, Destroyed
/// - Compacting → Active, Destroyed
/// - Suspended → Active, Archived, Destroyed
/// - Archived → Destroyed
/// - TrustRequired → Active (trust established), Archived (trust denied)
/// - ReadyForPrompt → Active (prompt received)
/// - Paused → Active (unpause), Archived, Destroyed
/// - Shadow → Destroyed (terminal for shadow sessions)
/// - Destroyed → (terminal)
#[non_exhaustive]
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
    /// Container starting — transitional state before Active.
    Spawning,
    /// Requires trust establishment before proceeding.
    #[serde(rename = "trust_required")]
    TrustRequired,
    /// Ready to accept user input.
    #[serde(rename = "ready_for_prompt")]
    ReadyForPrompt,
    /// Operator-initiated pause (distinct from Suspended which is system-initiated).
    Paused,
    /// Shadow session for testing/monitoring.
    Shadow,
}

impl SessionState {
    /// Check if a transition from this state to `target` is valid.
    pub fn can_transition_to(self, target: SessionState) -> bool {
        matches!(
            (self, target),
            // Created can activate, spawn, enter shadow, or be destroyed
            (SessionState::Created, SessionState::Active)
            | (SessionState::Created, SessionState::Destroyed)
            | (SessionState::Created, SessionState::Spawning)
            | (SessionState::Created, SessionState::Shadow)
            // Spawning can roll back to Created or be destroyed on failure
            | (SessionState::Spawning, SessionState::Created)
            | (SessionState::Spawning, SessionState::Destroyed)
            // Active is the hub — can go anywhere except Created
            | (SessionState::Active, SessionState::WaitingForApproval)
            | (SessionState::Active, SessionState::Compacting)
            | (SessionState::Active, SessionState::Suspended)
            | (SessionState::Active, SessionState::Archived)
            | (SessionState::Active, SessionState::Destroyed)
            | (SessionState::Active, SessionState::TrustRequired)
            | (SessionState::Active, SessionState::ReadyForPrompt)
            | (SessionState::Active, SessionState::Paused)
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
            // TrustRequired resolves to Active (trust established) or Archived (trust denied)
            | (SessionState::TrustRequired, SessionState::Active)
            | (SessionState::TrustRequired, SessionState::Archived)
            // ReadyForPrompt transitions to Active when prompt is received
            | (SessionState::ReadyForPrompt, SessionState::Active)
            // Paused can unpause, archive, or be destroyed
            | (SessionState::Paused, SessionState::Active)
            | (SessionState::Paused, SessionState::Archived)
            | (SessionState::Paused, SessionState::Destroyed)
            // Shadow sessions can only be destroyed
            | (SessionState::Shadow, SessionState::Destroyed) // Destroyed is terminal — no transitions
        )
    }

    /// Whether this state allows processing new turns.
    pub fn is_runnable(self) -> bool {
        matches!(self, SessionState::Active | SessionState::ReadyForPrompt)
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

// ── Session State Machine ─────────────────────────────────────────────────────

/// Error type for session state machine operations.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum SessionError {
    /// Attempted a transition not allowed from the current state.
    #[error("invalid transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: SessionState,
        to: SessionState,
    },
    /// Already in the requested target state.
    #[error("already in state {0:?}")]
    AlreadyInState(SessionState),
}

/// Condition that must be satisfied for a transition to be allowed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionCondition {
    /// Transition is always valid.
    Always,
    /// Transition requires HITL approval before it can proceed.
    RequiresApproval,
    /// Custom condition evaluated by hook — the string is the hook name.
    Custom(String),
}

/// A single allowed state transition with optional hook chain and condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTransition {
    /// The state being transitioned from.
    pub from: SessionState,
    /// The state being transitioned to.
    pub to: SessionState,
    /// Name of the hook chain to fire on this transition (SPEC-gateway §6,
    /// HookPoint::OnSessionTransition).
    pub hook_chain: Option<String>,
    /// Condition that must be satisfied for the transition to be valid.
    pub condition: Option<TransitionCondition>,
}

impl SessionTransition {
    fn new(from: SessionState, to: SessionState) -> Self {
        Self {
            from,
            to,
            hook_chain: None,
            condition: None,
        }
    }
}

/// State machine that manages session lifecycle transitions.
///
/// Wraps a `SessionState` with a validated transition table and history log.
/// Callers should prefer this over manipulating `Session::state` directly when
/// they need hook-chain integration.
#[derive(Debug, Clone)]
pub struct SessionStateMachine {
    current_state: SessionState,
    transitions: Vec<SessionTransition>,
    history: Vec<(SessionState, SessionState, chrono::DateTime<chrono::Utc>)>,
}

impl SessionStateMachine {
    /// Create a new state machine starting in `Created` state with the
    /// standard SERA session lifecycle transitions.
    pub fn new() -> Self {
        Self {
            current_state: SessionState::Created,
            transitions: Self::default_transitions(),
            history: Vec::new(),
        }
    }

    /// Build the default transition table for the standard session lifecycle.
    fn default_transitions() -> Vec<SessionTransition> {
        vec![
            SessionTransition::new(SessionState::Created, SessionState::Active),
            SessionTransition::new(SessionState::Created, SessionState::Spawning),
            SessionTransition::new(SessionState::Created, SessionState::Shadow),
            SessionTransition::new(SessionState::Spawning, SessionState::Created),
            SessionTransition::new(SessionState::Spawning, SessionState::Destroyed),
            SessionTransition::new(SessionState::Active, SessionState::WaitingForApproval),
            SessionTransition::new(SessionState::Active, SessionState::Compacting),
            SessionTransition::new(SessionState::Active, SessionState::Suspended),
            SessionTransition::new(SessionState::Active, SessionState::TrustRequired),
            SessionTransition::new(SessionState::Active, SessionState::ReadyForPrompt),
            SessionTransition::new(SessionState::Active, SessionState::Paused),
            SessionTransition::new(SessionState::WaitingForApproval, SessionState::Active),
            SessionTransition::new(SessionState::WaitingForApproval, SessionState::Suspended),
            SessionTransition::new(SessionState::Compacting, SessionState::Active),
            SessionTransition::new(SessionState::Compacting, SessionState::Archived),
            SessionTransition::new(SessionState::Suspended, SessionState::Active),
            SessionTransition::new(SessionState::Archived, SessionState::Destroyed),
            SessionTransition::new(SessionState::TrustRequired, SessionState::Active),
            SessionTransition::new(SessionState::TrustRequired, SessionState::Archived),
            SessionTransition::new(SessionState::ReadyForPrompt, SessionState::Active),
            SessionTransition::new(SessionState::Paused, SessionState::Active),
            SessionTransition::new(SessionState::Paused, SessionState::Archived),
            SessionTransition::new(SessionState::Paused, SessionState::Destroyed),
            SessionTransition::new(SessionState::Shadow, SessionState::Destroyed),
        ]
    }

    /// Return the current state.
    pub fn current(&self) -> SessionState {
        self.current_state
    }

    /// Return true if transitioning to `to` is allowed from the current state.
    pub fn can_transition(&self, to: &SessionState) -> bool {
        self.transitions
            .iter()
            .any(|t| t.from == self.current_state && t.to == *to)
    }

    /// Attempt to transition to `to`.
    ///
    /// On success, records the transition in history and returns the hook chain
    /// name if one is configured for this transition. Returns an error if the
    /// transition is not in the allowed table or the machine is already in `to`.
    pub fn transition(&mut self, to: SessionState) -> Result<Option<String>, SessionError> {
        if self.current_state == to {
            return Err(SessionError::AlreadyInState(to));
        }

        let hook_chain = self
            .transitions
            .iter()
            .find(|t| t.from == self.current_state && t.to == to)
            .ok_or(SessionError::InvalidTransition {
                from: self.current_state,
                to,
            })?
            .hook_chain
            .clone();

        let from = self.current_state;
        self.history.push((from, to, chrono::Utc::now()));
        self.current_state = to;
        Ok(hook_chain)
    }

    /// Return the full transition history as a slice.
    pub fn history(&self) -> &[(SessionState, SessionState, chrono::DateTime<chrono::Utc>)] {
        &self.history
    }
}

impl Default for SessionStateMachine {
    fn default() -> Self {
        Self::new()
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

    // ── SessionStateMachine tests ────────────────────────────────────────────

    #[test]
    fn state_machine_new_starts_created() {
        let sm = SessionStateMachine::new();
        assert_eq!(sm.current(), SessionState::Created);
        assert!(sm.history().is_empty());
    }

    #[test]
    fn state_machine_default_transitions_work() {
        let mut sm = SessionStateMachine::new();
        // Created → Active
        let result = sm.transition(SessionState::Active);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
        assert_eq!(sm.current(), SessionState::Active);

        // Active → WaitingForApproval
        sm.transition(SessionState::WaitingForApproval).unwrap();
        assert_eq!(sm.current(), SessionState::WaitingForApproval);

        // WaitingForApproval → Active (approval granted)
        sm.transition(SessionState::Active).unwrap();
        assert_eq!(sm.current(), SessionState::Active);

        // Active → Compacting
        sm.transition(SessionState::Compacting).unwrap();
        assert_eq!(sm.current(), SessionState::Compacting);

        // Compacting → Active
        sm.transition(SessionState::Active).unwrap();
        assert_eq!(sm.current(), SessionState::Active);

        // Active → Suspended
        sm.transition(SessionState::Suspended).unwrap();
        assert_eq!(sm.current(), SessionState::Suspended);

        // Suspended → Active (resumed)
        sm.transition(SessionState::Active).unwrap();
        assert_eq!(sm.current(), SessionState::Active);
    }

    #[test]
    fn state_machine_compacting_to_archived() {
        let mut sm = SessionStateMachine::new();
        sm.transition(SessionState::Active).unwrap();
        sm.transition(SessionState::Compacting).unwrap();
        sm.transition(SessionState::Archived).unwrap();
        assert_eq!(sm.current(), SessionState::Archived);
        // Archived → Destroyed
        sm.transition(SessionState::Destroyed).unwrap();
        assert_eq!(sm.current(), SessionState::Destroyed);
    }

    #[test]
    fn state_machine_waiting_for_approval_to_suspended() {
        let mut sm = SessionStateMachine::new();
        sm.transition(SessionState::Active).unwrap();
        sm.transition(SessionState::WaitingForApproval).unwrap();
        // denial/timeout → Suspended
        sm.transition(SessionState::Suspended).unwrap();
        assert_eq!(sm.current(), SessionState::Suspended);
    }

    #[test]
    fn state_machine_invalid_transition_rejected() {
        let mut sm = SessionStateMachine::new();
        // Created → Suspended is not in default table
        let err = sm.transition(SessionState::Suspended).unwrap_err();
        assert_eq!(
            err,
            SessionError::InvalidTransition {
                from: SessionState::Created,
                to: SessionState::Suspended,
            }
        );
        // State should be unchanged
        assert_eq!(sm.current(), SessionState::Created);
    }

    #[test]
    fn state_machine_already_in_state_error() {
        let mut sm = SessionStateMachine::new();
        let err = sm.transition(SessionState::Created).unwrap_err();
        assert_eq!(err, SessionError::AlreadyInState(SessionState::Created));
    }

    #[test]
    fn state_machine_history_tracking() {
        let mut sm = SessionStateMachine::new();
        sm.transition(SessionState::Active).unwrap();
        sm.transition(SessionState::Suspended).unwrap();
        sm.transition(SessionState::Active).unwrap();

        let history = sm.history();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].0, SessionState::Created);
        assert_eq!(history[0].1, SessionState::Active);
        assert_eq!(history[1].0, SessionState::Active);
        assert_eq!(history[1].1, SessionState::Suspended);
        assert_eq!(history[2].0, SessionState::Suspended);
        assert_eq!(history[2].1, SessionState::Active);
    }

    #[test]
    fn state_machine_can_transition_checks() {
        let sm = SessionStateMachine::new();
        // Created can go to Active
        assert!(sm.can_transition(&SessionState::Active));
        // Created cannot go to Destroyed (not in default table)
        assert!(!sm.can_transition(&SessionState::Destroyed));
        // Created cannot go to Suspended
        assert!(!sm.can_transition(&SessionState::Suspended));
    }

    #[test]
    fn state_machine_hook_chain_returned_on_transition() {
        let mut sm = SessionStateMachine::new();
        // Inject a hook chain on Created → Active
        sm.transitions[0].hook_chain = Some("on-activate-chain".to_string());
        let hook = sm.transition(SessionState::Active).unwrap();
        assert_eq!(hook, Some("on-activate-chain".to_string()));
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
