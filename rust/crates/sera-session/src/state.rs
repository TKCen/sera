//! SessionState — 6-state finite state machine for session lifecycle.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Session states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// Session created, not yet started.
    Created,
    /// Session is actively processing a turn.
    Active,
    /// Session is idle, waiting for input.
    Idle,
    /// Session is suspended (can be resumed).
    Suspended,
    /// Session is being compacted.
    Compacting,
    /// Session is closed (terminal state).
    Closed,
}

/// Errors from state transitions.
#[derive(Debug, Error)]
pub enum SessionStateError {
    #[error("invalid transition from {from:?} to {to:?}")]
    InvalidTransition { from: SessionState, to: SessionState },
}

/// Session state machine.
#[derive(Debug, Clone)]
pub struct SessionStateMachine {
    state: SessionState,
    session_key: String,
    transitions: Vec<(SessionState, SessionState, chrono::DateTime<chrono::Utc>)>,
}

impl SessionStateMachine {
    pub fn new(session_key: String) -> Self {
        Self {
            state: SessionState::Created,
            session_key,
            transitions: Vec::new(),
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn session_key(&self) -> &str {
        &self.session_key
    }

    pub fn transition_count(&self) -> usize {
        self.transitions.len()
    }

    /// Attempt a state transition. Returns error if the transition is not allowed.
    pub fn transition(&mut self, to: SessionState) -> Result<(), SessionStateError> {
        if !self.is_valid_transition(to) {
            return Err(SessionStateError::InvalidTransition {
                from: self.state,
                to,
            });
        }
        let from = self.state;
        self.state = to;
        self.transitions.push((from, to, chrono::Utc::now()));
        Ok(())
    }

    /// Check if a transition is valid.
    pub fn is_valid_transition(&self, to: SessionState) -> bool {
        matches!(
            (self.state, to),
            // From Created
            (SessionState::Created, SessionState::Active)
            | (SessionState::Created, SessionState::Closed)
            // From Active
            | (SessionState::Active, SessionState::Idle)
            | (SessionState::Active, SessionState::Compacting)
            | (SessionState::Active, SessionState::Suspended)
            | (SessionState::Active, SessionState::Closed)
            // From Idle
            | (SessionState::Idle, SessionState::Active)
            | (SessionState::Idle, SessionState::Suspended)
            | (SessionState::Idle, SessionState::Closed)
            // From Suspended
            | (SessionState::Suspended, SessionState::Active)
            | (SessionState::Suspended, SessionState::Closed)
            // From Compacting
            | (SessionState::Compacting, SessionState::Active)
            | (SessionState::Compacting, SessionState::Idle)
            | (SessionState::Compacting, SessionState::Closed)
            // Closed is terminal — no transitions out
        )
    }

    /// Check if the session is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        self.state == SessionState::Closed
    }

    /// Check if the session can accept new input.
    pub fn can_accept_input(&self) -> bool {
        matches!(self.state, SessionState::Active | SessionState::Idle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_starts_in_created() {
        let machine = SessionStateMachine::new("key-001".to_string());
        assert_eq!(machine.state(), SessionState::Created);
    }

    #[test]
    fn valid_transitions_succeed() {
        let mut machine = SessionStateMachine::new("key-002".to_string());
        machine.transition(SessionState::Active).unwrap();
        assert_eq!(machine.state(), SessionState::Active);
        machine.transition(SessionState::Idle).unwrap();
        assert_eq!(machine.state(), SessionState::Idle);
        machine.transition(SessionState::Active).unwrap();
        assert_eq!(machine.state(), SessionState::Active);
        machine.transition(SessionState::Closed).unwrap();
        assert_eq!(machine.state(), SessionState::Closed);
    }

    #[test]
    fn invalid_transition_returns_error() {
        let mut machine = SessionStateMachine::new("key-003".to_string());
        let err = machine.transition(SessionState::Idle).unwrap_err();
        assert!(matches!(
            err,
            SessionStateError::InvalidTransition {
                from: SessionState::Created,
                to: SessionState::Idle
            }
        ));
    }

    #[test]
    fn closed_is_terminal() {
        let mut machine = SessionStateMachine::new("key-004".to_string());
        machine.transition(SessionState::Closed).unwrap();
        assert!(machine.is_terminal());
        // No transitions out of Closed
        assert!(machine.transition(SessionState::Active).is_err());
        assert!(machine.transition(SessionState::Idle).is_err());
        assert!(machine.transition(SessionState::Suspended).is_err());
        assert!(machine.transition(SessionState::Compacting).is_err());
        assert!(machine.transition(SessionState::Created).is_err());
    }

    #[test]
    fn transition_history_recorded() {
        let mut machine = SessionStateMachine::new("key-005".to_string());
        assert_eq!(machine.transition_count(), 0);
        machine.transition(SessionState::Active).unwrap();
        assert_eq!(machine.transition_count(), 1);
        machine.transition(SessionState::Idle).unwrap();
        assert_eq!(machine.transition_count(), 2);
    }

    #[test]
    fn can_accept_input() {
        let mut machine = SessionStateMachine::new("key-006".to_string());
        assert!(!machine.can_accept_input()); // Created
        machine.transition(SessionState::Active).unwrap();
        assert!(machine.can_accept_input()); // Active
        machine.transition(SessionState::Idle).unwrap();
        assert!(machine.can_accept_input()); // Idle
        machine.transition(SessionState::Suspended).unwrap();
        assert!(!machine.can_accept_input()); // Suspended
    }

    #[test]
    fn all_valid_transitions_compile() {
        // Created → Active
        let mut m = SessionStateMachine::new("t".to_string());
        assert!(m.transition(SessionState::Active).is_ok());
        // Created → Closed
        let mut m = SessionStateMachine::new("t".to_string());
        assert!(m.transition(SessionState::Closed).is_ok());
        // Active → Idle
        let mut m = SessionStateMachine::new("t".to_string());
        m.transition(SessionState::Active).unwrap();
        assert!(m.transition(SessionState::Idle).is_ok());
        // Active → Compacting
        let mut m = SessionStateMachine::new("t".to_string());
        m.transition(SessionState::Active).unwrap();
        assert!(m.transition(SessionState::Compacting).is_ok());
        // Active → Suspended
        let mut m = SessionStateMachine::new("t".to_string());
        m.transition(SessionState::Active).unwrap();
        assert!(m.transition(SessionState::Suspended).is_ok());
        // Active → Closed
        let mut m = SessionStateMachine::new("t".to_string());
        m.transition(SessionState::Active).unwrap();
        assert!(m.transition(SessionState::Closed).is_ok());
        // Idle → Active
        let mut m = SessionStateMachine::new("t".to_string());
        m.transition(SessionState::Active).unwrap();
        m.transition(SessionState::Idle).unwrap();
        assert!(m.transition(SessionState::Active).is_ok());
        // Idle → Suspended
        let mut m = SessionStateMachine::new("t".to_string());
        m.transition(SessionState::Active).unwrap();
        m.transition(SessionState::Idle).unwrap();
        assert!(m.transition(SessionState::Suspended).is_ok());
        // Idle → Closed
        let mut m = SessionStateMachine::new("t".to_string());
        m.transition(SessionState::Active).unwrap();
        m.transition(SessionState::Idle).unwrap();
        assert!(m.transition(SessionState::Closed).is_ok());
        // Suspended → Active
        let mut m = SessionStateMachine::new("t".to_string());
        m.transition(SessionState::Active).unwrap();
        m.transition(SessionState::Suspended).unwrap();
        assert!(m.transition(SessionState::Active).is_ok());
        // Suspended → Closed
        let mut m = SessionStateMachine::new("t".to_string());
        m.transition(SessionState::Active).unwrap();
        m.transition(SessionState::Suspended).unwrap();
        assert!(m.transition(SessionState::Closed).is_ok());
        // Compacting → Active
        let mut m = SessionStateMachine::new("t".to_string());
        m.transition(SessionState::Active).unwrap();
        m.transition(SessionState::Compacting).unwrap();
        assert!(m.transition(SessionState::Active).is_ok());
        // Compacting → Idle
        let mut m = SessionStateMachine::new("t".to_string());
        m.transition(SessionState::Active).unwrap();
        m.transition(SessionState::Compacting).unwrap();
        assert!(m.transition(SessionState::Idle).is_ok());
        // Compacting → Closed
        let mut m = SessionStateMachine::new("t".to_string());
        m.transition(SessionState::Active).unwrap();
        m.transition(SessionState::Compacting).unwrap();
        assert!(m.transition(SessionState::Closed).is_ok());
    }

    #[test]
    fn serde_roundtrip() {
        let states = [
            SessionState::Created,
            SessionState::Active,
            SessionState::Idle,
            SessionState::Suspended,
            SessionState::Compacting,
            SessionState::Closed,
        ];
        for state in states {
            let json = serde_json::to_string(&state).unwrap();
            let back: SessionState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, back);
        }
    }

    // --- new tests ---

    #[test]
    fn session_key_accessor() {
        let m = SessionStateMachine::new("my-key".to_string());
        assert_eq!(m.session_key(), "my-key");
    }

    #[test]
    fn created_cannot_go_to_idle() {
        let mut m = SessionStateMachine::new("k".to_string());
        assert!(!m.is_valid_transition(SessionState::Idle));
        assert!(m.transition(SessionState::Idle).is_err());
    }

    #[test]
    fn created_cannot_go_to_suspended() {
        let mut m = SessionStateMachine::new("k".to_string());
        assert!(!m.is_valid_transition(SessionState::Suspended));
        assert!(m.transition(SessionState::Suspended).is_err());
    }

    #[test]
    fn created_cannot_go_to_compacting() {
        let mut m = SessionStateMachine::new("k".to_string());
        assert!(!m.is_valid_transition(SessionState::Compacting));
        assert!(m.transition(SessionState::Compacting).is_err());
    }

    #[test]
    fn suspended_cannot_go_to_idle() {
        let mut m = SessionStateMachine::new("k".to_string());
        m.transition(SessionState::Active).unwrap();
        m.transition(SessionState::Suspended).unwrap();
        assert!(!m.is_valid_transition(SessionState::Idle));
        assert!(m.transition(SessionState::Idle).is_err());
    }

    #[test]
    fn suspended_cannot_go_to_compacting() {
        let mut m = SessionStateMachine::new("k".to_string());
        m.transition(SessionState::Active).unwrap();
        m.transition(SessionState::Suspended).unwrap();
        assert!(!m.is_valid_transition(SessionState::Compacting));
        assert!(m.transition(SessionState::Compacting).is_err());
    }

    #[test]
    fn compacting_cannot_go_to_suspended() {
        let mut m = SessionStateMachine::new("k".to_string());
        m.transition(SessionState::Active).unwrap();
        m.transition(SessionState::Compacting).unwrap();
        assert!(!m.is_valid_transition(SessionState::Suspended));
        assert!(m.transition(SessionState::Suspended).is_err());
    }

    #[test]
    fn closed_cannot_go_to_closed() {
        let mut m = SessionStateMachine::new("k".to_string());
        m.transition(SessionState::Closed).unwrap();
        // Closed → Closed is also invalid (self-transition not listed)
        assert!(!m.is_valid_transition(SessionState::Closed));
        assert!(m.transition(SessionState::Closed).is_err());
    }

    #[test]
    fn error_message_contains_states() {
        let mut m = SessionStateMachine::new("k".to_string());
        let err = m.transition(SessionState::Idle).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Created") || msg.contains("created"), "msg={msg}");
        assert!(msg.contains("Idle") || msg.contains("idle"), "msg={msg}");
    }

    #[test]
    fn compacting_state_not_input_accepting() {
        let mut m = SessionStateMachine::new("k".to_string());
        m.transition(SessionState::Active).unwrap();
        m.transition(SessionState::Compacting).unwrap();
        assert!(!m.can_accept_input());
    }

    #[test]
    fn suspended_state_not_input_accepting() {
        let mut m = SessionStateMachine::new("k".to_string());
        m.transition(SessionState::Active).unwrap();
        m.transition(SessionState::Suspended).unwrap();
        assert!(!m.can_accept_input());
    }

    #[test]
    fn closed_state_not_input_accepting() {
        let mut m = SessionStateMachine::new("k".to_string());
        m.transition(SessionState::Closed).unwrap();
        assert!(!m.can_accept_input());
    }

    #[test]
    fn transition_count_not_incremented_on_failure() {
        let mut m = SessionStateMachine::new("k".to_string());
        // Attempt invalid transitions — count must stay 0.
        let _ = m.transition(SessionState::Idle);
        let _ = m.transition(SessionState::Suspended);
        assert_eq!(m.transition_count(), 0);
        // Now do one valid transition.
        m.transition(SessionState::Active).unwrap();
        assert_eq!(m.transition_count(), 1);
    }

    #[test]
    fn serde_state_snake_case() {
        // Verify JSON representation uses snake_case per `#[serde(rename_all = "snake_case")]`.
        assert_eq!(serde_json::to_string(&SessionState::Created).unwrap(), "\"created\"");
        assert_eq!(serde_json::to_string(&SessionState::Active).unwrap(), "\"active\"");
        assert_eq!(serde_json::to_string(&SessionState::Idle).unwrap(), "\"idle\"");
        assert_eq!(serde_json::to_string(&SessionState::Suspended).unwrap(), "\"suspended\"");
        assert_eq!(serde_json::to_string(&SessionState::Compacting).unwrap(), "\"compacting\"");
        assert_eq!(serde_json::to_string(&SessionState::Closed).unwrap(), "\"closed\"");
    }
}
