//! Subagent handle — stub for spawning child agents.

/// Handle to a running subagent.
pub struct SubagentHandle {
    pub agent_id: String,
    pub session_key: String,
}

/// Spawn a subagent. Returns NotImplemented in P0.
pub fn spawn_subagent(
    _agent_id: &str,
    _session_key: &str,
) -> Result<SubagentHandle, SubagentError> {
    Err(SubagentError::NotImplemented)
}

/// Subagent errors.
#[derive(Debug, thiserror::Error)]
pub enum SubagentError {
    #[error("subagent spawning not implemented in Phase 0")]
    NotImplemented,
}
