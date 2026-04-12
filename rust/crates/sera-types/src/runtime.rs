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

/// The result of a completed agent turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnResult {
    /// The final text response from the model.
    pub response: String,
    /// All tool calls made during the turn (in order).
    pub tool_calls: Vec<ToolCall>,
    /// Token usage for the entire turn (may span multiple model calls).
    pub tokens_used: TokenUsage,
    /// Wall-clock duration of the turn in milliseconds.
    pub duration_ms: u64,
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
    async fn execute_turn(&self, ctx: TurnContext) -> Result<TurnResult, RuntimeError>;

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
    fn turn_result_with_tool_calls() {
        let result = TurnResult {
            response: "I ran the tool for you.".to_string(),
            tool_calls: vec![ToolCall {
                id: "call-abc".to_string(),
                name: "memory_read".to_string(),
                arguments: serde_json::json!({"path": "notes.md"}),
                result: Some(crate::tool::ToolResult::success("# Notes\nSome content")),
            }],
            tokens_used: TokenUsage {
                prompt_tokens: 120,
                completion_tokens: 40,
                total_tokens: 160,
            },
            duration_ms: 512,
        };
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].name, "memory_read");
        assert!(result.tool_calls[0].result.is_some());
        assert!(!result.tool_calls[0].result.as_ref().unwrap().is_error);
        assert_eq!(result.duration_ms, 512);
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
    fn turn_result_serde_roundtrip() {
        let result = TurnResult {
            response: "Done.".to_string(),
            tool_calls: vec![],
            tokens_used: TokenUsage {
                prompt_tokens: 50,
                completion_tokens: 10,
                total_tokens: 60,
            },
            duration_ms: 200,
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: TurnResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.response, "Done.");
        assert_eq!(parsed.tokens_used.prompt_tokens, 50);
        assert_eq!(parsed.duration_ms, 200);
    }
}
