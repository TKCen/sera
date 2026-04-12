//! Four-method turn lifecycle — _observe, _think, _act, _react.

use std::collections::HashSet;
use uuid::Uuid;

use async_trait::async_trait;
use sera_types::runtime::{TokenUsage, TurnOutcome};

use crate::handoff::Handoff;

/// Doom loop threshold — triggers Interruption after this many consecutive act cycles.
pub const DOOM_LOOP_THRESHOLD: u32 = 3;

/// React mode for the think step.
#[derive(Debug, Clone)]
pub enum ReactMode {
    /// Default mode — model decides.
    Default,
    /// Deterministic ordering (P0 stub).
    ByOrder,
}

// ── LlmProvider trait ────────────────────────────────────────────────────────

/// Errors from the LLM provider.
#[derive(Debug, thiserror::Error)]
pub enum ThinkError {
    #[error("LLM call failed: {0}")]
    Llm(String),
    #[error("type conversion error: {0}")]
    Conversion(String),
}

/// Trait for calling an LLM from the think step.
///
/// Messages and tools use `serde_json::Value` to stay decoupled from any
/// specific provider's wire types. Implementations convert internally.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(
        &self,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
    ) -> Result<ThinkResult, ThinkError>;
}

// ── Turn context ─────────────────────────────────────────────────────────────

/// Turn context for the four-method lifecycle.
pub struct TurnContext {
    pub turn_id: Uuid,
    pub session_key: String,
    pub agent_id: String,
    pub messages: Vec<serde_json::Value>,
    pub tools: Vec<serde_json::Value>,
    pub handoffs: Vec<Handoff>,
    pub watch_signals: HashSet<String>,
    pub change_artifact: Option<String>,
    pub react_mode: ReactMode,
    pub doom_loop_count: u32,
}

/// Observe — filter messages by watch signals.
pub fn observe(ctx: &TurnContext) -> Vec<serde_json::Value> {
    // P0: return all messages (filtering by cause_by is P1)
    ctx.messages.clone()
}

/// Think — call the LLM via the provided `LlmProvider`.
///
/// Falls back to a stub response when no provider is given (useful for tests).
pub async fn think(
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    _react_mode: &ReactMode,
    llm: Option<&dyn LlmProvider>,
) -> ThinkResult {
    match llm {
        Some(provider) => match provider.chat(messages, tools).await {
            Ok(result) => result,
            Err(e) => {
                tracing::error!("LLM call failed in think step: {e}");
                ThinkResult {
                    response: serde_json::json!({"role": "assistant", "content": format!("[LLM error: {e}]")}),
                    tool_calls: vec![],
                    tokens: TokenUsage::default(),
                }
            }
        },
        None => ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "[think stub]"}),
            tool_calls: vec![],
            tokens: TokenUsage::default(),
        },
    }
}

/// Result of the think step.
pub struct ThinkResult {
    pub response: serde_json::Value,
    pub tool_calls: Vec<serde_json::Value>,
    pub tokens: TokenUsage,
}

/// Act — dispatch tool calls, check for handoffs, doom-loop detection.
pub fn act(ctx: &TurnContext, think_result: &ThinkResult) -> ActResult {
    // Doom loop check
    if ctx.doom_loop_count >= DOOM_LOOP_THRESHOLD {
        return ActResult::Interruption {
            reason: format!(
                "doom loop: {} consecutive act cycles",
                ctx.doom_loop_count
            ),
        };
    }

    // Check for handoff tool calls
    for tc in &think_result.tool_calls {
        if let Some(name) = tc
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
            && ctx.handoffs.iter().any(|h| h.tool_name == name)
        {
            return ActResult::Handoff {
                target: name.to_string(),
                context: tc.clone(),
            };
        }
    }

    // Normal tool dispatch (P0 stub — returns empty results)
    ActResult::ToolResults(vec![])
}

/// Result of the act step.
pub enum ActResult {
    ToolResults(Vec<serde_json::Value>),
    Handoff {
        target: String,
        context: serde_json::Value,
    },
    Interruption {
        reason: String,
    },
}

/// React — decide what to do next based on tool results.
pub fn react(act_result: &ActResult, tokens: &TokenUsage, elapsed_ms: u64) -> TurnOutcome {
    match act_result {
        ActResult::ToolResults(results) => {
            if results.is_empty() {
                TurnOutcome::FinalOutput {
                    response: "[react stub — no tool calls]".to_string(),
                    tool_calls: vec![],
                    tokens_used: tokens.clone(),
                    duration_ms: elapsed_ms,
                }
            } else {
                TurnOutcome::RunAgain {
                    tool_calls: vec![],
                    tokens_used: tokens.clone(),
                    duration_ms: elapsed_ms,
                }
            }
        }
        ActResult::Handoff { target, context } => TurnOutcome::Handoff {
            target_agent_id: target.clone(),
            context: context.clone(),
            tokens_used: tokens.clone(),
            duration_ms: elapsed_ms,
        },
        ActResult::Interruption { reason } => TurnOutcome::Interruption {
            hook_point: "doom_loop".to_string(),
            reason: reason.clone(),
            duration_ms: elapsed_ms,
        },
    }
}
