//! Subagent management — spawning, tracking, and cancelling child agents.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

// ── Status types ──────────────────────────────────────────────────────────────

/// Lifecycle status of a subagent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    /// The subagent process is being initialised.
    Spawning,
    /// The subagent is actively processing its task.
    Running,
    /// The subagent finished successfully.
    Completed,
    /// The subagent terminated with an error.
    Failed,
    /// The subagent was cancelled before it could finish.
    Cancelled,
}

/// Output produced by a subagent that completed successfully.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentResult {
    /// The agent that produced this result.
    pub agent_id: String,
    /// The session key under which the agent ran.
    pub session_key: String,
    /// Agent output as a JSON value.
    pub output: serde_json::Value,
    /// Final status (should be [`SubagentStatus::Completed`] for a result).
    pub status: SubagentStatus,
}

// ── Handle ────────────────────────────────────────────────────────────────────

/// Handle to a running subagent.
///
/// The handle provides access to the agent's identity and a channel for
/// observing status transitions.
pub struct SubagentHandle {
    /// Stable agent identifier.
    pub agent_id: String,
    /// Session key for this particular invocation.
    pub session_key: String,
    /// Receiver side of the status watch channel.
    ///
    /// Callers can `.await` status transitions via
    /// [`watch::Receiver::changed`].
    pub status_rx: watch::Receiver<SubagentStatus>,
}

impl SubagentHandle {
    /// Create a new handle with the given identity and status channel.
    pub fn new(
        agent_id: impl Into<String>,
        session_key: impl Into<String>,
        status_rx: watch::Receiver<SubagentStatus>,
    ) -> Self {
        Self {
            agent_id: agent_id.into(),
            session_key: session_key.into(),
            status_rx,
        }
    }

    /// Return the current status without blocking.
    pub fn current_status(&self) -> SubagentStatus {
        self.status_rx.borrow().clone()
    }
}

// ── Manager trait ─────────────────────────────────────────────────────────────

/// Errors returned by subagent management operations.
#[derive(Debug, thiserror::Error)]
pub enum SubagentError {
    /// The requested agent ID was not found in the registry.
    #[error("subagent '{agent_id}' not found")]
    NotFound { agent_id: String },
    /// The subagent could not be spawned.
    #[error("failed to spawn subagent '{agent_id}': {reason}")]
    SpawnFailed { agent_id: String, reason: String },
    /// The operation was attempted on an agent that is not active.
    #[error("subagent '{agent_id}' is not active (status: {status:?})")]
    NotActive {
        agent_id: String,
        status: SubagentStatus,
    },
    /// An internal or transport error occurred.
    #[error("subagent error: {0}")]
    Internal(#[from] anyhow::Error),
}

/// Async trait for managing subagent lifecycles.
#[async_trait]
pub trait SubagentManager: Send + Sync {
    /// Spawn a new subagent and return a handle to it.
    ///
    /// `agent_id` identifies the agent template; `session_key` scopes the
    /// invocation.  `input` is arbitrary task context forwarded to the agent.
    async fn spawn(
        &self,
        agent_id: &str,
        session_key: &str,
        input: serde_json::Value,
    ) -> Result<SubagentHandle, SubagentError>;

    /// Return the current status of a subagent identified by `session_key`.
    async fn status(&self, session_key: &str) -> Result<SubagentStatus, SubagentError>;

    /// Cancel a running subagent.  Returns `Ok(())` if the cancellation signal
    /// was delivered, regardless of whether the agent has finished yet.
    async fn cancel(&self, session_key: &str) -> Result<(), SubagentError>;

    /// List the session keys of all currently active subagents.
    async fn list_active(&self) -> Vec<String>;
}
