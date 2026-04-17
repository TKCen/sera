//! Agent runtime trait and associated types.
//!
//! Defines the `AgentRuntime` pluggable interface per SPEC-runtime §2.
//! Both the default runtime and external (gRPC) runtimes implement this trait.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::evolution::ChangeArtifactId;

// ── Input types ──────────────────────────────────────────────────────────────

/// Context passed to the runtime for a single agent turn.
///
/// Contains everything the runtime needs: event identity, session history,
/// available tools, and arbitrary metadata (e.g., model override, sampler
/// profile, hook-injected state).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnContext {
    /// The event ID that triggered this turn.
    pub event_id: String,
    /// The agent instance being executed.
    pub agent_id: String,
    /// The session key scoping this turn.
    pub session_key: String,
    /// Conversation history (OpenAI-format message objects).
    pub messages: Vec<serde_json::Value>,
    /// Tool schemas available to the model during this turn.
    pub available_tools: Vec<crate::tool::ToolDefinition>,
    /// Arbitrary metadata: model overrides, hook-injected state, etc.
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    /// The change artifact this turn is associated with, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_artifact: Option<ChangeArtifactId>,
    /// Parent session key — set when this agent was spawned as a child of another session.
    ///
    /// Propagated from the gateway submission through all frames so observers can
    /// reconstruct the session hierarchy. `None` for root sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_key: Option<String>,
    /// Tool selection policy for this turn (SPEC-runtime §6.3).
    ///
    /// Controls how the LLM chooses among available tools. Defaults to `Auto`
    /// (model decides freely). The `OnLlmStart` hook may override this field
    /// before the model call to enforce per-turn policy gates.
    #[serde(default)]
    pub tool_use_behavior: crate::tool::ToolUseBehavior,
}

// ── Output types ─────────────────────────────────────────────────────────────

/// A single tool call made by the model during a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// The call ID assigned by the model.
    pub id: String,
    /// Name of the tool that was called.
    pub name: String,
    /// Arguments passed by the model (JSON object).
    pub arguments: serde_json::Value,
    /// The result of executing the tool, if it was executed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<crate::tool::ToolResult>,
}

/// Token usage for a turn.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl TokenUsage {
    /// Compute total from prompt + completion (ignores stored `total_tokens`).
    pub fn computed_total(&self) -> u32 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// The outcome of a completed agent turn (SPEC-runtime §2.3).
/// Replaces `TurnResult` in the design-forward contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TurnOutcome {
    RunAgain {
        tool_calls: Vec<ToolCall>,
        tokens_used: TokenUsage,
        duration_ms: u64,
    },
    Handoff {
        target_agent_id: String,
        context: serde_json::Value,
        tokens_used: TokenUsage,
        duration_ms: u64,
    },
    FinalOutput {
        response: String,
        tool_calls: Vec<ToolCall>,
        tokens_used: TokenUsage,
        duration_ms: u64,
        /// Accumulated conversation messages during the tool-call loop (assistant + tool results).
        /// Used by the gateway to persist full turn history to the session transcript.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        transcript: Vec<serde_json::Value>,
    },
    Compact {
        tokens_used: TokenUsage,
        duration_ms: u64,
    },
    Interruption {
        hook_point: String,
        reason: String,
        duration_ms: u64,
    },
    /// The turn is paused waiting for human/agent approval of a tool call.
    WaitingForApproval {
        /// The tool call that requires approval.
        tool_call: serde_json::Value,
        /// Approval ticket ID for tracking.
        ticket_id: String,
        tokens_used: TokenUsage,
        duration_ms: u64,
    },
    Stop {
        summary: String,
        tokens_used: TokenUsage,
        duration_ms: u64,
    },
}

// ── Capabilities ─────────────────────────────────────────────────────────────

/// What the runtime supports — reported via `AgentRuntime::capabilities()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    /// Whether the runtime can stream partial responses.
    pub supports_streaming: bool,
    /// Whether the runtime can execute tool calls.
    pub supports_tool_calls: bool,
    /// Whether the runtime supports constrained/structured output.
    pub supports_structured_output: bool,
    /// Maximum context window in tokens, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<u32>,
}

impl Default for RuntimeCapabilities {
    fn default() -> Self {
        Self {
            supports_streaming: false,
            supports_tool_calls: true,
            supports_structured_output: false,
            max_context_tokens: None,
        }
    }
}

// ── Health ────────────────────────────────────────────────────────────────────

/// Liveness/readiness of a runtime — reported via `AgentRuntime::health()`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "message", rename_all = "snake_case")]
pub enum HealthStatus {
    /// Runtime is operating normally.
    Healthy,
    /// Runtime is operational but experiencing issues (detail in message).
    Degraded(String),
    /// Runtime is not operational (detail in message).
    Unhealthy(String),
}

// ── Errors ────────────────────────────────────────────────────────────────────

/// Errors that can occur during runtime execution.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RuntimeError {
    /// The underlying model returned an error.
    #[error("model error: {0}")]
    ModelError(String),

    /// The assembled context exceeds the model's context window.
    #[error("context overflow: limit {limit}, actual {actual}")]
    ContextOverflow { limit: u32, actual: u32 },

    /// A tool call failed during execution.
    #[error("tool execution failed: tool={tool}, reason={reason}")]
    ToolExecutionFailed { tool: String, reason: String },

    /// The turn exceeded its time budget.
    #[error("turn timed out")]
    Timeout,

    /// An unexpected internal error.
    #[error("internal error: {0}")]
    Internal(String),
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Pluggable agent runtime interface (SPEC-runtime §2).
///
/// Implementors:
/// - Default runtime (built-in pipeline: context assembly → model call → tool loop)
/// - External runtime adapter (gRPC bridge to `AgentRuntimeService`)
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    /// Execute one complete agent turn.
    ///
    /// Responsible for: context assembly, model call, tool call loop,
    /// memory write, and response delivery.
    async fn execute_turn(&self, ctx: TurnContext) -> Result<TurnOutcome, RuntimeError>;

    /// Report what this runtime supports.
    async fn capabilities(&self) -> RuntimeCapabilities;

    /// Report liveness / readiness of this runtime.
    async fn health(&self) -> HealthStatus;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_turn_context() -> TurnContext {
        TurnContext {
            event_id: "evt-001".to_string(),
            agent_id: "agent-sera".to_string(),
            session_key: "session:agent-sera:user-42".to_string(),
            messages: vec![serde_json::json!({"role": "user", "content": "Hello"})],
            available_tools: vec![],
            metadata: HashMap::new(),
            change_artifact: None,
            parent_session_key: None,
            tool_use_behavior: crate::tool::ToolUseBehavior::default(),
        }
    }

    #[test]
    fn turn_context_construction() {
        let ctx = make_turn_context();
        assert_eq!(ctx.event_id, "evt-001");
        assert_eq!(ctx.agent_id, "agent-sera");
        assert_eq!(ctx.session_key, "session:agent-sera:user-42");
        assert_eq!(ctx.messages.len(), 1);
        assert!(ctx.available_tools.is_empty());
        assert!(ctx.metadata.is_empty());
    }

    #[test]
    fn turn_context_with_metadata() {
        let mut ctx = make_turn_context();
        ctx.metadata.insert(
            "model_override".to_string(),
            serde_json::json!("gemini-2.5-pro"),
        );
        assert_eq!(
            ctx.metadata.get("model_override"),
            Some(&serde_json::json!("gemini-2.5-pro"))
        );
    }

    #[test]
    fn token_usage_total_calculation() {
        let usage = TokenUsage {
            prompt_tokens: 300,
            completion_tokens: 150,
            total_tokens: 450,
        };
        assert_eq!(usage.total_tokens, 450);
        assert_eq!(usage.computed_total(), 450);
    }

    #[test]
    fn token_usage_computed_total_independent() {
        // computed_total() ignores stored total_tokens
        let usage = TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 999, // intentionally wrong stored value
        };
        assert_eq!(usage.computed_total(), 150);
    }

    #[test]
    fn runtime_capabilities_defaults() {
        let caps = RuntimeCapabilities::default();
        assert!(!caps.supports_streaming);
        assert!(caps.supports_tool_calls);
        assert!(!caps.supports_structured_output);
        assert!(caps.max_context_tokens.is_none());
    }

    #[test]
    fn runtime_capabilities_full() {
        let caps = RuntimeCapabilities {
            supports_streaming: true,
            supports_tool_calls: true,
            supports_structured_output: true,
            max_context_tokens: Some(128_000),
        };
        assert!(caps.supports_streaming);
        assert_eq!(caps.max_context_tokens, Some(128_000));
    }

    #[test]
    fn health_status_variants() {
        let healthy = HealthStatus::Healthy;
        let degraded = HealthStatus::Degraded("high latency".to_string());
        let unhealthy = HealthStatus::Unhealthy("connection refused".to_string());

        assert_eq!(healthy, HealthStatus::Healthy);
        assert_ne!(healthy, degraded);
        assert_ne!(healthy, unhealthy);

        if let HealthStatus::Degraded(msg) = &degraded {
            assert_eq!(msg, "high latency");
        } else {
            panic!("expected Degraded variant");
        }

        if let HealthStatus::Unhealthy(msg) = &unhealthy {
            assert_eq!(msg, "connection refused");
        } else {
            panic!("expected Unhealthy variant");
        }
    }

    #[test]
    fn runtime_error_display() {
        let e = RuntimeError::ModelError("quota exceeded".to_string());
        assert_eq!(e.to_string(), "model error: quota exceeded");

        let e = RuntimeError::ContextOverflow { limit: 4096, actual: 5000 };
        assert_eq!(e.to_string(), "context overflow: limit 4096, actual 5000");

        let e = RuntimeError::ToolExecutionFailed {
            tool: "shell".to_string(),
            reason: "permission denied".to_string(),
        };
        assert_eq!(
            e.to_string(),
            "tool execution failed: tool=shell, reason=permission denied"
        );

        let e = RuntimeError::Timeout;
        assert_eq!(e.to_string(), "turn timed out");

        let e = RuntimeError::Internal("unexpected panic".to_string());
        assert_eq!(e.to_string(), "internal error: unexpected panic");
    }

    #[test]
    fn turn_context_serde_roundtrip() {
        let ctx = make_turn_context();
        let json = serde_json::to_string(&ctx).unwrap();
        let parsed: TurnContext = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event_id, ctx.event_id);
        assert_eq!(parsed.agent_id, ctx.agent_id);
        assert_eq!(parsed.session_key, ctx.session_key);
        assert_eq!(parsed.messages.len(), ctx.messages.len());
    }

    #[test]
    fn turn_context_parent_session_key_propagation() {
        let mut ctx = make_turn_context();
        ctx.parent_session_key = Some("parent-sess-abc".to_string());

        let json = serde_json::to_string(&ctx).unwrap();
        assert!(json.contains("parent_session_key"));
        assert!(json.contains("parent-sess-abc"));

        let parsed: TurnContext = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.parent_session_key.as_deref(), Some("parent-sess-abc"));
    }

    #[test]
    fn turn_context_parent_session_key_absent_when_none() {
        let ctx = make_turn_context(); // parent_session_key: None
        let json = serde_json::to_string(&ctx).unwrap();
        // skip_serializing_if = "Option::is_none" → field omitted from JSON
        assert!(!json.contains("parent_session_key"));

        // Legacy frames without the field still parse cleanly
        let legacy = r#"{"event_id":"e1","agent_id":"a1","session_key":"s1","messages":[],"available_tools":[]}"#;
        let parsed: TurnContext = serde_json::from_str(legacy).unwrap();
        assert!(parsed.parent_session_key.is_none());
    }

    #[test]
    fn turn_context_tool_use_behavior_defaults_to_auto() {
        let ctx = make_turn_context();
        assert_eq!(ctx.tool_use_behavior, crate::tool::ToolUseBehavior::Auto);
    }

    #[test]
    fn turn_context_tool_use_behavior_serde_roundtrip() {
        let mut ctx = make_turn_context();
        ctx.tool_use_behavior = crate::tool::ToolUseBehavior::Specific {
            name: "read_file".to_string(),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        let parsed: TurnContext = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tool_use_behavior, ctx.tool_use_behavior);
    }

    #[test]
    fn turn_context_tool_use_behavior_default_from_legacy_json() {
        // Frames without tool_use_behavior field deserialize as Auto.
        let legacy = r#"{"event_id":"e1","agent_id":"a1","session_key":"s1","messages":[],"available_tools":[]}"#;
        let parsed: TurnContext = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.tool_use_behavior, crate::tool::ToolUseBehavior::Auto);
    }

}
