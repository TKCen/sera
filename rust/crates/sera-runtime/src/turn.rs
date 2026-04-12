//! Four-method turn lifecycle — _observe, _think, _act, _react.

use std::collections::HashSet;
use uuid::Uuid;

use async_trait::async_trait;
use sera_hitl;
use sera_hooks::ChainExecutor;
use sera_types::hook::{HookChain, HookContext, HookPoint, HookResult};
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
    pub enforcement_mode: sera_hitl::EnforcementMode,
    pub approval_routing: sera_hitl::ApprovalRouting,
}

/// Observe — filter messages by watch signals and run ConstitutionalGate hooks on input.
///
/// Returns `Ok(messages)` when hooks allow the turn to proceed, or
/// `Err(TurnOutcome::Interruption)` when a hook rejects the incoming messages.
pub async fn observe(
    ctx: &TurnContext,
    executor: Option<&ChainExecutor>,
    chains: &[HookChain],
) -> Result<Vec<serde_json::Value>, TurnOutcome> {
    // P0: return all messages (filtering by cause_by is P1)
    let messages = ctx.messages.clone();

    if let Some(exec) = executor {
        let hook_ctx = HookContext {
            point: HookPoint::ConstitutionalGate,
            event: Some(serde_json::json!({ "messages": messages })),
            session: Some(serde_json::json!({
                "session_key": ctx.session_key,
                "agent_id": ctx.agent_id,
            })),
            tool_call: None,
            tool_result: None,
            principal: None,
            metadata: std::collections::HashMap::new(),
            change_artifact: None,
        };

        let result = exec
            .execute_at_point(HookPoint::ConstitutionalGate, chains, hook_ctx)
            .await;

        match result {
            Ok(chain_result) => match chain_result.outcome {
                HookResult::Reject { reason, .. } => {
                    return Err(TurnOutcome::Interruption {
                        hook_point: "constitutional_gate".to_string(),
                        reason,
                        duration_ms: 0,
                    });
                }
                HookResult::Continue { updated_input, .. } => {
                    // If a hook modified the input, use the updated messages.
                    if let Some(updated) = updated_input
                        && let Some(arr) = updated.as_array()
                    {
                        return Ok(arr.clone());
                    }
                }
                HookResult::Redirect { target, reason } => {
                    let reason_str = reason.unwrap_or_else(|| format!("redirected to {target}"));
                    return Err(TurnOutcome::Interruption {
                        hook_point: "constitutional_gate".to_string(),
                        reason: reason_str,
                        duration_ms: 0,
                    });
                }
            },
            Err(e) => {
                tracing::warn!("ConstitutionalGate hook error in observe: {e}");
                // Fail-safe: allow the turn to proceed on hook executor error.
            }
        }
    }

    Ok(messages)
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

    // HITL approval check
    for tc in &think_result.tool_calls {
        // Extract tool name and determine risk level
        let tool_name = tc.get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");

        // Default to Execute risk for tool calls (can be refined later with per-tool risk)
        let risk_level = sera_types::tool::RiskLevel::Execute;

        if sera_hitl::ApprovalRouter::needs_approval(
            ctx.enforcement_mode,
            risk_level,
            &ctx.approval_routing,
        ) {
            // Create approval ticket
            let spec = sera_hitl::ApprovalSpec {
                scope: sera_hitl::ApprovalScope::ToolCall {
                    tool_name: tool_name.to_string(),
                    risk_level,
                },
                description: format!("Tool call: {tool_name}"),
                urgency: sera_hitl::ApprovalUrgency::Medium,
                routing: ctx.approval_routing.clone(),
                timeout: std::time::Duration::from_secs(300),
                required_approvals: 1,
                evidence: sera_hitl::ApprovalEvidence {
                    tool_args: tc.get("function").and_then(|f| f.get("arguments")).cloned(),
                    risk_score: Some(sera_hitl::ApprovalRouter::risk_level_to_score_public(risk_level)),
                    principal: sera_types::principal::Principal::default_admin().as_ref(),
                    session_context: Some(ctx.session_key.clone()),
                    additional: std::collections::HashMap::new(),
                },
            };
            let ticket = sera_hitl::ApprovalTicket::new(spec, &ctx.session_key);
            return ActResult::WaitingForApproval {
                tool_call: tc.clone(),
                ticket_id: ticket.id.clone(),
            };
        }
    }

    // Normal tool dispatch (P0 stub — returns empty results)
    ActResult::ToolResults(vec![])
}

/// Result of the act step.
#[derive(Debug)]
pub enum ActResult {
    ToolResults(Vec<serde_json::Value>),
    Handoff {
        target: String,
        context: serde_json::Value,
    },
    Interruption {
        reason: String,
    },
    WaitingForApproval {
        tool_call: serde_json::Value,
        ticket_id: String,
    },
}

/// React — decide what to do next based on tool results, running ConstitutionalGate hooks
/// on the model's final response before emitting.
///
/// When a hook rejects the response, `TurnOutcome::Interruption` is returned instead.
pub async fn react(
    act_result: &ActResult,
    tokens: &TokenUsage,
    elapsed_ms: u64,
    executor: Option<&ChainExecutor>,
    chains: &[HookChain],
) -> TurnOutcome {
    // Build a preliminary outcome from act results.
    let outcome = match act_result {
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
        ActResult::WaitingForApproval { tool_call, ticket_id } => TurnOutcome::WaitingForApproval {
            tool_call: tool_call.clone(),
            ticket_id: ticket_id.clone(),
            tokens_used: tokens.clone(),
            duration_ms: elapsed_ms,
        },
    };

    // Run ConstitutionalGate hooks on FinalOutput responses only.
    if let Some(exec) = executor
        && let TurnOutcome::FinalOutput { ref response, .. } = outcome
    {
        let hook_ctx = HookContext {
            point: HookPoint::ConstitutionalGate,
            event: Some(serde_json::json!({ "response": response })),
            session: None,
            tool_call: None,
            tool_result: None,
            principal: None,
            metadata: std::collections::HashMap::new(),
            change_artifact: None,
        };

        let result = exec
            .execute_at_point(HookPoint::ConstitutionalGate, chains, hook_ctx)
            .await;

        match result {
            Ok(chain_result) => match chain_result.outcome {
                HookResult::Reject { reason, .. } => {
                    return TurnOutcome::Interruption {
                        hook_point: "constitutional_gate".to_string(),
                        reason,
                        duration_ms: elapsed_ms,
                    };
                }
                HookResult::Continue { updated_input, .. } => {
                    // If a hook modified the response, return updated FinalOutput.
                    if let Some(updated) = updated_input
                        && let Some(new_response) = updated.as_str()
                    {
                        return TurnOutcome::FinalOutput {
                            response: new_response.to_string(),
                            tool_calls: vec![],
                            tokens_used: tokens.clone(),
                            duration_ms: elapsed_ms,
                        };
                    }
                }
                HookResult::Redirect { target, reason } => {
                    let reason_str =
                        reason.unwrap_or_else(|| format!("redirected to {target}"));
                    return TurnOutcome::Interruption {
                        hook_point: "constitutional_gate".to_string(),
                        reason: reason_str,
                        duration_ms: elapsed_ms,
                    };
                }
            },
            Err(e) => {
                tracing::warn!("ConstitutionalGate hook error in react: {e}");
                // Fail-safe: emit original outcome on hook executor error.
            }
        }
    }

    outcome
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sera_hooks::{ChainExecutor, HookRegistry};
    use sera_types::hook::{HookChain, HookContext, HookInstance, HookMetadata, HookPoint, HookResult};
    use sera_types::runtime::TokenUsage;

    use super::*;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_turn_ctx(messages: Vec<serde_json::Value>) -> TurnContext {
        TurnContext {
            turn_id: uuid::Uuid::new_v4(),
            session_key: "sess-test".into(),
            agent_id: "agent-test".into(),
            messages,
            tools: vec![],
            handoffs: vec![],
            watch_signals: HashSet::new(),
            change_artifact: None,
            react_mode: ReactMode::Default,
            doom_loop_count: 0,
            enforcement_mode: sera_hitl::EnforcementMode::Autonomous,
            approval_routing: sera_hitl::ApprovalRouting::Autonomous,
        }
    }

    /// A hook that always rejects with a fixed reason.
    struct AlwaysRejectHook {
        reason: String,
    }

    #[async_trait::async_trait]
    impl sera_hooks::Hook for AlwaysRejectHook {
        fn metadata(&self) -> HookMetadata {
            HookMetadata {
                name: "always-reject".into(),
                description: "Rejects every call".into(),
                version: "1.0.0".into(),
                supported_points: vec![HookPoint::ConstitutionalGate],
                author: None,
            }
        }

        async fn init(&mut self, _config: serde_json::Value) -> Result<(), sera_hooks::HookError> {
            Ok(())
        }

        async fn execute(
            &self,
            _ctx: &HookContext,
        ) -> Result<HookResult, sera_hooks::HookError> {
            Ok(HookResult::reject(self.reason.clone()))
        }
    }

    /// A hook that always passes through unchanged.
    struct AlwaysAllowHook;

    #[async_trait::async_trait]
    impl sera_hooks::Hook for AlwaysAllowHook {
        fn metadata(&self) -> HookMetadata {
            HookMetadata {
                name: "always-allow".into(),
                description: "Allows every call".into(),
                version: "1.0.0".into(),
                supported_points: vec![HookPoint::ConstitutionalGate],
                author: None,
            }
        }

        async fn init(&mut self, _config: serde_json::Value) -> Result<(), sera_hooks::HookError> {
            Ok(())
        }

        async fn execute(
            &self,
            _ctx: &HookContext,
        ) -> Result<HookResult, sera_hooks::HookError> {
            Ok(HookResult::pass())
        }
    }

    fn make_chain(hook_ref: &str) -> HookChain {
        HookChain {
            name: "constitutional-gate-chain".into(),
            point: HookPoint::ConstitutionalGate,
            hooks: vec![HookInstance {
                hook_ref: hook_ref.into(),
                config: serde_json::Value::Null,
                enabled: true,
            }],
            timeout_ms: 5000,
            fail_open: false,
        }
    }

    fn make_reject_executor() -> ChainExecutor {
        let mut registry = HookRegistry::new();
        registry.register(Box::new(AlwaysRejectHook {
            reason: "constitutional violation".into(),
        }));
        ChainExecutor::new(Arc::new(registry))
    }

    fn make_allow_executor() -> ChainExecutor {
        let mut registry = HookRegistry::new();
        registry.register(Box::new(AlwaysAllowHook));
        ChainExecutor::new(Arc::new(registry))
    }

    // ── observe() tests ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn observe_no_hooks_passes_through() {
        let ctx = make_turn_ctx(vec![
            serde_json::json!({"role": "user", "content": "hello"}),
        ]);
        let msgs = observe(&ctx, None, &[]).await.unwrap();
        assert_eq!(msgs.len(), 1);
    }

    #[tokio::test]
    async fn observe_allow_hook_passes_through() {
        let ctx = make_turn_ctx(vec![
            serde_json::json!({"role": "user", "content": "hello"}),
        ]);
        let exec = make_allow_executor();
        let chain = make_chain("always-allow");
        let msgs = observe(&ctx, Some(&exec), &[chain]).await.unwrap();
        assert_eq!(msgs.len(), 1);
    }

    #[tokio::test]
    async fn observe_reject_hook_returns_interruption() {
        let ctx = make_turn_ctx(vec![
            serde_json::json!({"role": "user", "content": "harmful content"}),
        ]);
        let exec = make_reject_executor();
        let chain = make_chain("always-reject");
        let result = observe(&ctx, Some(&exec), &[chain]).await;
        match result {
            Err(TurnOutcome::Interruption { hook_point, reason, .. }) => {
                assert_eq!(hook_point, "constitutional_gate");
                assert_eq!(reason, "constitutional violation");
            }
            other => panic!("expected Err(Interruption), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn observe_no_matching_chains_passes_through() {
        // Chain targets a different hook point — should not fire.
        let ctx = make_turn_ctx(vec![
            serde_json::json!({"role": "user", "content": "hello"}),
        ]);
        let exec = make_reject_executor();
        // Supply a chain for PreRoute, not ConstitutionalGate.
        let non_matching_chain = HookChain {
            name: "pre-route-chain".into(),
            point: HookPoint::PreRoute,
            hooks: vec![HookInstance {
                hook_ref: "always-reject".into(),
                config: serde_json::Value::Null,
                enabled: true,
            }],
            timeout_ms: 5000,
            fail_open: false,
        };
        let msgs = observe(&ctx, Some(&exec), &[non_matching_chain]).await.unwrap();
        assert_eq!(msgs.len(), 1);
    }

    // ── react() tests ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn react_no_hooks_returns_final_output() {
        let act = ActResult::ToolResults(vec![]);
        let tokens = TokenUsage::default();
        let outcome = react(&act, &tokens, 10, None, &[]).await;
        match outcome {
            TurnOutcome::FinalOutput { response, .. } => {
                assert_eq!(response, "[react stub — no tool calls]");
            }
            other => panic!("expected FinalOutput, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn react_allow_hook_passes_final_output_through() {
        let act = ActResult::ToolResults(vec![]);
        let tokens = TokenUsage::default();
        let exec = make_allow_executor();
        let chain = make_chain("always-allow");
        let outcome = react(&act, &tokens, 10, Some(&exec), &[chain]).await;
        match outcome {
            TurnOutcome::FinalOutput { response, .. } => {
                assert_eq!(response, "[react stub — no tool calls]");
            }
            other => panic!("expected FinalOutput, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn react_reject_hook_returns_interruption() {
        let act = ActResult::ToolResults(vec![]);
        let tokens = TokenUsage::default();
        let exec = make_reject_executor();
        let chain = make_chain("always-reject");
        let outcome = react(&act, &tokens, 10, Some(&exec), &[chain]).await;
        match outcome {
            TurnOutcome::Interruption { hook_point, reason, .. } => {
                assert_eq!(hook_point, "constitutional_gate");
                assert_eq!(reason, "constitutional violation");
            }
            other => panic!("expected Interruption, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn react_reject_hook_does_not_fire_on_run_again() {
        // ConstitutionalGate only fires on FinalOutput; RunAgain should pass through.
        let act = ActResult::ToolResults(vec![serde_json::json!({"tool": "result"})]);
        let tokens = TokenUsage::default();
        let exec = make_reject_executor();
        let chain = make_chain("always-reject");
        let outcome = react(&act, &tokens, 10, Some(&exec), &[chain]).await;
        match outcome {
            TurnOutcome::RunAgain { .. } => {}
            other => panic!("expected RunAgain, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn react_reject_hook_does_not_fire_on_interruption() {
        // A doom-loop Interruption from act should pass through unchanged.
        let act = ActResult::Interruption {
            reason: "doom loop: 3 consecutive act cycles".into(),
        };
        let tokens = TokenUsage::default();
        let exec = make_reject_executor();
        let chain = make_chain("always-reject");
        let outcome = react(&act, &tokens, 10, Some(&exec), &[chain]).await;
        match outcome {
            TurnOutcome::Interruption { hook_point, reason, .. } => {
                assert_eq!(hook_point, "doom_loop");
                assert!(reason.contains("doom loop"));
            }
            other => panic!("expected doom_loop Interruption, got {:?}", other),
        }
    }

    #[test]
    fn act_hitl_strict_mode_returns_waiting_for_approval() {
        let mut ctx = make_turn_ctx(vec![]);
        ctx.enforcement_mode = sera_hitl::EnforcementMode::Strict;
        ctx.approval_routing = sera_hitl::ApprovalRouting::Static {
            targets: vec![sera_hitl::ApprovalTarget::Role { name: "admin".to_string() }],
        };
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "let me run that"}),
            tool_calls: vec![serde_json::json!({
                "function": { "name": "shell", "arguments": {"cmd": "ls"} }
            })],
            tokens: TokenUsage::default(),
        };
        let result = act(&ctx, &think_result);
        match result {
            ActResult::WaitingForApproval { tool_call, ticket_id } => {
                assert!(!ticket_id.is_empty());
                assert_eq!(
                    tool_call.get("function").unwrap().get("name").unwrap().as_str().unwrap(),
                    "shell"
                );
            }
            other => panic!("expected WaitingForApproval, got {:?}", other),
        }
    }

    #[test]
    fn act_hitl_autonomous_mode_skips_approval() {
        let ctx = make_turn_ctx(vec![]);
        // Autonomous mode is the default in make_turn_ctx
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "running"}),
            tool_calls: vec![serde_json::json!({
                "function": { "name": "shell", "arguments": {"cmd": "ls"} }
            })],
            tokens: TokenUsage::default(),
        };
        let result = act(&ctx, &think_result);
        match result {
            ActResult::ToolResults(_) => {} // Expected — no approval needed
            other => panic!("expected ToolResults, got {:?}", other),
        }
    }
}
