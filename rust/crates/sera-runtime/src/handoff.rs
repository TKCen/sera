//! Handoff — first-class agent-to-agent handoff as a tool, plus delegation protocol types.

use std::collections::HashSet;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ── Existing handoff types ────────────────────────────────────────────────────

/// Handoff input filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffInputFilter {
    None,
    RemoveAllTools,
}

/// Handoff input data passed to the handoff callback.
#[derive(Debug, Clone)]
pub struct HandoffInputData {
    pub input_history: Vec<serde_json::Value>,
    pub pre_handoff_items: Vec<serde_json::Value>,
    pub new_items: Vec<serde_json::Value>,
}

/// Handoff definition — a tool that transfers control to another agent.
#[derive(Clone)]
pub struct Handoff {
    pub tool_name: String,
    pub tool_description: String,
    pub input_json_schema: serde_json::Value,
    pub input_filter: Option<HandoffInputFilter>,
}

impl Handoff {
    /// Convert this handoff into a tool definition for the LLM.
    pub fn as_tool_definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.tool_name,
                "description": self.tool_description,
                "parameters": self.input_json_schema,
            }
        })
    }
}

// ── Delegation protocol types ─────────────────────────────────────────────────

/// A request to delegate a task to another agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationRequest {
    /// ID of the target agent to delegate to.
    pub target_agent_id: String,
    /// Human-readable description of the task being delegated.
    pub task_description: String,
    /// Arbitrary context data passed to the target agent.
    pub context: serde_json::Value,
    /// Input filter applied before passing history to the target agent.
    pub input_filter: Option<HandoffInputFilter>,
    /// Maximum time to wait for the delegated agent to complete.
    #[serde(with = "humantime_serde_opt", default)]
    pub timeout: Option<Duration>,
    /// Current delegation depth (incremented on each nested delegation).
    pub depth: u32,
}

/// Result returned by a delegated agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DelegationResponse {
    /// The delegated agent completed successfully.
    Success {
        /// Agent output as a JSON value.
        output: serde_json::Value,
        /// ID of the agent session that produced this result.
        session_id: String,
    },
    /// The delegated agent failed with an error.
    Error {
        /// Error message from the delegated agent.
        message: String,
        /// Optional structured error detail.
        detail: Option<serde_json::Value>,
    },
    /// The delegation timed out before the agent responded.
    Timeout {
        /// Timeout duration that elapsed.
        elapsed_secs: f64,
    },
}

/// Configuration controlling delegation behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationConfig {
    /// Maximum allowed delegation depth. Requests exceeding this depth are rejected.
    /// Defaults to [`DelegationConfig::DEFAULT_MAX_DEPTH`].
    pub max_depth: u32,
    /// Default timeout applied to delegations that do not specify one.
    #[serde(with = "humantime_serde_opt", default)]
    pub default_timeout: Option<Duration>,
    /// Explicit set of agent IDs that may be delegated to.
    /// When `None`, all agents are allowed (subject to other policy).
    pub allowed_targets: Option<HashSet<String>>,
}

impl DelegationConfig {
    /// Default maximum delegation depth.
    pub const DEFAULT_MAX_DEPTH: u32 = 5;
}

impl Default for DelegationConfig {
    fn default() -> Self {
        Self {
            max_depth: Self::DEFAULT_MAX_DEPTH,
            default_timeout: Some(Duration::from_secs(crate::llm_client::DEFAULT_LLM_TIMEOUT_SECS)),
            allowed_targets: None,
        }
    }
}

/// Errors that can occur during delegation.
#[derive(Debug, thiserror::Error)]
pub enum DelegationError {
    /// The requested delegation would exceed the maximum allowed depth.
    #[error("delegation depth {depth} exceeds maximum {max_depth}")]
    MaxDepthExceeded {
        depth: u32,
        max_depth: u32,
    },
    /// The target agent is not in the allowed-targets list.
    #[error("delegation target '{target}' is not in the allowed-targets list")]
    TargetNotAllowed {
        target: String,
    },
    /// The target agent was not found.
    #[error("delegation target agent '{target}' not found")]
    TargetNotFound {
        target: String,
    },
    /// The delegation timed out.
    #[error("delegation to '{target}' timed out after {elapsed_secs:.1}s")]
    Timeout {
        target: String,
        elapsed_secs: f64,
    },
    /// An underlying transport or serialisation error occurred.
    #[error("delegation transport error: {0}")]
    Transport(#[from] anyhow::Error),
}

/// Async trait for executing agent-to-agent delegations.
#[async_trait]
pub trait DelegationProtocol: Send + Sync {
    /// Delegate a task to another agent and await its response.
    async fn delegate(
        &self,
        request: DelegationRequest,
    ) -> Result<DelegationResponse, DelegationError>;

    /// Return `true` if this agent is permitted to delegate to `target_agent_id`.
    async fn can_delegate_to(&self, target_agent_id: &str) -> bool;

    /// List the agent IDs available for delegation from this context.
    async fn list_available_agents(&self) -> Vec<String>;
}

// ── Private serde helper for Option<Duration> ─────────────────────────────────

mod humantime_serde_opt {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        value: &Option<Duration>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(d) => serializer.serialize_some(&d.as_secs_f64()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Duration>, D::Error> {
        let opt: Option<f64> = Option::deserialize(deserializer)?;
        Ok(opt.map(Duration::from_secs_f64))
    }
}
