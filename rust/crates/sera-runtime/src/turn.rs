//! Four-method turn lifecycle — _observe, _think, _act, _react.

use std::collections::HashSet;
use uuid::Uuid;

use async_trait::async_trait;
use sera_hitl;
use sera_hooks::ChainExecutor;
use sera_types::hook::{HookChain, HookContext, HookPoint, HookResult};
use sera_types::runtime::{TokenUsage, TurnOutcome};
use sera_types::tool::{ToolContext, ToolUseBehavior};

use crate::handoff::Handoff;

/// Doom loop threshold — triggers Interruption after this many consecutive act cycles.
pub const DOOM_LOOP_THRESHOLD: u32 = 3;

/// Maximum compaction checkpoints retained per session (SPEC-runtime §6a / P0-6 M2 gate).
///
/// Re-exported here alongside [`DOOM_LOOP_THRESHOLD`] so the turn-loop budget
/// constants live next to each other; the canonical definition lives in
/// [`crate::context_engine::MAX_COMPACTION_CHECKPOINTS_PER_SESSION`].
pub const MAX_COMPACTION_CHECKPOINTS_PER_SESSION: u32 =
    crate::context_engine::MAX_COMPACTION_CHECKPOINTS_PER_SESSION;

/// React mode for the think step.
#[derive(Debug, Clone)]
pub enum ReactMode {
    /// Default mode — model decides.
    Default,
    /// Deterministic ordering (P0 stub).
    ByOrder,
    /// Planning-phase separation — the think step emits a [`Plan`] (tool
    /// intents + rationale) without dispatching, and a subsequent act step
    /// executes the plan's tool calls. Enables review/approval of the plan
    /// mid-turn (future work).
    PlanAndAct,
}

/// A plan produced during the think step under [`ReactMode::PlanAndAct`].
///
/// Plans capture the model's intended tool calls and the accompanying
/// rationale, without triggering dispatch. They act as a mid-turn checkpoint
/// that downstream review/approval surfaces can inspect or mutate before
/// the runtime re-enters the act step to execute them.
#[derive(Debug, Clone)]
pub struct Plan {
    /// Intended tool calls, in OpenAI tool_call wire format.
    pub tool_calls: Vec<serde_json::Value>,
    /// Model-authored rationale extracted from the assistant response content.
    pub rationale: String,
    /// Monotonic epoch millis when the plan was produced.
    pub created_at_ms: u64,
}

// ── LlmProvider trait ────────────────────────────────────────────────────────

/// Errors from the LLM provider.
#[derive(Debug, thiserror::Error)]
pub enum ThinkError {
    #[error("LLM call failed: {0}")]
    Llm(String),
    #[error("type conversion error: {0}")]
    Conversion(String),
    /// The provider does not support the requested non-`Auto` [`ToolUseBehavior`].
    ///
    /// Returned by the default [`LlmProvider::chat_with_behavior`] when a caller
    /// asks for `None`, `Required`, or `Specific` against a provider that did
    /// not override the method. Surfacing this error before the LLM call avoids
    /// wasting a turn on a free-form request the runtime backstop in [`act`]
    /// would only catch after the fact.
    #[error("provider does not support tool_use_behavior={0}; override chat_with_behavior to enforce or translate the policy")]
    UnsupportedToolUseBehavior(String),
}

impl ThinkError {
    /// True when a provider/model emitted an empty assistant message with no tool calls.
    ///
    /// This is a provider edge case, not useful user-facing detail. Callers
    /// should keep the raw error in logs/audit and sanitize assistant-visible
    /// content for this exact failure.
    fn is_empty_assistant_message(&self) -> bool {
        match self {
            ThinkError::Llm(message) => message
                .contains("provider returned assistant message with neither content nor tool_calls"),
            ThinkError::Conversion(_) | ThinkError::UnsupportedToolUseBehavior(_) => false,
        }
    }
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

    /// Like `chat`, but also forwards the tool-use policy to the provider.
    ///
    /// The default implementation delegates to `chat` when either the behavior
    /// is [`ToolUseBehavior::Auto`] *or* the tools slice is empty — with no
    /// tools on the wire there is no policy for the provider to enforce, so a
    /// plain no-tool LLM call is exactly what every non-`Auto` mode reduces to.
    /// For non-`Auto` modes with a non-empty tools slice it returns
    /// [`ThinkError::UnsupportedToolUseBehavior`] *before* calling the LLM, so
    /// a provider that cannot translate the policy onto the wire does not
    /// waste a turn on a free-form request that the runtime backstop in
    /// [`act`] would only catch after the fact (sera-xh3q). Providers that
    /// natively support `tool_choice` (e.g. `LlmClient`) override this method
    /// and translate the policy onto the request body. Runtime-level
    /// enforcement in [`act`] still runs as defence-in-depth against models
    /// that ignore the wire-level field.
    async fn chat_with_behavior(
        &self,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        tool_use_behavior: &ToolUseBehavior,
    ) -> Result<ThinkResult, ThinkError> {
        if !matches!(tool_use_behavior, ToolUseBehavior::Auto) && !tools.is_empty() {
            return Err(ThinkError::UnsupportedToolUseBehavior(format!(
                "{tool_use_behavior:?}"
            )));
        }
        self.chat(messages, tools).await
    }
}

// ── ToolDispatcher trait ────────────────────────────────────────────────────

/// Errors from tool dispatch.
///
/// Re-exported from [`sera_types::tool::ToolError`] — the canonical definition
/// lives there. Use `sera_types::tool::ToolError` directly in new code;
/// `crate::turn::ToolError` is kept as an alias for back-compat.
pub use sera_types::tool::ToolError;

/// Trait for dispatching tool calls from the act step.
///
/// Tool calls and results use `serde_json::Value` to stay decoupled from
/// any specific tool registry implementation. The gateway provides the
/// concrete implementation that bridges to sera-tools or MCP servers.
#[async_trait]
pub trait ToolDispatcher: Send + Sync {
    /// Execute a single tool call and return the result.
    ///
    /// The `tool_call` value follows the OpenAI tool_call format:
    /// ```json
    /// {"id": "call_xxx", "type": "function", "function": {"name": "...", "arguments": "..."}}
    /// ```
    ///
    /// `ctx` carries the per-turn [`ToolContext`] (principal, session,
    /// policy, and authz handle). Implementations built on legacy
    /// executor-based registries may ignore `ctx` during the adapter-first
    /// `TraitToolRegistry` migration (see sera-ttrm-*). Once the migration
    /// lands, `ctx` is used for per-call policy + authz checks.
    ///
    /// Returns a tool result value:
    /// ```json
    /// {"tool_call_id": "call_xxx", "role": "tool", "content": "..."}
    /// ```
    async fn dispatch(
        &self,
        tool_call: &serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<serde_json::Value, ToolError>;

    /// Return the static [`RiskLevel`](sera_types::tool::RiskLevel) declared by
    /// the registered tool, if any.
    ///
    /// Used by [`act`] to drive HITL approval routing per call instead of
    /// hard-coding a single risk for every tool (sera-gran). Dispatchers
    /// backed by a `Tool` registry should return
    /// `Some(metadata().risk_level)`; dispatchers without a registry view
    /// (mocks, ad-hoc test stubs) keep the default `None`, in which case
    /// [`act`] falls back to [`RiskLevel::Execute`](sera_types::tool::RiskLevel)
    /// — the prior, conservative behaviour.
    fn tool_risk_level(&self, _tool_name: &str) -> Option<sera_types::tool::RiskLevel> {
        None
    }
}

// ── Turn context ─────────────────────────────────────────────────────────────

/// Turn context for the four-method lifecycle.
#[derive(Clone)]
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
    pub enforcement_mode: sera_hitl::HitlMode,
    pub approval_routing: sera_hitl::ApprovalRouting,
    /// Pending steer event from the lane queue (injected at next tool boundary).
    /// Set by the gateway when the session has a queued steer message.
    pub pending_steer: Option<serde_json::Value>,
    /// Tool selection policy for this turn (SPEC-runtime §6.3).
    ///
    /// The `OnLlmStart` hook may mutate this field before the model call to
    /// enforce per-turn policy gates. Defaults to `ToolUseBehavior::Auto`.
    pub tool_use_behavior: ToolUseBehavior,
    /// Per-turn [`ToolContext`] threaded into `ToolDispatcher::dispatch`.
    ///
    /// Carries the principal, session, credentials, policy, audit handle,
    /// and authz provider. Built at the turn boundary by the runtime
    /// (see `DefaultRuntime`) and passed by reference to the dispatcher.
    /// Legacy executor-based dispatchers ignore this field; the migration
    /// to `TraitToolRegistry` (sera-ttrm-*) activates the policy gates.
    pub tool_context: ToolContext,
}

/// Reserved interruption reason emitted when the `ConstitutionalGate` hook
/// point is enforced but no policy chain is installed. The absence of a
/// registered chain is treated as a deny by default — callers must opt in to
/// permissive behaviour via `allow_missing` on [`observe`] / [`react`] (see
/// `DefaultRuntime::with_allow_missing_constitutional_gate` for the runtime
/// config knob).
pub const MISSING_GATE_REASON: &str = "no ConstitutionalGate policy installed";

/// Observe — filter messages by watch signals and run ConstitutionalGate hooks on input.
///
/// Returns `Ok(messages)` when hooks allow the turn to proceed, or
/// `Err(TurnOutcome::Interruption)` when a hook rejects the incoming messages.
///
/// `allow_missing` controls the fail-closed default: when `false` (production
/// default), missing executor/chains and executor errors halt the turn with
/// [`MISSING_GATE_REASON`]. Tests may set `true` to opt in to permissive mode.
pub async fn observe(
    ctx: &TurnContext,
    executor: Option<&ChainExecutor>,
    chains: &[HookChain],
    allow_missing: bool,
) -> Result<Vec<serde_json::Value>, TurnOutcome> {
    // P0: return all messages (filtering by cause_by is P1)
    let messages = ctx.messages.clone();

    // Fail-closed: absence of any registered ConstitutionalGate chain is a deny
    // unless the runtime explicitly opted in to permissive mode.
    let has_gate_chain = executor.is_some()
        && chains.iter().any(|c| c.point == HookPoint::ConstitutionalGate);
    if !has_gate_chain {
        if allow_missing {
            return Ok(messages);
        }
        return Err(TurnOutcome::Interruption {
            hook_point: "constitutional_gate".to_string(),
            reason: MISSING_GATE_REASON.to_string(),
            duration_ms: 0,
        });
    }

    let exec = executor.expect("has_gate_chain implies executor is Some");
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
            HookResult::Reject { reason, .. } => Err(TurnOutcome::Interruption {
                hook_point: "constitutional_gate".to_string(),
                reason,
                duration_ms: 0,
            }),
            HookResult::Continue { updated_input, .. } => {
                if let Some(updated) = updated_input
                    && let Some(arr) = updated.as_array()
                {
                    return Ok(arr.clone());
                }
                Ok(messages)
            }
            HookResult::Redirect { target, reason } => {
                let reason_str = reason.unwrap_or_else(|| format!("redirected to {target}"));
                Err(TurnOutcome::Interruption {
                    hook_point: "constitutional_gate".to_string(),
                    reason: reason_str,
                    duration_ms: 0,
                })
            }
        },
        // Executor error on a configured gate is a deny — we never swallow a
        // gate failure, regardless of `allow_missing` (that flag only governs
        // the policy-absence case; a present-but-broken gate is always strict).
        Err(e) => Err(TurnOutcome::Interruption {
            hook_point: "constitutional_gate".to_string(),
            reason: format!("gate executor error: {e}"),
            duration_ms: 0,
        }),
    }
}

/// Think — call the LLM via the provided `LlmProvider`.
///
/// Falls back to a stub response when no provider is given (useful for tests).
/// The `tool_use_behavior` is forwarded to the provider so it can set the
/// appropriate `tool_choice` field on the wire request (SPEC-runtime §6.3).
pub async fn think(
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    react_mode: &ReactMode,
    llm: Option<&dyn LlmProvider>,
    tool_use_behavior: &ToolUseBehavior,
) -> ThinkResult {
    let raw = match llm {
        Some(provider) => match provider.chat_with_behavior(messages, tools, tool_use_behavior).await {
            Ok(result) => result,
            Err(e) => {
                let content = if e.is_empty_assistant_message() {
                    tracing::error!(
                        provider_error_kind = "empty_assistant_message",
                        error = %e,
                        "LLM call failed in think step; sanitized assistant-visible response"
                    );
                    "[LLM unavailable: model returned an empty response; retry the turn.]"
                        .to_string()
                } else {
                    tracing::error!("LLM call failed in think step: {e}");
                    format!("[LLM error: {e}]")
                };
                ThinkResult {
                    response: serde_json::json!({"role": "assistant", "content": content}),
                    tool_calls: vec![],
                    tokens: TokenUsage::default(),
                    plan: None,
                }
            }
        },
        None => ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "[think stub]"}),
            tool_calls: vec![],
            tokens: TokenUsage::default(),
            plan: None,
        },
    };

    // PlanAndAct: capture intended tool calls into a Plan and defer dispatch
    // to the next iteration. If the model emitted no tool calls, the mode is
    // a no-op and we fall through to the normal FinalOutput path.
    if matches!(react_mode, ReactMode::PlanAndAct) && !raw.tool_calls.is_empty() {
        let rationale = raw
            .response
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        let created_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let plan = Plan {
            tool_calls: raw.tool_calls.clone(),
            rationale,
            created_at_ms,
        };
        return ThinkResult {
            response: raw.response,
            // Empty tool_calls so act() does not dispatch in this iteration.
            tool_calls: vec![],
            tokens: raw.tokens,
            plan: Some(plan),
        };
    }

    raw
}

/// Result of the think step.
pub struct ThinkResult {
    pub response: serde_json::Value,
    pub tool_calls: Vec<serde_json::Value>,
    pub tokens: TokenUsage,
    /// Planning-phase output under [`ReactMode::PlanAndAct`].
    ///
    /// When `Some`, the think step captured the model's intended tool calls
    /// as a [`Plan`] and intentionally left `tool_calls` empty so the
    /// subsequent act step does not dispatch them. The runtime loop surfaces
    /// a [`TurnOutcome::PlanEmitted`] at this point and re-enters the
    /// dispatch path on the next iteration with the plan's tool calls.
    pub plan: Option<Plan>,
}

impl ThinkResult {
    /// Build a plain `ThinkResult` with no plan attached (default path).
    pub fn new(
        response: serde_json::Value,
        tool_calls: Vec<serde_json::Value>,
        tokens: TokenUsage,
    ) -> Self {
        Self {
            response,
            tool_calls,
            tokens,
            plan: None,
        }
    }
}

/// Act — dispatch tool calls, check for handoffs, doom-loop detection.
///
/// When a `ToolDispatcher` is provided, tool calls from the LLM are dispatched
/// and their results collected. Without a dispatcher, tool calls are acknowledged
/// but return empty results (useful for tests).
///
/// Enforces [`ToolUseBehavior`] as a runtime defense-in-depth check against
/// non-compliant model responses (SPEC-runtime §6.3):
/// - `None`: any tool call is rejected with an [`ActResult::Interruption`].
/// - `Specific { name }`: tool calls whose name differs from `name` are
///   rejected with an [`ActResult::Interruption`].
/// - `Auto` / `Required`: no runtime gate — the wire-level `tool_choice` is
///   the only enforcement.
pub async fn act(
    ctx: &mut TurnContext,
    think_result: &ThinkResult,
    tool_dispatcher: Option<&dyn ToolDispatcher>,
) -> ActResult {
    // No tool calls — return empty results. Check this before doom-loop
    // enforcement so a model that used several tool rounds and then emits a
    // final answer is allowed to complete instead of being interrupted only
    // because the previous act-cycle count reached the threshold.
    if think_result.tool_calls.is_empty() {
        return ActResult::ToolResults(vec![]);
    }

    // Doom loop check applies only when the model is trying another act cycle.
    if ctx.doom_loop_count >= DOOM_LOOP_THRESHOLD {
        return ActResult::Interruption {
            reason: format!(
                "doom loop: {} consecutive act cycles",
                ctx.doom_loop_count
            ),
        };
    }

    // Tool-use-behavior enforcement — reject disallowed tool calls before
    // any other processing (handoff, HITL, dispatch). This is the runtime
    // backstop when the model ignores the wire-level tool_choice directive.
    if !think_result.tool_calls.is_empty() {
        if ctx.tool_use_behavior.forbids_tools() {
            return ActResult::Interruption {
                reason: format!(
                    "tool_use_behavior=None forbids tool calls, but model emitted {} call(s)",
                    think_result.tool_calls.len()
                ),
            };
        }
        if let Some(required_name) = ctx.tool_use_behavior.forced_name() {
            for tc in &think_result.tool_calls {
                let name = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("");
                if name != required_name {
                    return ActResult::Interruption {
                        reason: format!(
                            "tool_use_behavior=Specific{{{required_name}}} but model called '{name}'"
                        ),
                    };
                }
            }
        }
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

        // Per-tool risk drives the routing decision; fall back to Execute when
        // the dispatcher cannot expose a risk level for this tool (sera-gran).
        // Falling back to Execute preserves the prior conservative behaviour:
        // unknown tools route as if they could mutate state.
        let risk_level = tool_dispatcher
            .and_then(|d| d.tool_risk_level(tool_name))
            .unwrap_or(sera_types::tool::RiskLevel::Execute);

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
                timeout: std::time::Duration::from_secs(crate::llm_client::DEFAULT_LLM_TIMEOUT_SECS),
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

    // Dispatch tool calls and capture the result
    let act_result_inner = match tool_dispatcher {
        Some(dispatcher) => {
            let mut results = Vec::with_capacity(think_result.tool_calls.len());
            for tc in &think_result.tool_calls {
                match dispatcher.dispatch(tc, &ctx.tool_context).await {
                    Ok(result) => results.push(result),
                    Err(e) => {
                        let tool_call_id = tc.get("id")
                            .and_then(|id| id.as_str())
                            .unwrap_or("unknown");
                        // sera-tqzd: stamp the structured failure markers
                        // alongside the human-readable `content` string so
                        // the downstream wire path (`stdio::emit_tool_events_from_transcript`)
                        // can lift them onto `EventMsg::ToolCallEnd::{status,error_class}`
                        // without re-parsing the error text. The marker keys
                        // are documented on `sera_types::envelope`.
                        results.push(serde_json::json!({
                            "tool_call_id": tool_call_id,
                            "role": "tool",
                            "content": format!("[tool error: {e}]"),
                            sera_types::envelope::TOOL_STATUS_MARKER: "failure",
                            sera_types::envelope::TOOL_ERROR_CLASS_MARKER: e.class_name(),
                        }));
                    }
                }
            }
            ActResult::ToolResults(results)
        }
        None => {
            // No dispatcher — return empty results for each tool call
            let results: Vec<serde_json::Value> = think_result.tool_calls.iter().map(|tc| {
                let tool_call_id = tc
                    .get("id")
                    .and_then(|id| id.as_str())
                    .unwrap_or("unknown");
                serde_json::json!({
                    "tool_call_id": tool_call_id,
                    "role": "tool",
                    "content": "[no tool dispatcher configured]",
                })
            }).collect();
            ActResult::ToolResults(results)
        }
    };

    // ── Steer injection: if there's a pending steer message, inject it now (at tool boundary) ──
    // This implements the "Steer Contract" from SPEC-gateway §5.2:
    // Check for steer after each tool call; if present, inject into transcript and signal RunAgain.
    if let Some(steer_content) = ctx.pending_steer.take() {
        // Validate steer content: must be a non-empty string within size limits.
        const MAX_STEER_BYTES: usize = 64 * 1024; // 64 KB
        let steer_text = match steer_content.as_str() {
            Some("") => {
                tracing::warn!(session_key = %ctx.session_key, "Steer injection rejected: empty message");
                return act_result_inner;
            }
            Some(s) if s.len() > MAX_STEER_BYTES => {
                tracing::warn!(
                    session_key = %ctx.session_key,
                    len = s.len(),
                    max = MAX_STEER_BYTES,
                    "Steer injection rejected: message exceeds size limit"
                );
                return act_result_inner;
            }
            Some(s) if s.chars().any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t') => {
                tracing::warn!(session_key = %ctx.session_key, "Steer injection rejected: message contains invalid control characters");
                return act_result_inner;
            }
            Some(s) => s,
            None => {
                tracing::warn!(session_key = %ctx.session_key, "Steer injection rejected: content is not a string");
                return act_result_inner;
            }
        };

        tracing::info!(
            session_key = %ctx.session_key,
            "Steer injection at tool boundary"
        );
        // Convert steer content to a user message and prepend to results
        let steer_message = serde_json::json!({
            "role": "user",
            "content": steer_text
        });
        // Return a special result that signals to the runtime to re-enter think with the steer message
        return ActResult::SteerInjected {
            steer_message: steer_message.clone(),
            tool_results: match act_result_inner {
                ActResult::ToolResults(r) => r,
                _ => vec![],
            },
        };
    }

    act_result_inner
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
    /// Steer message injected at tool boundary — runtime should re-enter think with this message.
    /// Remaining tool calls from the current assistant message are skipped.
    SteerInjected {
        steer_message: serde_json::Value,
        tool_results: Vec<serde_json::Value>,
    },
}

/// React — decide what to do next based on tool results, running ConstitutionalGate hooks
/// on the model's final response before emitting.
///
/// When a hook rejects the response, `TurnOutcome::Interruption` is returned instead.
pub async fn react(
    act_result: &ActResult,
    think_result: &ThinkResult,
    elapsed_ms: u64,
    executor: Option<&ChainExecutor>,
    chains: &[HookChain],
    allow_missing: bool,
) -> TurnOutcome {
    let tokens = &think_result.tokens;

    // PlanAndAct: think() produced a plan and deliberately suppressed the
    // immediate dispatch. Surface it as a PlanEmitted checkpoint so the
    // runtime loop can re-enter act() with the plan's tool calls on the
    // next iteration. Runs before the ToolResults arm because act() will
    // have returned an empty ToolResults vec for this iteration.
    if let Some(plan) = think_result.plan.as_ref()
        && matches!(act_result, ActResult::ToolResults(r) if r.is_empty())
    {
        let plan_tool_calls = plan
            .tool_calls
            .iter()
            .map(json_to_tool_call)
            .collect::<Vec<_>>();
        return TurnOutcome::PlanEmitted {
            plan_tool_calls,
            rationale: plan.rationale.clone(),
            created_at_ms: plan.created_at_ms,
            tokens_used: tokens.clone(),
            duration_ms: elapsed_ms,
        };
    }

    // Build a preliminary outcome from act results.
    let outcome = match act_result {
        ActResult::ToolResults(results) => {
            if results.is_empty() {
                // Extract the LLM's response content for the final output.
                let response = think_result
                    .response
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                TurnOutcome::FinalOutput {
                    response,
                    tool_calls: vec![],
                    tokens_used: tokens.clone(),
                    duration_ms: elapsed_ms,
                    transcript: vec![],
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
        ActResult::SteerInjected { steer_message, tool_results: _ } => {
            // Steer injection at tool boundary: return RunAgain with the steer content embedded
            // so downstream observers and the audit chain can record what was injected.
            let steer_text = steer_message
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            tracing::debug!(steer_content = %steer_text, "SteerInjected propagated to RunAgain");
            TurnOutcome::RunAgain {
                tool_calls: vec![],
                tokens_used: tokens.clone(),
                duration_ms: elapsed_ms,
            }
        }
    };

    // Run ConstitutionalGate enforcement on FinalOutput responses only.
    // Non-FinalOutput outcomes (RunAgain, Handoff, etc.) are intermediate
    // states that the observe() gate already vetted; gating them again would
    // trigger missing-gate denials mid-loop on otherwise valid turns.
    if let TurnOutcome::FinalOutput { ref response, .. } = outcome {
        let has_gate_chain = executor.is_some()
            && chains.iter().any(|c| c.point == HookPoint::ConstitutionalGate);
        if !has_gate_chain {
            if allow_missing {
                return outcome;
            }
            return TurnOutcome::Interruption {
                hook_point: "constitutional_gate".to_string(),
                reason: MISSING_GATE_REASON.to_string(),
                duration_ms: elapsed_ms,
            };
        }

        let exec = executor.expect("has_gate_chain implies executor is Some");
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
                    if let Some(updated) = updated_input
                        && let Some(new_response) = updated.as_str()
                    {
                        return TurnOutcome::FinalOutput {
                            response: new_response.to_string(),
                            tool_calls: vec![],
                            tokens_used: tokens.clone(),
                            duration_ms: elapsed_ms,
                            transcript: vec![],
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
                return TurnOutcome::Interruption {
                    hook_point: "constitutional_gate".to_string(),
                    reason: format!("gate executor error: {e}"),
                    duration_ms: elapsed_ms,
                };
            }
        }
    }

    outcome
}

// ── Plan helpers ─────────────────────────────────────────────────────────────

/// Convert a wire-format tool_call JSON value into a typed [`ToolCall`].
///
/// Tolerates missing/malformed fields — unknown pieces degrade to empty
/// strings / `Value::Null` so a misshapen plan never panics the runtime.
/// `arguments` arrives as a JSON-encoded string on the OpenAI wire format;
/// this helper attempts to re-parse it so the stored `ToolCall.arguments`
/// is an actual JSON object when possible.
fn json_to_tool_call(v: &serde_json::Value) -> sera_types::runtime::ToolCall {
    let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let function = v.get("function");
    let name = function
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let arguments = function
        .and_then(|f| f.get("arguments"))
        .map(|a| match a {
            serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(s)
                .unwrap_or_else(|_| serde_json::Value::String(s.clone())),
            other => other.clone(),
        })
        .unwrap_or(serde_json::Value::Null);
    sera_types::runtime::ToolCall {
        id,
        name,
        arguments,
        result: None,
    }
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
            enforcement_mode: sera_hitl::HitlMode::Autonomous,
            approval_routing: sera_hitl::ApprovalRouting::Autonomous,
            pending_steer: None,
            tool_use_behavior: ToolUseBehavior::Auto,
            tool_context: ToolContext::default(),
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

    // Under `allow_missing = true` (test-mode opt-in) the absence of any
    // policy chain passes through cleanly.
    #[tokio::test]
    async fn observe_no_hooks_passes_through_when_allow_missing() {
        let ctx = make_turn_ctx(vec![
            serde_json::json!({"role": "user", "content": "hello"}),
        ]);
        let msgs = observe(&ctx, None, &[], true).await.unwrap();
        assert_eq!(msgs.len(), 1);
    }

    // Fail-closed default: `allow_missing = false` with no registered gate
    // chain halts the turn with a `constitutional_gate` interruption.
    #[tokio::test]
    async fn observe_no_hooks_is_fail_closed_by_default() {
        let ctx = make_turn_ctx(vec![
            serde_json::json!({"role": "user", "content": "hello"}),
        ]);
        let result = observe(&ctx, None, &[], false).await;
        match result {
            Err(TurnOutcome::Interruption { hook_point, reason, .. }) => {
                assert_eq!(hook_point, "constitutional_gate");
                assert_eq!(reason, MISSING_GATE_REASON);
            }
            other => panic!("expected fail-closed Interruption, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn observe_allow_hook_passes_through() {
        let ctx = make_turn_ctx(vec![
            serde_json::json!({"role": "user", "content": "hello"}),
        ]);
        let exec = make_allow_executor();
        let chain = make_chain("always-allow");
        let msgs = observe(&ctx, Some(&exec), &[chain], false).await.unwrap();
        assert_eq!(msgs.len(), 1);
    }

    #[tokio::test]
    async fn observe_reject_hook_returns_interruption() {
        let ctx = make_turn_ctx(vec![
            serde_json::json!({"role": "user", "content": "harmful content"}),
        ]);
        let exec = make_reject_executor();
        let chain = make_chain("always-reject");
        let result = observe(&ctx, Some(&exec), &[chain], false).await;
        match result {
            Err(TurnOutcome::Interruption { hook_point, reason, .. }) => {
                assert_eq!(hook_point, "constitutional_gate");
                assert_eq!(reason, "constitutional violation");
            }
            other => panic!("expected Err(Interruption), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn observe_no_matching_chains_is_fail_closed() {
        // A chain for a different hook point does not count as a
        // ConstitutionalGate policy — fail-closed must still apply.
        let ctx = make_turn_ctx(vec![
            serde_json::json!({"role": "user", "content": "hello"}),
        ]);
        let exec = make_reject_executor();
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
        let result = observe(&ctx, Some(&exec), &[non_matching_chain], false).await;
        match result {
            Err(TurnOutcome::Interruption { hook_point, reason, .. }) => {
                assert_eq!(hook_point, "constitutional_gate");
                assert_eq!(reason, MISSING_GATE_REASON);
            }
            other => panic!("expected fail-closed Interruption, got {:?}", other),
        }
    }

    // ── react() tests ─────────────────────────────────────────────────────────

    fn make_think_result(content: &str) -> ThinkResult {
        ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": content}),
            tool_calls: vec![],
            tokens: TokenUsage::default(),
            plan: None,
        }
    }

    #[tokio::test]
    async fn react_no_hooks_passes_through_when_allow_missing() {
        let act = ActResult::ToolResults(vec![]);
        let think = make_think_result("Hello from LLM");
        let outcome = react(&act, &think, 10, None, &[], true).await;
        match outcome {
            TurnOutcome::FinalOutput { response, .. } => {
                assert_eq!(response, "Hello from LLM");
            }
            other => panic!("expected FinalOutput, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn react_no_hooks_is_fail_closed_by_default() {
        let act = ActResult::ToolResults(vec![]);
        let think = make_think_result("Hello from LLM");
        let outcome = react(&act, &think, 10, None, &[], false).await;
        match outcome {
            TurnOutcome::Interruption { hook_point, reason, .. } => {
                assert_eq!(hook_point, "constitutional_gate");
                assert_eq!(reason, MISSING_GATE_REASON);
            }
            other => panic!("expected fail-closed Interruption, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn react_allow_hook_passes_final_output_through() {
        let act = ActResult::ToolResults(vec![]);
        let think = make_think_result("Hello from LLM");
        let exec = make_allow_executor();
        let chain = make_chain("always-allow");
        let outcome = react(&act, &think, 10, Some(&exec), &[chain], false).await;
        match outcome {
            TurnOutcome::FinalOutput { response, .. } => {
                assert_eq!(response, "Hello from LLM");
            }
            other => panic!("expected FinalOutput, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn react_reject_hook_returns_interruption() {
        let act = ActResult::ToolResults(vec![]);
        let think = make_think_result("Hello from LLM");
        let exec = make_reject_executor();
        let chain = make_chain("always-reject");
        let outcome = react(&act, &think, 10, Some(&exec), &[chain], false).await;
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
        let think = make_think_result("");
        let exec = make_reject_executor();
        let chain = make_chain("always-reject");
        let outcome = react(&act, &think, 10, Some(&exec), &[chain], false).await;
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
        let think = make_think_result("");
        let exec = make_reject_executor();
        let chain = make_chain("always-reject");
        let outcome = react(&act, &think, 10, Some(&exec), &[chain], false).await;
        match outcome {
            TurnOutcome::Interruption { hook_point, reason, .. } => {
                assert_eq!(hook_point, "doom_loop");
                assert!(reason.contains("doom loop"));
            }
            other => panic!("expected doom_loop Interruption, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn act_allows_final_answer_after_threshold_tool_cycles() {
        let mut ctx = make_turn_ctx(vec![]);
        ctx.doom_loop_count = DOOM_LOOP_THRESHOLD;
        let think_result = make_think_result("Deployment is healthy.");

        let result = act(&mut ctx, &think_result, None).await;

        assert!(
            matches!(result, ActResult::ToolResults(ref results) if results.is_empty()),
            "expected empty ToolResults so react can finish final output, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn act_interrupts_when_threshold_reached_and_model_calls_another_tool() {
        let mut ctx = make_turn_ctx(vec![]);
        ctx.doom_loop_count = DOOM_LOOP_THRESHOLD;
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "checking once more"}),
            tool_calls: vec![make_tool_call("status_probe")],
            tokens: TokenUsage::default(),
            plan: None,
        };

        let result = act(&mut ctx, &think_result, None).await;

        match result {
            ActResult::Interruption { reason } => {
                assert!(reason.contains("doom loop"), "unexpected reason: {reason}");
            }
            other => panic!("expected doom-loop Interruption, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn act_hitl_strict_mode_returns_waiting_for_approval() {
        let mut ctx = make_turn_ctx(vec![]);
        ctx.enforcement_mode = sera_hitl::HitlMode::Strict;
        ctx.approval_routing = sera_hitl::ApprovalRouting::Static {
            targets: vec![sera_hitl::ApprovalTarget::Role { name: "admin".to_string() }],
        };
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "let me run that"}),
            tool_calls: vec![serde_json::json!({
                "function": { "name": "shell", "arguments": {"cmd": "ls"} }
            })],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, None).await;
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

    // ── Steer validation tests ────────────────────────────────────────────────

    #[tokio::test]
    async fn act_steer_empty_message_is_dropped() {
        let mut ctx = make_turn_ctx(vec![]);
        ctx.pending_steer = Some(serde_json::json!(""));
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "ok"}),
            tool_calls: vec![serde_json::json!({
                "id": "call_1",
                "type": "function",
                "function": { "name": "noop", "arguments": "{}" }
            })],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, None).await;
        // Empty steer must be dropped — result is ToolResults not SteerInjected.
        assert!(
            matches!(result, ActResult::ToolResults(_)),
            "expected ToolResults after empty steer drop, got {:?}", result
        );
    }

    #[tokio::test]
    async fn act_steer_oversized_message_is_dropped() {
        let mut ctx = make_turn_ctx(vec![]);
        let big = "x".repeat(64 * 1024 + 1);
        ctx.pending_steer = Some(serde_json::json!(big));
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "ok"}),
            tool_calls: vec![serde_json::json!({
                "id": "call_2",
                "type": "function",
                "function": { "name": "noop", "arguments": "{}" }
            })],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, None).await;
        assert!(
            matches!(result, ActResult::ToolResults(_)),
            "expected ToolResults after oversized steer drop, got {:?}", result
        );
    }

    #[tokio::test]
    async fn act_steer_valid_message_is_injected() {
        let mut ctx = make_turn_ctx(vec![]);
        ctx.pending_steer = Some(serde_json::json!("please focus on task B"));
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "ok"}),
            tool_calls: vec![serde_json::json!({
                "id": "call_3",
                "type": "function",
                "function": { "name": "noop", "arguments": "{}" }
            })],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, None).await;
        match result {
            ActResult::SteerInjected { steer_message, .. } => {
                assert_eq!(
                    steer_message.get("content").and_then(|c| c.as_str()),
                    Some("please focus on task B")
                );
            }
            other => panic!("expected SteerInjected, got {:?}", other),
        }
    }

    // ── ToolUseBehavior enforcement tests (SPEC-runtime §6.3) ─────────────────

    fn make_tool_call(name: &str) -> serde_json::Value {
        serde_json::json!({
            "id": format!("call_{name}"),
            "type": "function",
            "function": { "name": name, "arguments": "{}" }
        })
    }

    #[tokio::test]
    async fn act_tool_use_behavior_auto_allows_any_tool_call() {
        // Baseline: Auto (the default) imposes no runtime gate.
        let mut ctx = make_turn_ctx(vec![]);
        assert_eq!(ctx.tool_use_behavior, ToolUseBehavior::Auto);
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "ok"}),
            tool_calls: vec![make_tool_call("any_tool")],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, None).await;
        match result {
            ActResult::ToolResults(_) => {}
            other => panic!("expected ToolResults under Auto, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn act_tool_use_behavior_none_rejects_tool_call() {
        // None forbids any tool call — runtime must short-circuit with Interruption.
        let mut ctx = make_turn_ctx(vec![]);
        ctx.tool_use_behavior = ToolUseBehavior::None;
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "ok"}),
            tool_calls: vec![make_tool_call("shell")],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, None).await;
        match result {
            ActResult::Interruption { reason } => {
                assert!(
                    reason.contains("tool_use_behavior=None"),
                    "reason missing policy name: {reason}"
                );
            }
            other => panic!("expected Interruption under None, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn act_tool_use_behavior_specific_rejects_other_tool() {
        // Specific{read_file} with a call to `shell` must be rejected.
        let mut ctx = make_turn_ctx(vec![]);
        ctx.tool_use_behavior = ToolUseBehavior::Specific {
            name: "read_file".to_string(),
        };
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "ok"}),
            tool_calls: vec![make_tool_call("shell")],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, None).await;
        match result {
            ActResult::Interruption { reason } => {
                assert!(
                    reason.contains("Specific") && reason.contains("read_file"),
                    "reason missing policy detail: {reason}"
                );
                assert!(reason.contains("shell"), "reason missing offending tool: {reason}");
            }
            other => panic!("expected Interruption under Specific, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn act_tool_use_behavior_specific_allows_matching_tool() {
        // Specific{read_file} with a call to `read_file` must pass through.
        let mut ctx = make_turn_ctx(vec![]);
        ctx.tool_use_behavior = ToolUseBehavior::Specific {
            name: "read_file".to_string(),
        };
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "ok"}),
            tool_calls: vec![make_tool_call("read_file")],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, None).await;
        match result {
            ActResult::ToolResults(_) => {}
            other => panic!(
                "expected ToolResults when tool name matches Specific, got {:?}",
                other
            ),
        }
    }

    #[tokio::test]
    async fn act_tool_use_behavior_default_is_auto() {
        // The default path (no explicit override) maps to Auto.
        let ctx = make_turn_ctx(vec![]);
        assert_eq!(ctx.tool_use_behavior, ToolUseBehavior::Auto);
        // And ToolUseBehavior::default() itself is Auto.
        assert_eq!(ToolUseBehavior::default(), ToolUseBehavior::Auto);
    }

    #[tokio::test]
    async fn act_tool_use_behavior_none_with_no_tool_calls_passes() {
        // Round-trip: None is observed at the wiring site; empty tool_calls is not a violation.
        let mut ctx = make_turn_ctx(vec![]);
        ctx.tool_use_behavior = ToolUseBehavior::None;
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "plain text"}),
            tool_calls: vec![],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, None).await;
        match result {
            ActResult::ToolResults(results) if results.is_empty() => {}
            other => panic!(
                "expected empty ToolResults when None + no tool_calls, got {:?}",
                other
            ),
        }
    }

    // ── Default LlmProvider::chat_with_behavior — sera-xh3q regression guard ──
    //
    // A provider that does not override `chat_with_behavior` must reject
    // non-`Auto` ToolUseBehavior with a non-empty tools slice *before* any LLM
    // call, instead of silently discarding the policy and free-forming the
    // request (which the runtime backstop only catches after a wasted turn).
    // With an empty tools slice there is nothing to enforce on the wire, so
    // any behavior reduces to a plain no-tool chat() and must be allowed
    // through (sera-xh3q follow-up).

    fn dummy_tool() -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {"name": "read_file", "parameters": {}},
        })
    }

    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Test provider that records whether `chat()` was called and never
    /// overrides `chat_with_behavior` — exercising the trait default.
    struct ChatCallCounter {
        calls: AtomicUsize,
    }

    impl ChatCallCounter {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }
        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl LlmProvider for ChatCallCounter {
        async fn chat(
            &self,
            _messages: &[serde_json::Value],
            _tools: &[serde_json::Value],
        ) -> Result<ThinkResult, ThinkError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ThinkResult {
                response: serde_json::json!({"role": "assistant", "content": "ok"}),
                tool_calls: vec![],
                tokens: TokenUsage::default(),
                plan: None,
            })
        }
    }

    #[tokio::test]
    async fn default_chat_with_behavior_rejects_specific_with_tools_before_llm_call() {
        let provider = ChatCallCounter::new();
        let tools = vec![dummy_tool()];
        let result = provider
            .chat_with_behavior(
                &[],
                &tools,
                &ToolUseBehavior::Specific {
                    name: "read_file".to_string(),
                },
            )
            .await;
        match result {
            Err(ThinkError::UnsupportedToolUseBehavior(detail)) => {
                assert!(
                    detail.contains("Specific") && detail.contains("read_file"),
                    "error must surface the rejected behavior: {detail}"
                );
            }
            Err(other) => panic!(
                "expected UnsupportedToolUseBehavior for Specific, got Err({other:?})"
            ),
            Ok(_) => panic!(
                "expected UnsupportedToolUseBehavior for Specific, got Ok(_) — provider must reject before calling chat()"
            ),
        }
        assert_eq!(
            provider.call_count(),
            0,
            "default chat_with_behavior must not call chat() when rejecting Specific with tools present"
        );
    }

    #[tokio::test]
    async fn default_chat_with_behavior_rejects_none_with_tools_before_llm_call() {
        let provider = ChatCallCounter::new();
        let tools = vec![dummy_tool()];
        let result = provider
            .chat_with_behavior(&[], &tools, &ToolUseBehavior::None)
            .await;
        assert!(
            matches!(result, Err(ThinkError::UnsupportedToolUseBehavior(_))),
            "expected UnsupportedToolUseBehavior for None with tools"
        );
        assert_eq!(provider.call_count(), 0);
    }

    #[tokio::test]
    async fn default_chat_with_behavior_rejects_required_with_tools_before_llm_call() {
        let provider = ChatCallCounter::new();
        let tools = vec![dummy_tool()];
        let result = provider
            .chat_with_behavior(&[], &tools, &ToolUseBehavior::Required)
            .await;
        assert!(matches!(
            result,
            Err(ThinkError::UnsupportedToolUseBehavior(_))
        ));
        assert_eq!(provider.call_count(), 0);
    }

    #[tokio::test]
    async fn default_chat_with_behavior_passes_auto_through_to_chat() {
        let provider = ChatCallCounter::new();
        let result = provider
            .chat_with_behavior(&[], &[], &ToolUseBehavior::Auto)
            .await;
        assert!(result.is_ok(), "Auto must delegate to chat()");
        assert_eq!(
            provider.call_count(),
            1,
            "Auto must invoke chat() exactly once"
        );
    }

    #[tokio::test]
    async fn default_chat_with_behavior_passes_none_with_empty_tools_through_to_chat() {
        // sera-xh3q follow-up: with no tools on the wire there is nothing for
        // the provider to enforce, so `None` must reduce to a plain no-tool
        // chat() instead of returning UnsupportedToolUseBehavior.
        let provider = ChatCallCounter::new();
        let result = provider
            .chat_with_behavior(&[], &[], &ToolUseBehavior::None)
            .await;
        assert!(
            result.is_ok(),
            "None with empty tools must delegate to chat()"
        );
        assert_eq!(
            provider.call_count(),
            1,
            "None with empty tools must invoke chat() exactly once"
        );
    }

    #[tokio::test]
    async fn default_chat_with_behavior_passes_specific_with_empty_tools_through_to_chat() {
        let provider = ChatCallCounter::new();
        let result = provider
            .chat_with_behavior(
                &[],
                &[],
                &ToolUseBehavior::Specific {
                    name: "read_file".to_string(),
                },
            )
            .await;
        assert!(result.is_ok(), "Specific with empty tools must delegate to chat()");
        assert_eq!(provider.call_count(), 1);
    }

    #[tokio::test]
    async fn think_handles_unsupported_behavior_via_error_stub_without_llm_call() {
        // think() should surface the unsupported-behavior error as a clean stub
        // response with no tool_calls, without invoking the provider's chat().
        let provider = ChatCallCounter::new();
        let tools = vec![dummy_tool()];
        let result = think(
            &[],
            &tools,
            &ReactMode::Default,
            Some(&provider as &dyn LlmProvider),
            &ToolUseBehavior::Specific {
                name: "x".to_string(),
            },
        )
        .await;
        assert_eq!(provider.call_count(), 0, "chat() must not be invoked");
        assert!(
            result.tool_calls.is_empty(),
            "stub response must not invent tool calls"
        );
        let content = result
            .response
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("");
        assert!(
            content.contains("LLM error") && content.contains("tool_use_behavior"),
            "expected error stub mentioning unsupported tool_use_behavior, got: {content}"
        );
    }

    struct LlmErrorProvider {
        message: &'static str,
    }

    #[async_trait]
    impl LlmProvider for LlmErrorProvider {
        async fn chat(
            &self,
            _messages: &[serde_json::Value],
            _tools: &[serde_json::Value],
        ) -> Result<ThinkResult, ThinkError> {
            Err(ThinkError::Llm(self.message.to_string()))
        }
    }

    #[tokio::test]
    async fn think_sanitizes_empty_assistant_message_provider_error() {
        let provider = LlmErrorProvider {
            message: "request error: provider returned assistant message with neither content nor tool_calls",
        };
        let result = think(
            &[],
            &[],
            &ReactMode::Default,
            Some(&provider as &dyn LlmProvider),
            &ToolUseBehavior::Auto,
        )
        .await;

        assert!(
            result.tool_calls.is_empty(),
            "sanitized provider error must not invent tool calls"
        );
        let content = result
            .response
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("");
        assert!(
            content.contains("model returned an empty response"),
            "expected compact user-facing empty-response text, got: {content}"
        );
        assert!(
            !content.contains("LLM call failed")
                && !content.contains("provider returned assistant message")
                && !content.contains("neither content nor tool_calls"),
            "raw provider detail must not be user-visible: {content}"
        );
    }

    /// Test-only dispatcher that maps tool names to a fixed risk level.
    ///
    /// `dispatch` is unreachable in `act_hitl_*` tests because they short-circuit
    /// on `WaitingForApproval` or skip approval before dispatch — but the trait
    /// still requires it, so we panic to flag any accidental wiring change.
    struct StaticRiskDispatcher {
        risks: std::collections::HashMap<String, sera_types::tool::RiskLevel>,
    }

    impl StaticRiskDispatcher {
        fn new(entries: &[(&str, sera_types::tool::RiskLevel)]) -> Self {
            Self {
                risks: entries
                    .iter()
                    .map(|(n, r)| ((*n).to_string(), *r))
                    .collect(),
            }
        }
    }

    #[async_trait]
    impl ToolDispatcher for StaticRiskDispatcher {
        async fn dispatch(
            &self,
            tool_call: &serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<serde_json::Value, ToolError> {
            // Tests that fall through approval still hit dispatch — return a
            // benign OK so we can assert the non-approval branch was taken.
            let id = tool_call
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Ok(serde_json::json!({
                "tool_call_id": id,
                "role": "tool",
                "content": "ok",
            }))
        }

        fn tool_risk_level(&self, tool_name: &str) -> Option<sera_types::tool::RiskLevel> {
            self.risks.get(tool_name).copied()
        }
    }

    /// Build a Standard-mode routing whose Dynamic policy only escalates when
    /// risk_score ≥ 0.5 — i.e. Read (0.1) skips approval, Execute (0.7) needs it.
    fn execute_or_above_routing() -> sera_hitl::ApprovalRouting {
        sera_hitl::ApprovalRouting::Dynamic(sera_hitl::ApprovalPolicy {
            risk_thresholds: vec![sera_hitl::RiskThreshold {
                min_risk_score: 0.5,
                chain: vec![sera_hitl::ApprovalTarget::Role {
                    name: "admin".to_string(),
                }],
                required_approvals: 1,
            }],
            fallback_chain: vec![],
        })
    }

    #[tokio::test]
    async fn act_hitl_standard_mode_skips_approval_for_read_risk() {
        // Standard mode + threshold-at-0.5 routing — a Read tool (score 0.1)
        // resolves to an empty chain → no approval needed, dispatch proceeds.
        let mut ctx = make_turn_ctx(vec![]);
        ctx.enforcement_mode = sera_hitl::HitlMode::Standard;
        ctx.approval_routing = execute_or_above_routing();
        let dispatcher = StaticRiskDispatcher::new(&[
            ("file-read", sera_types::tool::RiskLevel::Read),
        ]);
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "reading"}),
            tool_calls: vec![serde_json::json!({
                "id": "call_read",
                "type": "function",
                "function": { "name": "file-read", "arguments": "{}" }
            })],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, Some(&dispatcher)).await;
        // Read-risk under threshold@0.5 must skip approval. We don't care
        // which non-approval branch we land in — the goal is to prove we did
        // *not* gate. Without dispatcher consultation this would have routed
        // at Execute and returned WaitingForApproval.
        assert!(
            !matches!(result, ActResult::WaitingForApproval { .. }),
            "Read-risk tool must not require approval under Standard + threshold@0.5, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn act_hitl_standard_mode_requires_approval_for_execute_risk() {
        // Same routing, same mode — an Execute tool (score 0.7) resolves to
        // the admin chain → must produce WaitingForApproval.
        let mut ctx = make_turn_ctx(vec![]);
        ctx.enforcement_mode = sera_hitl::HitlMode::Standard;
        ctx.approval_routing = execute_or_above_routing();
        let dispatcher = StaticRiskDispatcher::new(&[
            ("shell-exec", sera_types::tool::RiskLevel::Execute),
        ]);
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "running"}),
            tool_calls: vec![serde_json::json!({
                "id": "call_exec",
                "type": "function",
                "function": { "name": "shell-exec", "arguments": "{}" }
            })],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, Some(&dispatcher)).await;
        match result {
            ActResult::WaitingForApproval { tool_call, ticket_id } => {
                assert!(!ticket_id.is_empty());
                assert_eq!(
                    tool_call.get("function").unwrap().get("name").unwrap().as_str().unwrap(),
                    "shell-exec"
                );
            }
            other => panic!("expected WaitingForApproval for Execute-risk tool, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn act_hitl_standard_mode_falls_back_to_execute_when_risk_unknown() {
        // The dispatcher does not know this tool — fall back to Execute. Under
        // the threshold@0.5 routing that means the call must be gated. This
        // preserves the prior conservative behaviour for unmapped tools.
        let mut ctx = make_turn_ctx(vec![]);
        ctx.enforcement_mode = sera_hitl::HitlMode::Standard;
        ctx.approval_routing = execute_or_above_routing();
        // Empty dispatcher → tool_risk_level returns None for every name.
        let dispatcher = StaticRiskDispatcher::new(&[]);
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "unknown"}),
            tool_calls: vec![serde_json::json!({
                "id": "call_unknown",
                "type": "function",
                "function": { "name": "totally-unregistered", "arguments": "{}" }
            })],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, Some(&dispatcher)).await;
        match result {
            ActResult::WaitingForApproval { tool_call, .. } => {
                assert_eq!(
                    tool_call.get("function").unwrap().get("name").unwrap().as_str().unwrap(),
                    "totally-unregistered"
                );
            }
            other => panic!(
                "expected WaitingForApproval (fallback Execute) for unknown tool, got {:?}",
                other
            ),
        }
    }

    #[tokio::test]
    async fn act_hitl_no_dispatcher_falls_back_to_execute() {
        // No dispatcher provided at all — must still default to Execute risk
        // so existing strict/standard tests keep working without wiring a
        // dispatcher just for risk lookup.
        let mut ctx = make_turn_ctx(vec![]);
        ctx.enforcement_mode = sera_hitl::HitlMode::Standard;
        ctx.approval_routing = execute_or_above_routing();
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "no dispatcher"}),
            tool_calls: vec![serde_json::json!({
                "id": "call_no_disp",
                "type": "function",
                "function": { "name": "anything", "arguments": "{}" }
            })],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, None).await;
        assert!(
            matches!(result, ActResult::WaitingForApproval { .. }),
            "expected WaitingForApproval when no dispatcher is wired (fallback=Execute), got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn act_hitl_autonomous_mode_skips_approval() {
        let mut ctx = make_turn_ctx(vec![]);
        // Autonomous mode is the default in make_turn_ctx
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "running"}),
            tool_calls: vec![serde_json::json!({
                "function": { "name": "shell", "arguments": {"cmd": "ls"} }
            })],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, None).await;
        match result {
            ActResult::ToolResults(_) => {} // Expected — no approval needed
            other => panic!("expected ToolResults, got {:?}", other),
        }
    }

    // ── sera-tqzd: failure markers on tool result messages ───────────────────

    /// Dispatcher that always returns a fixed `ToolError` variant. Used to
    /// prove that `act()` stamps `_sera_status` + `_sera_error_class` onto
    /// the tool result message when dispatch fails, without depending on the
    /// real tool registry.
    struct AlwaysFailDispatcher {
        err: ToolError,
    }

    #[async_trait]
    impl ToolDispatcher for AlwaysFailDispatcher {
        async fn dispatch(
            &self,
            _tool_call: &serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<serde_json::Value, ToolError> {
            Err(self.err.clone())
        }
    }

    #[tokio::test]
    async fn act_marks_dispatch_failure_with_status_and_error_class() {
        // sera-tqzd: when the dispatcher returns an error, the tool result
        // message put into `ActResult::ToolResults` must carry the
        // `_sera_status: "failure"` / `_sera_error_class: "<variant>"`
        // markers so the runtime's NDJSON emitter can lift them onto the
        // wire `EventMsg::ToolCallEnd`. The human-readable `content` keeps
        // its `[tool error: …]` shape so the LLM still sees the message.
        let mut ctx = make_turn_ctx(vec![]);
        let dispatcher = AlwaysFailDispatcher {
            err: ToolError::PolicyDenied("denied for test".to_string()),
        };
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "trying"}),
            tool_calls: vec![serde_json::json!({
                "id": "call_fail_1",
                "type": "function",
                "function": { "name": "shell-exec", "arguments": "{}" }
            })],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, Some(&dispatcher)).await;
        match result {
            ActResult::ToolResults(results) => {
                assert_eq!(results.len(), 1);
                let r = &results[0];
                assert_eq!(r.get("tool_call_id").and_then(|v| v.as_str()), Some("call_fail_1"));
                assert_eq!(r.get("role").and_then(|v| v.as_str()), Some("tool"));
                let content = r.get("content").and_then(|v| v.as_str()).unwrap_or_default();
                assert!(
                    content.starts_with("[tool error: "),
                    "content must keep the human-readable prefix: {content}",
                );
                assert_eq!(
                    r.get(sera_types::envelope::TOOL_STATUS_MARKER)
                        .and_then(|v| v.as_str()),
                    Some("failure"),
                    "_sera_status marker must record failure: {r}",
                );
                assert_eq!(
                    r.get(sera_types::envelope::TOOL_ERROR_CLASS_MARKER)
                        .and_then(|v| v.as_str()),
                    Some("policy_denied"),
                    "_sera_error_class must record the canonical ToolError variant: {r}",
                );
            }
            other => panic!("expected ToolResults, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn act_propagates_error_class_for_each_variant() {
        // Smoke that every ToolError variant maps to a stable class name in
        // the failure marker — guards against future variants being silently
        // bucketed under a wrong class by the dispatcher path.
        let cases: &[(ToolError, &str)] = &[
            (ToolError::NotFound("x".into()), "not_found"),
            (ToolError::Unauthorized("x".into()), "unauthorized"),
            (ToolError::ExecutionFailed("x".into()), "execution_failed"),
            (ToolError::Timeout, "timeout"),
            (ToolError::InvalidInput("x".into()), "invalid_input"),
            (ToolError::PolicyDenied("x".into()), "policy_denied"),
            (ToolError::InvalidArguments("x".into()), "invalid_arguments"),
            (
                ToolError::AbortedByHook {
                    reason: "x".into(),
                },
                "aborted_by_hook",
            ),
            (
                ToolError::PermissionDenied {
                    reason: "x".into(),
                },
                "permission_denied",
            ),
        ];
        for (err, expected_class) in cases {
            assert_eq!(
                err.class_name(),
                *expected_class,
                "ToolError::class_name() must map {:?} → {}",
                err,
                expected_class,
            );

            let mut ctx = make_turn_ctx(vec![]);
            let dispatcher = AlwaysFailDispatcher { err: err.clone() };
            let think_result = ThinkResult {
                response: serde_json::json!({"role": "assistant", "content": "trying"}),
                tool_calls: vec![serde_json::json!({
                    "id": "call_variant",
                    "type": "function",
                    "function": { "name": "any-tool", "arguments": "{}" }
                })],
                tokens: TokenUsage::default(),
                plan: None,
            };
            let result = act(&mut ctx, &think_result, Some(&dispatcher)).await;
            match result {
                ActResult::ToolResults(results) => {
                    assert_eq!(
                        results[0]
                            .get(sera_types::envelope::TOOL_ERROR_CLASS_MARKER)
                            .and_then(|v| v.as_str()),
                        Some(*expected_class),
                    );
                }
                other => panic!("expected ToolResults for {:?}, got {:?}", err, other),
            }
        }
    }

    #[tokio::test]
    async fn act_does_not_mark_success_path() {
        // The success path must not carry the failure markers — the wire
        // ToolCallEnd defaults to Success, so adding markers here would be a
        // soft regression.
        let mut ctx = make_turn_ctx(vec![]);
        let dispatcher = StaticRiskDispatcher::new(&[
            ("file-list", sera_types::tool::RiskLevel::Read),
        ]);
        let think_result = ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "listing"}),
            tool_calls: vec![serde_json::json!({
                "id": "call_ok_1",
                "type": "function",
                "function": { "name": "file-list", "arguments": "{}" }
            })],
            tokens: TokenUsage::default(),
            plan: None,
        };
        let result = act(&mut ctx, &think_result, Some(&dispatcher)).await;
        match result {
            ActResult::ToolResults(results) => {
                let r = &results[0];
                assert!(r.get(sera_types::envelope::TOOL_STATUS_MARKER).is_none());
                assert!(r.get(sera_types::envelope::TOOL_ERROR_CLASS_MARKER).is_none());
            }
            other => panic!("expected ToolResults, got {:?}", other),
        }
    }
}
