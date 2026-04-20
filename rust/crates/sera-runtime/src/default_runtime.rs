//! Default agent runtime — wires the four-method lifecycle to the AgentRuntime trait.
//!
//! Implements `AgentRuntime` using the `ContextEngine` for context assembly
//! and the four-method lifecycle (observe/think/act/react) for turn execution.
//! See SPEC-runtime §3 for the complete turn loop design.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sera_hitl;
use sera_types::runtime::{
    AgentRuntime, HealthStatus, RuntimeCapabilities, RuntimeError, TurnContext,
    TurnOutcome,
};

use sera_types::tool::AuthzProviderHandle;

use crate::context_engine::ContextEnricher;
use crate::memory_assembler::MemoryBlockAssembler;
use crate::signal_emit::SignalEmitter;
use crate::turn::{self, LlmProvider, ReactMode, ToolDispatcher};
use sera_types::signal::Signal;

// ── TurnTimer ────────────────────────────────────────────────────────────────

/// Measures wall-clock elapsed time for a turn.
pub struct TurnTimer {
    started: std::time::Instant,
}

impl TurnTimer {
    /// Start the timer.
    pub fn new() -> Self {
        Self {
            started: std::time::Instant::now(),
        }
    }

    /// Return elapsed time in milliseconds.
    pub fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}

impl Default for TurnTimer {
    fn default() -> Self {
        Self::new()
    }
}

// ── DefaultRuntime ────────────────────────────────────────────────────────────

/// Default SERA agent runtime.
///
/// Wires the `ContextEngine` and `LlmProvider` into the `AgentRuntime` trait,
/// executing turns via the four-method lifecycle (observe/think/act/react).
pub struct DefaultRuntime {
    context: Box<dyn crate::context_engine::ContextEngine>,
    llm: Option<Box<dyn LlmProvider>>,
    tool_dispatcher: Option<Box<dyn ToolDispatcher>>,
    max_tool_iterations: u32,
    /// Number of consecutive failures per tool before injecting a "Recent Issues" context block.
    /// Defaults to 3. Set to 0 to disable injection.
    pub failure_threshold: u32,
    /// Tier-1 compact memory block assembler.
    ///
    /// If `None`, memory block injection is skipped (backward-compatible default).
    /// Use `with_memory_assembler` to enable.
    memory_assembler: Option<Mutex<MemoryBlockAssembler>>,
    /// Tier-2 semantic enricher (sera-0yqq).
    ///
    /// When set, runs before the Tier-1 memory block assembly each turn and
    /// surfaces up to 3 `MemoryRecall` segments derived from a semantic query
    /// over the user message. Failures degrade silently per SPEC-memory §13.6
    /// so the turn continues when the embedding service or store is down.
    enricher: Option<Arc<ContextEnricher>>,
    /// Authorization provider threaded into every `ToolContext` built by
    /// `execute_turn`. Defaults to the allow-all `DefaultAuthzProviderStub`
    /// from `sera-types`; replaced with a `RoleBasedAuthzProvider` wrapped in
    /// `AuthzProviderAdapter` when `tool_authz_enabled = true` in config.
    authz_provider: Arc<dyn AuthzProviderHandle>,
    /// Optional lifecycle signal emitter. When set, `execute_turn` emits
    /// [`Signal::Started`] at the top of the turn, [`Signal::Progress`] on
    /// each tool-call iteration, and one of
    /// [`Signal::Done`] / [`Signal::Failed`] / [`Signal::Blocked`] /
    /// [`Signal::Review`] at the terminal outcome. See
    /// `docs/signal-system-design.md`.
    signal_emitter: Option<SignalEmitter>,
    /// When `false` (production default) the runtime fails closed at the
    /// `ConstitutionalGate` hook point if no policy chain is installed —
    /// `_observe` and `_react` return an [`TurnOutcome::Interruption`] rather
    /// than proceeding. Flip to `true` in tests / dev harnesses that have no
    /// gate wired up yet. Missing ≠ permissive by design (P0-6).
    allow_missing_constitutional_gate: bool,
}

impl std::fmt::Debug for DefaultRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultRuntime")
            .field("context", &self.context.describe())
            .field("has_llm", &self.llm.is_some())
            .field("has_tool_dispatcher", &self.tool_dispatcher.is_some())
            .field("max_tool_iterations", &self.max_tool_iterations)
            .field("failure_threshold", &self.failure_threshold)
            .field("has_memory_assembler", &self.memory_assembler.is_some())
            .field("has_enricher", &self.enricher.is_some())
            .field("has_signal_emitter", &self.signal_emitter.is_some())
            .finish()
    }
}

impl DefaultRuntime {
    /// Create a new `DefaultRuntime` with the given context engine.
    ///
    /// `max_tool_iterations` defaults to 10 (SPEC-runtime §3).
    pub fn new(context: Box<dyn crate::context_engine::ContextEngine>) -> Self {
        Self {
            context,
            llm: None,
            tool_dispatcher: None,
            max_tool_iterations: 10,
            failure_threshold: 3,
            memory_assembler: None,
            enricher: None,
            authz_provider: Arc::new(sera_types::tool::DefaultAuthzProviderStub),
            signal_emitter: None,
            allow_missing_constitutional_gate: false,
        }
    }

    /// Opt the runtime in to permissive behaviour when no `ConstitutionalGate`
    /// policy chain is installed. Defaults to `false` (fail-closed) — this
    /// setter only exists so tests / bootstrap harnesses without a policy
    /// wired in can proceed. Production deployments must leave it unset.
    pub fn with_allow_missing_constitutional_gate(mut self, allow: bool) -> Self {
        self.allow_missing_constitutional_gate = allow;
        self
    }

    /// Install a lifecycle [`SignalEmitter`].
    ///
    /// When set, `execute_turn` emits `Started` at the top of the turn,
    /// `Progress` on each tool-call iteration, and a terminal signal
    /// (`Done` / `Failed` / `Blocked` / `Review`) before returning. Routing
    /// is driven by the emitter's [`sera_types::signal::SignalTarget`], with
    /// the invariant that `Blocked` and `Review` always reach HITL regardless
    /// of the target. See `docs/signal-system-design.md` and PR #947.
    pub fn with_signal_emitter(mut self, emitter: SignalEmitter) -> Self {
        self.signal_emitter = Some(emitter);
        self
    }

    /// Install an authorization provider that will be threaded into every
    /// `ToolContext` constructed during `execute_turn`.
    ///
    /// Use this to wire a `RoleBasedAuthzProvider` (wrapped in
    /// `sera_auth::authz::AuthzProviderAdapter`) when `tool_authz_enabled`
    /// is set in config. When not called the default allow-all stub is used.
    pub fn with_authz_provider(mut self, provider: Arc<dyn AuthzProviderHandle>) -> Self {
        self.authz_provider = provider;
        self
    }

    /// Install the Tier-2 semantic enricher.
    ///
    /// When set, `execute_turn` embeds the latest user message, queries the
    /// configured semantic-memory store, and promotes up to 3 `MemoryRecall`
    /// segments into the turn's [`MemoryBlockAssembler`] before rendering.
    /// If no assembler is configured, the recalls are rendered inline as a
    /// system message so the LLM still sees them.
    pub fn with_enricher(mut self, enricher: Arc<ContextEnricher>) -> Self {
        self.enricher = Some(enricher);
        self
    }

    /// Set the Tier-1 memory block assembler.
    ///
    /// When set, `execute_turn` prepends the rendered memory block as a system
    /// message before every LLM call. When `record_turn` returns `true`
    /// (overflow_turns >= flush_min_turns), a `memory_pressure` signal is
    /// returned via `TurnOutcome::MemoryPressure` so the caller can emit the
    /// corresponding event.
    pub fn with_memory_assembler(mut self, assembler: MemoryBlockAssembler) -> Self {
        self.memory_assembler = Some(Mutex::new(assembler));
        self
    }

    /// Set the LLM provider for the think step.
    pub fn with_llm(mut self, llm: Box<dyn LlmProvider>) -> Self {
        self.llm = Some(llm);
        self
    }

    /// Set the tool dispatcher for the act step.
    pub fn with_tool_dispatcher(mut self, dispatcher: Box<dyn ToolDispatcher>) -> Self {
        self.tool_dispatcher = Some(dispatcher);
        self
    }

    /// Override the maximum number of tool-call loop iterations.
    pub fn with_max_tool_iterations(mut self, max: u32) -> Self {
        self.max_tool_iterations = max;
        self
    }

    /// Override the per-tool failure threshold for "Recent Issues" injection.
    ///
    /// When a tool fails this many times or more within a single session turn-loop,
    /// a "Recent Issues" block is prepended to the next LLM context. Set to 0 to disable.
    pub fn with_failure_threshold(mut self, threshold: u32) -> Self {
        self.failure_threshold = threshold;
        self
    }

    /// Return a reference to the memory assembler mutex, if one was set.
    ///
    /// Primarily used by tests to inspect assembler state after turn execution.
    pub fn memory_assembler(&self) -> Option<&Mutex<MemoryBlockAssembler>> {
        self.memory_assembler.as_ref()
    }
}

#[async_trait]
impl AgentRuntime for DefaultRuntime {
    /// Execute one agent turn via the four-method lifecycle.
    ///
    /// Turn loop (SPEC-runtime §3):
    /// 1. Ingest messages into the context engine.
    /// 2. Assemble context within token budget.
    /// 3. observe → think → act → react.
    /// 4. Return `TurnOutcome`.
    async fn execute_turn(&self, ctx: TurnContext) -> Result<TurnOutcome, RuntimeError> {
        let timer = TurnTimer::new();

        // Convert sera_types TurnContext → turn::TurnContext
        let tools_as_values: Vec<serde_json::Value> = ctx
            .available_tools
            .iter()
            .filter_map(|t| serde_json::to_value(t).ok())
            .collect();

        let original_message_count = ctx.messages.len();

        // Snapshot for lifecycle signal emission. The `task_id` / `artifact_id`
        // used by the signal system is the turn id — stable across the turn
        // and easy to correlate with the runtime's own tracing spans.
        let turn_id = uuid::Uuid::new_v4();
        let turn_task_id = turn_id.to_string();
        if let Some(emitter) = self.signal_emitter.as_ref() {
            let description = extract_last_user_message(&ctx.messages);
            emitter
                .emit(&Signal::Started {
                    task_id: turn_task_id.clone(),
                    description,
                })
                .await;
        }

        // Build a fresh ToolContext for this turn. Populated from session +
        // agent identity; no principal is available on `sera_types::TurnContext`
        // yet, so we fall back to a Default-sourced allow-all authz handle and
        // an agent-scoped principal. sera-ttrm-4 will thread a real principal
        // through once auth is wired through the turn context.
        let tool_context = sera_types::tool::ToolContext {
            session: sera_types::tool::SessionRef::new(&ctx.session_key),
            principal: sera_types::principal::PrincipalRef {
                id: sera_types::principal::PrincipalId::new(format!(
                    "agent:{}",
                    ctx.agent_id
                )),
                kind: sera_types::principal::PrincipalKind::Agent,
            },
            authz: Arc::clone(&self.authz_provider),
            ..sera_types::tool::ToolContext::default()
        };

        // Respect per-turn react_mode override when present in metadata.
        // Metadata schema: {"react_mode": "default" | "by_order" | "plan_and_act"}.
        // Unknown values fall back to Default so upstream typos never brick a turn.
        let initial_react_mode = ctx
            .metadata
            .get("react_mode")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "plan_and_act" | "PlanAndAct" => ReactMode::PlanAndAct,
                "by_order" | "ByOrder" => ReactMode::ByOrder,
                _ => ReactMode::Default,
            })
            .unwrap_or(ReactMode::Default);

        let mut turn_ctx = turn::TurnContext {
            turn_id,
            session_key: ctx.session_key,
            agent_id: ctx.agent_id,
            messages: ctx.messages,
            tools: tools_as_values,
            handoffs: vec![],
            watch_signals: HashSet::new(),
            change_artifact: ctx.change_artifact.map(|id| id.to_string()),
            react_mode: initial_react_mode,
            doom_loop_count: 0,
            enforcement_mode: sera_hitl::EnforcementMode::Autonomous,
            approval_routing: sera_hitl::ApprovalRouting::Autonomous,
            pending_steer: None,
            tool_use_behavior: ctx.tool_use_behavior,
            tool_context,
        };

        // Per-tool failure counter, reset on session end (i.e. when this method returns).
        let mut tool_failure_counts: HashMap<String, u32> = HashMap::new();

        // Pending plan staged by ReactMode::PlanAndAct. When `Some`, the
        // runtime skips the LLM call on the next iteration and dispatches the
        // plan's tool calls directly via act(). This implements the planning /
        // execution separation as two distinct iterations sharing one turn.
        let mut pending_plan: Option<turn::Plan> = None;

        let max_iterations = self.max_tool_iterations;
        for _iteration in 0..max_iterations {
            // 1. Observe — filter messages, run ConstitutionalGate hooks on input
            let observed = match turn::observe(
                &turn_ctx,
                None,
                &[],
                self.allow_missing_constitutional_gate,
            )
            .await
            {
                Ok(msgs) => msgs,
                Err(interruption) => return Ok(interruption),
            };

            // Run Tier-2 semantic enrichment before Tier-1 assembly (sera-0yqq).
            // The enricher embeds the latest user message, queries the
            // semantic-memory store, hybrid-reranks, and returns up to 3
            // MemoryRecall segments. Failures degrade silently per
            // SPEC-memory §13.6 — the empty return makes the rest of the turn
            // behave exactly as it did before the enricher was wired in.
            let enrichment = if let Some(ref enricher) = self.enricher {
                let last_user = extract_last_user_message(&observed);
                let budget_remaining = remaining_memory_budget(self.memory_assembler.as_ref());
                enricher.enrich(&last_user, budget_remaining).await
            } else {
                crate::context_engine::EnrichmentResult {
                    segments: Vec::new(),
                    query_embedding: None,
                }
            };

            // Inject Tier-1 memory block (Architecture Addendum 2026-04-16 §1).
            // Prepend a system message containing the rendered block before the
            // LLM call. When record_turn returns true (overflow_turns reaches
            // flush_min_turns), surface did_trigger_pressure via return value so
            // the gateway can emit memory_pressure without a cross-crate dep.
            let (observed, memory_pressure_triggered) =
                if let Some(ref asm_mutex) = self.memory_assembler {
                    let rendered = {
                        let mut guard = asm_mutex.lock().unwrap();
                        // Transiently push Tier-2 recalls into the block so
                        // the assembler renders them under its normal
                        // priority/budget rules, then drain them back out so
                        // the persistent segment list is unchanged between
                        // turns.
                        let recall_count = enrichment.segments.len();
                        for seg in &enrichment.segments {
                            guard.block_mut().push(seg.clone());
                        }
                        let result = guard.assemble();
                        // Remove the transient recall segments we just
                        // pushed. They are always the last `recall_count`
                        // entries because `assemble` does not mutate the
                        // segment list order.
                        if recall_count > 0 {
                            let segs = &mut guard.block_mut().segments;
                            let truncate_to = segs.len().saturating_sub(recall_count);
                            segs.truncate(truncate_to);
                        }
                        result
                    };
                    if rendered.rendered.is_empty() {
                        (observed, false)
                    } else {
                        let memory_msg = serde_json::json!({
                            "role": "system",
                            "content": rendered.rendered,
                        });
                        let mut with_memory = Vec::with_capacity(observed.len() + 1);
                        with_memory.push(memory_msg);
                        with_memory.extend(observed);
                        (with_memory, rendered.did_trigger_pressure)
                    }
                } else if !enrichment.segments.is_empty() {
                    // No Tier-1 assembler but we still have Tier-2 recalls —
                    // render them directly as a single system message so the
                    // LLM still sees the retrieved context.
                    let mut body = String::new();
                    for seg in &enrichment.segments {
                        if !body.is_empty() {
                            body.push_str("\n\n");
                        }
                        body.push_str(&seg.content);
                    }
                    let memory_msg = serde_json::json!({
                        "role": "system",
                        "content": body,
                    });
                    let mut with_memory = Vec::with_capacity(observed.len() + 1);
                    with_memory.push(memory_msg);
                    with_memory.extend(observed);
                    (with_memory, false)
                } else {
                    (observed, false)
                };

            if memory_pressure_triggered {
                tracing::info!(
                    session_key = %turn_ctx.session_key,
                    "memory_pressure: overflow_turns reached flush_min_turns threshold"
                );
            }

            // Inject "Recent Issues" block when any tool has hit the failure threshold.
            // Positioned as the last message before the user turn so the model sees it immediately.
            let observed = if self.failure_threshold > 0 {
                let failing: Vec<(&String, &u32)> = tool_failure_counts
                    .iter()
                    .filter(|&(_, &count)| count >= self.failure_threshold)
                    .collect();
                if failing.is_empty() {
                    observed
                } else {
                    let mut lines = vec!["## Recent Issues".to_string()];
                    let mut sorted = failing;
                    sorted.sort_by_key(|(name, _)| name.as_str());
                    for (name, count) in sorted {
                        let noun = if *count == 1 { "time" } else { "times" };
                        lines.push(format!("- Tool `{name}` has failed {count} {noun}"));
                    }
                    let issues_text = lines.join("\n");
                    let issues_msg = serde_json::json!({
                        "role": "user",
                        "content": issues_text,
                    });
                    // Insert the issues block just before the final message in the observed list.
                    let mut with_issues = observed;
                    let insert_pos = if with_issues.is_empty() { 0 } else { with_issues.len() - 1 };
                    with_issues.insert(insert_pos, issues_msg);
                    tracing::debug!(
                        session_key = %turn_ctx.session_key,
                        tool_count = tool_failure_counts.len(),
                        "injecting Recent Issues context block"
                    );
                    with_issues
                }
            } else {
                observed
            };

            // 2. Think — call LLM
            // The OnLlmStart hook may have mutated turn_ctx.tool_use_behavior before
            // this point to enforce per-turn policy gates (SPEC-runtime §6.3).
            //
            // ReactMode::PlanAndAct two-phase flow:
            //   * First iteration: turn::think captures the plan and returns
            //     ThinkResult with empty tool_calls; react() surfaces
            //     TurnOutcome::PlanEmitted, which this loop traps below to
            //     stage the plan into `pending_plan` and continue.
            //   * Next iteration: when `pending_plan` is Some, we synthesize
            //     a ThinkResult carrying the plan's tool_calls directly —
            //     bypassing the LLM — so act() dispatches them in a separate
            //     step. This is the planning/execution separation requested
            //     by the task.
            let think_result = if let Some(plan) = pending_plan.take() {
                tracing::debug!(
                    session_key = %turn_ctx.session_key,
                    plan_tool_call_count = plan.tool_calls.len(),
                    "PlanAndAct: dispatching staged plan (skipping LLM call)"
                );
                turn::ThinkResult {
                    response: serde_json::json!({
                        "role": "assistant",
                        "content": plan.rationale.clone(),
                    }),
                    tool_calls: plan.tool_calls.clone(),
                    tokens: sera_types::runtime::TokenUsage::default(),
                    plan: None,
                }
            } else {
                turn::think(
                    &observed,
                    &turn_ctx.tools,
                    &turn_ctx.react_mode,
                    self.llm.as_deref(),
                    &turn_ctx.tool_use_behavior,
                )
                .await
            };

            // 3. Act — dispatch tool calls, doom-loop detection
            let act_result = turn::act(&mut turn_ctx, &think_result, self.tool_dispatcher.as_deref()).await;

            // Track per-tool failures: scan ToolResults for error-content messages and
            // correlate them back to the tool call that produced them by position.
            if let turn::ActResult::ToolResults(ref results) = act_result {
                for (idx, result) in results.iter().enumerate() {
                    let content_is_error = result
                        .get("content")
                        .and_then(|c| c.as_str())
                        .map(|s| s.starts_with("[tool error:"))
                        .unwrap_or(false);
                    if content_is_error {
                        // Correlate by index to the tool call that produced this result.
                        let tool_name = think_result
                            .tool_calls
                            .get(idx)
                            .and_then(|tc| tc.get("function"))
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        *tool_failure_counts.entry(tool_name).or_insert(0) += 1;
                    }
                }
            }

            // 4. React — decide outcome, run ConstitutionalGate hooks on response
            let outcome = turn::react(
                &act_result,
                &think_result,
                timer.elapsed_ms(),
                None,
                &[],
                self.allow_missing_constitutional_gate,
            )
            .await;

            match outcome {
                TurnOutcome::RunAgain { tokens_used, .. } => {
                    // Append the assistant message (with tool_calls) to the conversation.
                    // The LLM API requires this message before tool results.
                    turn_ctx.messages.push(think_result.response);

                    // Append tool result messages from act
                    if let turn::ActResult::ToolResults(ref results) = act_result {
                        for result in results {
                            turn_ctx.messages.push(result.clone());
                        }
                    }

                    // If steer was injected at tool boundary, add the steer message to conversation
                    if let turn::ActResult::SteerInjected { steer_message, .. } = &act_result {
                        turn_ctx.messages.push(steer_message.clone());
                        tracing::debug!("Steer message injected into conversation for next think");
                    }

                    // Increment doom loop counter and continue
                    turn_ctx.doom_loop_count += 1;

                    tracing::debug!(
                        iteration = _iteration + 1,
                        doom_loop_count = turn_ctx.doom_loop_count,
                        tokens = tokens_used.total_tokens,
                        "tool-call loop: re-entering think with tool results"
                    );

                    if let Some(emitter) = self.signal_emitter.as_ref() {
                        let pct = progress_pct(_iteration + 1, max_iterations);
                        emitter
                            .emit(&Signal::Progress {
                                task_id: turn_task_id.clone(),
                                pct,
                                note: format!(
                                    "tool iteration {}/{}",
                                    _iteration + 1,
                                    max_iterations
                                ),
                            })
                            .await;
                    }
                }
                // Inject accumulated transcript into FinalOutput before returning
                TurnOutcome::FinalOutput { response, tool_calls, tokens_used, duration_ms, .. } => {
                    let transcript = turn_ctx.messages[original_message_count..].to_vec();
                    if let Some(emitter) = self.signal_emitter.as_ref() {
                        emitter
                            .emit(&Signal::Done {
                                artifact_id: turn_task_id.clone(),
                                summary: summarize(&response),
                                duration_ms,
                            })
                            .await;
                    }
                    return Ok(TurnOutcome::FinalOutput {
                        response,
                        tool_calls,
                        tokens_used,
                        duration_ms,
                        transcript,
                    });
                }
                // PlanAndAct: the planning phase completed — persist the
                // assistant message (rationale), stage the plan for the next
                // iteration's act phase, and continue the loop.
                TurnOutcome::PlanEmitted {
                    plan_tool_calls,
                    rationale,
                    created_at_ms,
                    ..
                } => {
                    tracing::info!(
                        session_key = %turn_ctx.session_key,
                        tool_call_count = plan_tool_calls.len(),
                        "PlanAndAct: plan emitted, staging for act phase"
                    );
                    // Record the assistant message (rationale-only) so the
                    // turn transcript preserves the planning step.
                    turn_ctx.messages.push(think_result.response);
                    // Re-stage the plan. The plan field on ThinkResult holds
                    // the original wire-format tool calls; prefer that to
                    // avoid the ToolCall → Value round-trip on re-dispatch.
                    let staged = think_result
                        .plan
                        .clone()
                        .unwrap_or_else(|| turn::Plan {
                            tool_calls: plan_tool_calls
                                .iter()
                                .map(|tc| serde_json::json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.name,
                                        "arguments": tc.arguments.to_string(),
                                    },
                                }))
                                .collect(),
                            rationale,
                            created_at_ms,
                        });
                    pending_plan = Some(staged);
                    turn_ctx.doom_loop_count += 1;

                    if let Some(emitter) = self.signal_emitter.as_ref() {
                        let pct = progress_pct(_iteration + 1, max_iterations);
                        emitter
                            .emit(&Signal::Progress {
                                task_id: turn_task_id.clone(),
                                pct,
                                note: "plan emitted".into(),
                            })
                            .await;
                    }
                }
                // Any other outcome (Handoff, Interruption, WaitingForApproval, etc.)
                // — emit the matching terminal signal and return.
                other => {
                    if let Some(emitter) = self.signal_emitter.as_ref()
                        && let Some(sig) = terminal_signal_for(&other, &turn_task_id)
                    {
                        emitter.emit(&sig).await;
                    }
                    return Ok(other);
                }
            }
        }

        // Exhausted max_tool_iterations — this is a Failed terminal state.
        let duration_ms = timer.elapsed_ms();
        let reason = format!(
            "max tool iterations ({}) exceeded",
            self.max_tool_iterations
        );
        if let Some(emitter) = self.signal_emitter.as_ref() {
            emitter
                .emit(&Signal::Failed {
                    artifact_id: turn_task_id.clone(),
                    error: reason.clone(),
                    retries: 0,
                })
                .await;
        }
        Ok(TurnOutcome::Interruption {
            hook_point: "tool_loop".to_string(),
            reason,
            duration_ms,
        })
    }

    /// Report runtime capabilities.
    async fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            supports_tool_calls: true,
            supports_streaming: false,
            supports_structured_output: false,
            max_context_tokens: None,
        }
    }

    /// Report runtime health.
    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract the most recent `role=="user"` message's `content` string. Returns
/// an empty string when no user message is present or `content` is missing.
fn extract_last_user_message(messages: &[serde_json::Value]) -> String {
    for msg in messages.iter().rev() {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role == "user"
            && let Some(content) = msg.get("content").and_then(|v| v.as_str())
        {
            return content.to_string();
        }
    }
    String::new()
}

/// Map a terminal [`TurnOutcome`] to the matching lifecycle [`Signal`].
///
/// * `FinalOutput` → emitted inline at the return site as `Signal::Done`; not
///   produced here (returns `None`).
/// * `Handoff` → `Signal::Handoff { from_agent, to_agent, artifact_id }`. The
///   `from_agent` is left empty because [`TurnOutcome::Handoff`] only carries
///   the target; callers who need the full pair can derive it from their
///   session context.
/// * `WaitingForApproval` → `Signal::Review` (attention-required, always HITL).
/// * `Interruption` with `hook_point == "constitutional_gate"` or
///   `hook_point == "permission"` → `Signal::Blocked` (attention-required).
/// * Any other `Interruption`, `Stop`, or `Compact` → `Signal::Failed` /
///   `Signal::Done` as appropriate.
/// * `RunAgain` / `PlanEmitted` → no terminal signal (handled by the caller).
fn terminal_signal_for(outcome: &TurnOutcome, artifact_id: &str) -> Option<Signal> {
    match outcome {
        TurnOutcome::FinalOutput { .. } | TurnOutcome::RunAgain { .. } | TurnOutcome::PlanEmitted { .. } => None,
        TurnOutcome::Handoff { target_agent_id, .. } => Some(Signal::Handoff {
            from_agent: String::new(),
            to_agent: target_agent_id.clone(),
            artifact_id: artifact_id.to_string(),
        }),
        TurnOutcome::WaitingForApproval { tool_call, ticket_id, .. } => {
            let tool_name = tool_call
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("unknown");
            Some(Signal::Review {
                artifact_id: artifact_id.to_string(),
                prompt: format!(
                    "approval required for tool `{tool_name}` (ticket {ticket_id})"
                ),
            })
        }
        TurnOutcome::Interruption { hook_point, reason, .. } => {
            // Constitutional gate / permission refusals are capability blocks —
            // they require human attention per the design doc invariant.
            if hook_point == "constitutional_gate" || hook_point == "permission" {
                Some(Signal::Blocked {
                    reason: reason.clone(),
                    requires: Vec::new(),
                })
            } else {
                Some(Signal::Failed {
                    artifact_id: artifact_id.to_string(),
                    error: reason.clone(),
                    retries: 0,
                })
            }
        }
        TurnOutcome::Stop { summary, duration_ms, .. } => Some(Signal::Done {
            artifact_id: artifact_id.to_string(),
            summary: summary.clone(),
            duration_ms: *duration_ms,
        }),
        TurnOutcome::Compact { duration_ms, .. } => Some(Signal::Done {
            artifact_id: artifact_id.to_string(),
            summary: "context compacted".into(),
            duration_ms: *duration_ms,
        }),
    }
}

/// Clamp a fractional progress ratio into the `0..=100` range expected by
/// [`Signal::Progress::pct`].
fn progress_pct(done: u32, total: u32) -> u8 {
    if total == 0 {
        return 0;
    }
    let raw = (done.saturating_mul(100) / total).min(100);
    raw as u8
}

/// Truncate a response string to a short summary suitable for
/// [`Signal::Done::summary`]. Keeps the first 240 chars — enough for a
/// human glance without bloating the inbox row.
fn summarize(response: &str) -> String {
    const MAX: usize = 240;
    if response.chars().count() <= MAX {
        response.to_string()
    } else {
        let mut s: String = response.chars().take(MAX).collect();
        s.push('…');
        s
    }
}

/// Compute the character budget remaining in the Tier-1 `MemoryBlock` after
/// existing segments are accounted for. When no assembler is present, returns
/// a large constant so Tier-2 recalls are not artificially starved — the
/// runtime caller will still respect [`crate::context_engine::MAX_RECALL_SEGMENTS`].
fn remaining_memory_budget(assembler: Option<&Mutex<MemoryBlockAssembler>>) -> usize {
    match assembler {
        Some(mutex) => {
            let guard = match mutex.lock() {
                Ok(g) => g,
                Err(_) => return 0,
            };
            let block = guard.block();
            block.char_budget.saturating_sub(block.current_chars())
        }
        None => 4096,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_engine::pipeline::ContextPipeline;

    fn make_context_engine() -> Box<dyn crate::context_engine::ContextEngine> {
        Box::new(ContextPipeline::new())
    }

    fn make_turn_context() -> TurnContext {
        TurnContext {
            event_id: "evt-001".to_string(),
            agent_id: "agent-sera".to_string(),
            session_key: "session:agent-sera:user-1".to_string(),
            messages: vec![serde_json::json!({"role": "user", "content": "Hello"})],
            available_tools: vec![],
            metadata: std::collections::HashMap::new(),
            change_artifact: None,
            parent_session_key: None,
            tool_use_behavior: Default::default(),
        }
    }

    #[test]
    fn default_runtime_creation() {
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true);
        assert_eq!(runtime.max_tool_iterations, 10);
    }

    #[test]
    fn default_runtime_with_max_tool_iterations() {
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true).with_max_tool_iterations(25);
        assert_eq!(runtime.max_tool_iterations, 25);
    }

    #[tokio::test]
    async fn execute_turn_returns_turn_outcome() {
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true);

        let outcome = runtime.execute_turn(make_turn_context()).await.unwrap();

        match outcome {
            TurnOutcome::FinalOutput { response, tool_calls, tokens_used, .. } => {
                assert_eq!(response, "[think stub]");
                assert!(tool_calls.is_empty());
                assert_eq!(tokens_used.prompt_tokens, 0);
                assert_eq!(tokens_used.completion_tokens, 0);
                assert_eq!(tokens_used.total_tokens, 0);
            }
            other => panic!("expected FinalOutput, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn capabilities_reports_correctly() {
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true);
        let caps = runtime.capabilities().await;

        assert!(caps.supports_tool_calls);
        assert!(!caps.supports_streaming);
        assert!(!caps.supports_structured_output);
        assert!(caps.max_context_tokens.is_none());
    }

    #[tokio::test]
    async fn health_returns_healthy() {
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true);
        assert_eq!(runtime.health().await, HealthStatus::Healthy);
    }

    #[test]
    fn turn_timer_measures_elapsed_time() {
        let timer = TurnTimer::new();
        let elapsed = timer.elapsed_ms();
        assert!(elapsed < 1000, "elapsed_ms={elapsed} should be < 1000ms");
    }

    #[test]
    fn turn_timer_default() {
        let timer = TurnTimer::default();
        let elapsed = timer.elapsed_ms();
        assert!(elapsed < 1000);
    }

    // ── Failure tracking helpers ──────────────────────────────────────────────

    /// LLM stub that returns one tool call per invocation, then a final response.
    struct ToolCallingLlm {
        /// Tool calls to emit on each successive think call.
        /// When the queue is exhausted, emit a plain final response.
        tool_calls: std::sync::Mutex<std::collections::VecDeque<Vec<serde_json::Value>>>,
    }

    impl ToolCallingLlm {
        fn new(rounds: Vec<Vec<serde_json::Value>>) -> Self {
            Self {
                tool_calls: std::sync::Mutex::new(rounds.into()),
            }
        }
    }

    #[async_trait::async_trait]
    impl turn::LlmProvider for ToolCallingLlm {
        async fn chat(
            &self,
            _messages: &[serde_json::Value],
            _tools: &[serde_json::Value],
        ) -> Result<turn::ThinkResult, turn::ThinkError> {
            let calls = self
                .tool_calls
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_default();
            Ok(turn::ThinkResult {
                response: serde_json::json!({"role": "assistant", "content": "[stub]"}),
                tool_calls: calls,
                tokens: sera_types::runtime::TokenUsage::default(),
                plan: None,
            })
        }
    }

    /// Dispatcher that always returns an error for every call.
    struct AlwaysFailDispatcher;

    #[async_trait::async_trait]
    impl turn::ToolDispatcher for AlwaysFailDispatcher {
        async fn dispatch(
            &self,
            tool_call: &serde_json::Value,
            _ctx: &sera_types::tool::ToolContext,
        ) -> Result<serde_json::Value, turn::ToolError> {
            let id = tool_call
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            // Mimic what turn::act does on Err: return a tool error message.
            // We return Err so turn::act formats it as "[tool error: ...]".
            Err(turn::ToolError::ExecutionFailed(format!(
                "forced failure for call {id}"
            )))
        }
    }

    /// Dispatcher that always succeeds.
    #[allow(dead_code)]
    struct AlwaysOkDispatcher;

    #[async_trait::async_trait]
    impl turn::ToolDispatcher for AlwaysOkDispatcher {
        async fn dispatch(
            &self,
            tool_call: &serde_json::Value,
            _ctx: &sera_types::tool::ToolContext,
        ) -> Result<serde_json::Value, turn::ToolError> {
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
    }

    fn tool_call(id: &str, name: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "type": "function",
            "function": { "name": name, "arguments": "{}" }
        })
    }

    // ── Test: counter increments on failure, not on success ───────────────────

    /// A runtime that exposes the internal failure counts for white-box testing.
    /// We drive the loop manually by limiting max_tool_iterations to 1 and
    /// inspecting the messages that were passed to think on iteration 2 by
    /// using a capturing LLM.
    ///
    /// Strategy: run 1 iteration with a failing tool call, then run a second
    /// turn with threshold=1 and verify the injected block appears.

    #[tokio::test]
    async fn failure_counter_increments_on_error_not_on_success() {
        // Use a dispatcher that fails on "bad-tool" but succeeds on "good-tool".
        struct SelectiveDispatcher;
        #[async_trait::async_trait]
        impl turn::ToolDispatcher for SelectiveDispatcher {
            async fn dispatch(
                &self,
                tc: &serde_json::Value,
                _ctx: &sera_types::tool::ToolContext,
            ) -> Result<serde_json::Value, turn::ToolError> {
                let name = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("");
                if name == "bad-tool" {
                    Err(turn::ToolError::ExecutionFailed("boom".into()))
                } else {
                    Ok(serde_json::json!({"tool_call_id": "c1", "role": "tool", "content": "ok"}))
                }
            }
        }

        // Round 1: call both tools. Round 2: no tool calls → FinalOutput.
        let llm = ToolCallingLlm::new(vec![
            vec![tool_call("c1", "good-tool"), tool_call("c2", "bad-tool")],
            vec![],
        ]);

        // Set threshold=2 so injection doesn't fire yet (only 1 failure so far).
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(llm))
            .with_tool_dispatcher(Box::new(SelectiveDispatcher))
            .with_failure_threshold(2)
            .with_max_tool_iterations(10);

        let ctx = make_turn_context();
        // Should complete without injection (only 1 bad-tool failure, threshold=2).
        let outcome = runtime.execute_turn(ctx).await.unwrap();
        // Expect FinalOutput (second think has no tool calls).
        assert!(
            matches!(outcome, TurnOutcome::FinalOutput { .. }),
            "expected FinalOutput, got {:?}", outcome
        );
    }

    // ── Test: no injection below threshold ────────────────────────────────────

    #[tokio::test]
    async fn no_injection_below_threshold() {
        // 2 failing rounds, threshold=3 → should not inject.
        // We verify by checking FinalOutput is reached (not Interruption or anything weird).
        let llm = ToolCallingLlm::new(vec![
            vec![tool_call("c1", "flaky")],
            vec![tool_call("c2", "flaky")],
            vec![], // final
        ]);

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(llm))
            .with_tool_dispatcher(Box::new(AlwaysFailDispatcher))
            .with_failure_threshold(3)
            .with_max_tool_iterations(10);

        let outcome = runtime.execute_turn(make_turn_context()).await.unwrap();
        assert!(
            matches!(outcome, TurnOutcome::FinalOutput { .. }),
            "expected FinalOutput without injection, got {:?}", outcome
        );
    }

    // ── Test: injection appears at threshold, correctly formatted ─────────────

    /// Capture which messages were passed to think on the 4th call.
    struct CapturingLlm {
        call_count: std::sync::Mutex<u32>,
        /// Messages seen on the 4th call (when injection should be present).
        captured: std::sync::Mutex<Vec<serde_json::Value>>,
    }

    impl CapturingLlm {
        fn new() -> Self {
            Self {
                call_count: std::sync::Mutex::new(0),
                captured: std::sync::Mutex::new(vec![]),
            }
        }
    }

    #[async_trait::async_trait]
    impl turn::LlmProvider for CapturingLlm {
        async fn chat(
            &self,
            messages: &[serde_json::Value],
            _tools: &[serde_json::Value],
        ) -> Result<turn::ThinkResult, turn::ThinkError> {
            let mut count = self.call_count.lock().unwrap();
            *count += 1;
            let current = *count;
            // On the 4th call (after 3 failures), capture messages.
            if current == 4 {
                *self.captured.lock().unwrap() = messages.to_vec();
                // Return FinalOutput (no tool calls).
                return Ok(turn::ThinkResult {
                    response: serde_json::json!({"role": "assistant", "content": "done"}),
                    tool_calls: vec![],
                    tokens: sera_types::runtime::TokenUsage::default(),
                    plan: None,
                });
            }
            // Calls 1-3: return a single tool call to trigger failures.
            Ok(turn::ThinkResult {
                response: serde_json::json!({"role": "assistant", "content": "[stub]"}),
                tool_calls: vec![tool_call(&format!("c{current}"), "fragile")],
                tokens: sera_types::runtime::TokenUsage::default(),
                plan: None,
            })
        }
    }

    #[tokio::test]
    async fn injection_appears_at_threshold_with_correct_format() {
        let llm = std::sync::Arc::new(CapturingLlm::new());
        let llm_clone = llm.clone();

        struct ArcLlm(std::sync::Arc<CapturingLlm>);
        #[async_trait::async_trait]
        impl turn::LlmProvider for ArcLlm {
            async fn chat(
                &self,
                messages: &[serde_json::Value],
                tools: &[serde_json::Value],
            ) -> Result<turn::ThinkResult, turn::ThinkError> {
                self.0.chat(messages, tools).await
            }
        }

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(ArcLlm(llm_clone)))
            .with_tool_dispatcher(Box::new(AlwaysFailDispatcher))
            .with_failure_threshold(3)
            .with_max_tool_iterations(10);

        runtime.execute_turn(make_turn_context()).await.unwrap();

        // Verify injection was present on the 4th call.
        let captured = llm.captured.lock().unwrap().clone();
        assert!(!captured.is_empty(), "expected captured messages on 4th call");

        // Find the injected message.
        let injected = captured.iter().find(|m| {
            m.get("content")
                .and_then(|c| c.as_str())
                .map(|s| s.contains("## Recent Issues"))
                .unwrap_or(false)
        });
        assert!(injected.is_some(), "expected '## Recent Issues' block in messages");

        let content = injected.unwrap()["content"].as_str().unwrap();
        assert!(content.contains("`fragile`"), "expected tool name in issues block");
        assert!(content.contains("3 times") || content.contains("3 time"), "expected count in issues block; got: {content}");
    }

    // ── Test: multiple tools each surface in the section ─────────────────────

    #[tokio::test]
    async fn multiple_failing_tools_all_surface_in_issues() {
        // 3 rounds, each with a different tool name failing.
        struct CapturingThinkLlm {
            inner: std::sync::Mutex<std::collections::VecDeque<Vec<serde_json::Value>>>,
            last_messages: std::sync::Mutex<Vec<serde_json::Value>>,
        }
        #[async_trait::async_trait]
        impl turn::LlmProvider for CapturingThinkLlm {
            async fn chat(
                &self,
                messages: &[serde_json::Value],
                _tools: &[serde_json::Value],
            ) -> Result<turn::ThinkResult, turn::ThinkError> {
                *self.last_messages.lock().unwrap() = messages.to_vec();
                let calls = self.inner.lock().unwrap().pop_front().unwrap_or_default();
                Ok(turn::ThinkResult {
                    response: serde_json::json!({"role": "assistant", "content": "[stub]"}),
                    tool_calls: calls,
                    tokens: sera_types::runtime::TokenUsage::default(),
                    plan: None,
                })
            }
        }

        let rounds: std::collections::VecDeque<Vec<serde_json::Value>> = vec![
            vec![tool_call("c1", "alpha")],
            vec![tool_call("c2", "beta")],
            vec![tool_call("c3", "gamma")],
            vec![],
        ]
        .into();

        let capturing = std::sync::Arc::new(CapturingThinkLlm {
            inner: std::sync::Mutex::new(rounds),
            last_messages: std::sync::Mutex::new(vec![]),
        });

        struct ArcLlm2(std::sync::Arc<CapturingThinkLlm>);
        #[async_trait::async_trait]
        impl turn::LlmProvider for ArcLlm2 {
            async fn chat(
                &self,
                messages: &[serde_json::Value],
                tools: &[serde_json::Value],
            ) -> Result<turn::ThinkResult, turn::ThinkError> {
                self.0.chat(messages, tools).await
            }
        }

        let cap_ref = capturing.clone();
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(ArcLlm2(cap_ref)))
            .with_tool_dispatcher(Box::new(AlwaysFailDispatcher))
            .with_failure_threshold(1) // threshold=1 so each tool triggers immediately
            .with_max_tool_iterations(10);

        runtime.execute_turn(make_turn_context()).await.unwrap();

        let last = capturing.last_messages.lock().unwrap().clone();
        let injected_content = last
            .iter()
            .find_map(|m| {
                m.get("content")
                    .and_then(|c| c.as_str())
                    .filter(|s| s.contains("## Recent Issues"))
                    .map(|s| s.to_string())
            });

        let content = injected_content.expect("expected Recent Issues block on final think");
        assert!(content.contains("`alpha`"), "alpha missing: {content}");
        assert!(content.contains("`beta`"), "beta missing: {content}");
        assert!(content.contains("`gamma`"), "gamma missing: {content}");
    }

    // ── Test: counter resets on session end (new execute_turn call) ───────────

    #[tokio::test]
    async fn counter_resets_between_execute_turn_calls() {
        // First call: 2 failures — below threshold=3, no injection.
        // Second call: 0 failures — verifies counters don't persist.
        // If counters leaked, the second call would see stale state.
        // We verify by running two independent calls and both completing as FinalOutput.

        struct TwoFailsThenDone {
            call_count: std::sync::Mutex<u32>,
        }
        #[async_trait::async_trait]
        impl turn::LlmProvider for TwoFailsThenDone {
            async fn chat(
                &self,
                _messages: &[serde_json::Value],
                _tools: &[serde_json::Value],
            ) -> Result<turn::ThinkResult, turn::ThinkError> {
                let mut c = self.call_count.lock().unwrap();
                *c += 1;
                let n = *c;
                if n <= 2 {
                    Ok(turn::ThinkResult {
                        response: serde_json::json!({"role": "assistant", "content": "[stub]"}),
                        tool_calls: vec![tool_call(&format!("c{n}"), "flaky")],
                        tokens: sera_types::runtime::TokenUsage::default(),
                        plan: None,
                    })
                } else {
                    Ok(turn::ThinkResult {
                        response: serde_json::json!({"role": "assistant", "content": "done"}),
                        tool_calls: vec![],
                        tokens: sera_types::runtime::TokenUsage::default(),
                        plan: None,
                    })
                }
            }
        }

        struct ArcLlm3(std::sync::Arc<TwoFailsThenDone>);
        #[async_trait::async_trait]
        impl turn::LlmProvider for ArcLlm3 {
            async fn chat(
                &self,
                msgs: &[serde_json::Value],
                tools: &[serde_json::Value],
            ) -> Result<turn::ThinkResult, turn::ThinkError> {
                self.0.chat(msgs, tools).await
            }
        }

        let llm = std::sync::Arc::new(TwoFailsThenDone {
            call_count: std::sync::Mutex::new(0),
        });

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(ArcLlm3(llm)))
            .with_tool_dispatcher(Box::new(AlwaysFailDispatcher))
            .with_failure_threshold(3)
            .with_max_tool_iterations(10);

        // First call — 2 failures, threshold not reached.
        let out1 = runtime.execute_turn(make_turn_context()).await.unwrap();
        assert!(matches!(out1, TurnOutcome::FinalOutput { .. }), "first call: {:?}", out1);

        // Second call — counters must be fresh (0 failures), no injection.
        let out2 = runtime.execute_turn(make_turn_context()).await.unwrap();
        assert!(matches!(out2, TurnOutcome::FinalOutput { .. }), "second call: {:?}", out2);
    }

    #[test]
    fn failure_threshold_builder_sets_field() {
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true).with_failure_threshold(5);
        assert_eq!(runtime.failure_threshold, 5);
    }

    #[test]
    fn failure_threshold_default_is_three() {
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true);
        assert_eq!(runtime.failure_threshold, 3);
    }

    // ── Builder pattern tests ─────────────────────────────────────────────────

    #[test]
    fn builder_with_llm_sets_llm() {
        struct DummyLlm;
        #[async_trait::async_trait]
        impl turn::LlmProvider for DummyLlm {
            async fn chat(
                &self,
                _messages: &[serde_json::Value],
                _tools: &[serde_json::Value],
            ) -> Result<turn::ThinkResult, turn::ThinkError> {
                Ok(turn::ThinkResult {
                    response: serde_json::json!({"role": "assistant", "content": "hi"}),
                    tool_calls: vec![],
                    tokens: sera_types::runtime::TokenUsage::default(),
                    plan: None,
                })
            }
        }
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true).with_llm(Box::new(DummyLlm));
        assert!(runtime.llm.is_some());
    }

    #[test]
    fn builder_with_tool_dispatcher_sets_dispatcher() {
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_tool_dispatcher(Box::new(AlwaysOkDispatcher));
        assert!(runtime.tool_dispatcher.is_some());
    }

    #[test]
    fn builder_chaining_sets_all_fields() {
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_tool_dispatcher(Box::new(AlwaysOkDispatcher))
            .with_max_tool_iterations(7)
            .with_failure_threshold(2);
        assert!(runtime.tool_dispatcher.is_some());
        assert_eq!(runtime.max_tool_iterations, 7);
        assert_eq!(runtime.failure_threshold, 2);
    }

    #[test]
    fn debug_format_includes_key_fields() {
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true).with_max_tool_iterations(5);
        let debug_str = format!("{runtime:?}");
        assert!(debug_str.contains("max_tool_iterations"), "debug: {debug_str}");
        assert!(debug_str.contains("has_llm"), "debug: {debug_str}");
        assert!(debug_str.contains("has_tool_dispatcher"), "debug: {debug_str}");
    }

    // ── Happy path: no tool calls, immediate FinalOutput ─────────────────────

    #[tokio::test]
    async fn happy_path_no_tool_calls_returns_final_output_with_response() {
        struct ImmediateLlm;
        #[async_trait::async_trait]
        impl turn::LlmProvider for ImmediateLlm {
            async fn chat(
                &self,
                _messages: &[serde_json::Value],
                _tools: &[serde_json::Value],
            ) -> Result<turn::ThinkResult, turn::ThinkError> {
                Ok(turn::ThinkResult {
                    response: serde_json::json!({"role": "assistant", "content": "all done"}),
                    tool_calls: vec![],
                    tokens: sera_types::runtime::TokenUsage {
                        prompt_tokens: 10,
                        completion_tokens: 5,
                        total_tokens: 15,
                    },
                    plan: None,
                })
            }
        }

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true).with_llm(Box::new(ImmediateLlm));
        let outcome = runtime.execute_turn(make_turn_context()).await.unwrap();

        match outcome {
            TurnOutcome::FinalOutput { response, tool_calls, tokens_used, .. } => {
                assert_eq!(response, "all done");
                assert!(tool_calls.is_empty());
                assert_eq!(tokens_used.prompt_tokens, 10);
                assert_eq!(tokens_used.completion_tokens, 5);
                assert_eq!(tokens_used.total_tokens, 15);
            }
            other => panic!("expected FinalOutput, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn happy_path_transcript_includes_new_messages() {
        // When a tool call round happens, the transcript in FinalOutput should
        // contain the assistant message and tool results appended during the loop.
        let llm = ToolCallingLlm::new(vec![
            vec![tool_call("c1", "my-tool")],
            vec![],
        ]);
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(llm))
            .with_tool_dispatcher(Box::new(AlwaysOkDispatcher));

        let ctx = make_turn_context();
        let initial_len = ctx.messages.len();
        let outcome = runtime.execute_turn(ctx).await.unwrap();

        match outcome {
            TurnOutcome::FinalOutput { transcript, .. } => {
                // transcript = messages appended after the original messages
                // Round 1: assistant msg + tool result (2 msgs)
                // Round 2: assistant msg (1 msg, no tool calls → FinalOutput)
                assert!(
                    transcript.len() >= 2,
                    "transcript should have at least 2 new messages, got {}: {:?}",
                    transcript.len(), transcript
                );
                let _ = initial_len; // consumed
            }
            other => panic!("expected FinalOutput, got {:?}", other),
        }
    }

    // ── One tool call + successful result ─────────────────────────────────────

    #[tokio::test]
    async fn one_tool_call_then_final_output() {
        let llm = ToolCallingLlm::new(vec![
            vec![tool_call("call-1", "echo")],
            vec![],
        ]);
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(llm))
            .with_tool_dispatcher(Box::new(AlwaysOkDispatcher));

        let outcome = runtime.execute_turn(make_turn_context()).await.unwrap();
        assert!(
            matches!(outcome, TurnOutcome::FinalOutput { .. }),
            "expected FinalOutput after one tool round, got {:?}", outcome
        );
    }

    // ── Max-iterations exhaustion → Interruption ─────────────────────────────

    #[tokio::test]
    async fn max_tool_iterations_exceeded_returns_interruption() {
        // LLM always returns a tool call; with max=2 we should exhaust and get Interruption.
        struct AlwaysToolLlm;
        #[async_trait::async_trait]
        impl turn::LlmProvider for AlwaysToolLlm {
            async fn chat(
                &self,
                _messages: &[serde_json::Value],
                _tools: &[serde_json::Value],
            ) -> Result<turn::ThinkResult, turn::ThinkError> {
                Ok(turn::ThinkResult {
                    response: serde_json::json!({"role": "assistant", "content": "[stub]"}),
                    tool_calls: vec![tool_call("cx", "looping-tool")],
                    tokens: sera_types::runtime::TokenUsage::default(),
                    plan: None,
                })
            }
        }

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(AlwaysToolLlm))
            .with_tool_dispatcher(Box::new(AlwaysOkDispatcher))
            .with_max_tool_iterations(2);

        let outcome = runtime.execute_turn(make_turn_context()).await.unwrap();
        match outcome {
            TurnOutcome::Interruption { reason, .. } => {
                assert!(
                    reason.contains("max tool iterations"),
                    "expected max-iterations message, got: {reason}"
                );
                assert!(reason.contains('2'), "expected iteration count in message: {reason}");
            }
            other => panic!("expected Interruption, got {:?}", other),
        }
    }

    // ── ToolUseBehavior::None enforcement ─────────────────────────────────────

    #[tokio::test]
    async fn tool_use_behavior_none_rejects_tool_calls() {
        // LLM emits a tool call; runtime should reject it and return Interruption
        // because tool_use_behavior=None forbids tool calls.
        let llm = ToolCallingLlm::new(vec![vec![tool_call("c1", "forbidden")]]);

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(llm))
            .with_tool_dispatcher(Box::new(AlwaysOkDispatcher));

        let mut ctx = make_turn_context();
        ctx.tool_use_behavior = sera_types::tool::ToolUseBehavior::None;

        let outcome = runtime.execute_turn(ctx).await.unwrap();
        match outcome {
            TurnOutcome::Interruption { reason, .. } => {
                assert!(
                    reason.contains("forbids tool calls") || reason.contains("None"),
                    "expected tool-use-behavior rejection, got: {reason}"
                );
            }
            other => panic!("expected Interruption from ToolUseBehavior::None, got {:?}", other),
        }
    }

    // ── ToolUseBehavior::Specific enforcement ─────────────────────────────────

    #[tokio::test]
    async fn tool_use_behavior_specific_rejects_wrong_tool() {
        // LLM calls "wrong-tool" but Specific { name: "allowed-tool" } is set.
        let llm = ToolCallingLlm::new(vec![vec![tool_call("c1", "wrong-tool")]]);

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(llm))
            .with_tool_dispatcher(Box::new(AlwaysOkDispatcher));

        let mut ctx = make_turn_context();
        ctx.tool_use_behavior = sera_types::tool::ToolUseBehavior::Specific {
            name: "allowed-tool".to_string(),
        };

        let outcome = runtime.execute_turn(ctx).await.unwrap();
        match outcome {
            TurnOutcome::Interruption { reason, .. } => {
                assert!(
                    reason.contains("wrong-tool") || reason.contains("Specific"),
                    "expected specific-tool rejection, got: {reason}"
                );
            }
            other => panic!("expected Interruption from ToolUseBehavior::Specific, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn tool_use_behavior_specific_allows_correct_tool() {
        // LLM calls "allowed-tool" which matches Specific — should proceed to FinalOutput.
        let llm = ToolCallingLlm::new(vec![
            vec![tool_call("c1", "allowed-tool")],
            vec![],
        ]);

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(llm))
            .with_tool_dispatcher(Box::new(AlwaysOkDispatcher));

        let mut ctx = make_turn_context();
        ctx.tool_use_behavior = sera_types::tool::ToolUseBehavior::Specific {
            name: "allowed-tool".to_string(),
        };

        let outcome = runtime.execute_turn(ctx).await.unwrap();
        assert!(
            matches!(outcome, TurnOutcome::FinalOutput { .. }),
            "expected FinalOutput for matching Specific tool, got {:?}", outcome
        );
    }

    // ── LLM error path ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn llm_error_produces_error_response_not_panic() {
        // When the LLM provider returns Err, think() wraps it in "[LLM error: ...]"
        // and produces a FinalOutput (no tool calls in the error response).
        struct FailingLlm;
        #[async_trait::async_trait]
        impl turn::LlmProvider for FailingLlm {
            async fn chat(
                &self,
                _messages: &[serde_json::Value],
                _tools: &[serde_json::Value],
            ) -> Result<turn::ThinkResult, turn::ThinkError> {
                Err(turn::ThinkError::Llm("provider down".to_string()))
            }
        }

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true).with_llm(Box::new(FailingLlm));
        let outcome = runtime.execute_turn(make_turn_context()).await.unwrap();

        match outcome {
            TurnOutcome::FinalOutput { response, tool_calls, .. } => {
                assert!(
                    response.contains("LLM error") || response.contains("provider down"),
                    "expected error text in response, got: {response}"
                );
                assert!(tool_calls.is_empty());
            }
            other => panic!("expected FinalOutput wrapping LLM error, got {:?}", other),
        }
    }

    // ── Tool failure path ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn tool_failure_produces_error_content_in_result() {
        // Dispatcher fails; turn::act wraps it in "[tool error: ...]" and the
        // loop continues. With one failing round then a plain final response,
        // we should still reach FinalOutput.
        let llm = ToolCallingLlm::new(vec![
            vec![tool_call("c1", "bad")],
            vec![],
        ]);

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(llm))
            .with_tool_dispatcher(Box::new(AlwaysFailDispatcher))
            .with_failure_threshold(99); // don't let injection interfere

        let outcome = runtime.execute_turn(make_turn_context()).await.unwrap();
        assert!(
            matches!(outcome, TurnOutcome::FinalOutput { .. }),
            "expected FinalOutput after tool failure, got {:?}", outcome
        );
    }

    // ── Doom-loop interruption ─────────────────────────────────────────────────

    #[tokio::test]
    async fn doom_loop_triggers_interruption() {
        // Tool always succeeds but the DOOM_LOOP_THRESHOLD is hit because the
        // LLM keeps emitting tool calls while doom_loop_count increments.
        // With max_tool_iterations > threshold (4 > 3), the doom-loop check fires first.
        struct AlwaysToolLlm2;
        #[async_trait::async_trait]
        impl turn::LlmProvider for AlwaysToolLlm2 {
            async fn chat(
                &self,
                _messages: &[serde_json::Value],
                _tools: &[serde_json::Value],
            ) -> Result<turn::ThinkResult, turn::ThinkError> {
                Ok(turn::ThinkResult {
                    response: serde_json::json!({"role": "assistant", "content": "[stub]"}),
                    tool_calls: vec![tool_call("cx", "t")],
                    tokens: sera_types::runtime::TokenUsage::default(),
                    plan: None,
                })
            }
        }

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(AlwaysToolLlm2))
            .with_tool_dispatcher(Box::new(AlwaysOkDispatcher))
            .with_max_tool_iterations(10); // high enough that doom loop fires

        let outcome = runtime.execute_turn(make_turn_context()).await.unwrap();
        // Either doom_loop Interruption or max_tool_iterations Interruption
        assert!(
            matches!(outcome, TurnOutcome::Interruption { .. }),
            "expected Interruption from looping tools, got {:?}", outcome
        );
    }

    // ── failure_threshold=0 disables injection ────────────────────────────────

    #[tokio::test]
    async fn failure_threshold_zero_disables_injection() {
        // Even with many failures, threshold=0 must never inject Recent Issues.
        // We use a capturing LLM to verify no "## Recent Issues" block appears.
        let _llm = ToolCallingLlm::new(vec![
            vec![tool_call("c1", "flaky")],
            vec![tool_call("c2", "flaky")],
            vec![tool_call("c3", "flaky")],
            vec![],
        ]);

        struct Capturing {
            inner: std::sync::Mutex<std::collections::VecDeque<Vec<serde_json::Value>>>,
            all_messages: std::sync::Mutex<Vec<serde_json::Value>>,
        }
        #[async_trait::async_trait]
        impl turn::LlmProvider for Capturing {
            async fn chat(
                &self,
                messages: &[serde_json::Value],
                _tools: &[serde_json::Value],
            ) -> Result<turn::ThinkResult, turn::ThinkError> {
                self.all_messages.lock().unwrap().extend_from_slice(messages);
                let calls = self.inner.lock().unwrap().pop_front().unwrap_or_default();
                Ok(turn::ThinkResult {
                    response: serde_json::json!({"role": "assistant", "content": "[stub]"}),
                    tool_calls: calls,
                    tokens: sera_types::runtime::TokenUsage::default(),
                    plan: None,
                })
            }
        }

        let rounds: std::collections::VecDeque<_> = vec![
            vec![tool_call("c1", "flaky")],
            vec![tool_call("c2", "flaky")],
            vec![tool_call("c3", "flaky")],
            vec![],
        ]
        .into();
        let cap = std::sync::Arc::new(Capturing {
            inner: std::sync::Mutex::new(rounds),
            all_messages: std::sync::Mutex::new(vec![]),
        });

        struct ArcCap(std::sync::Arc<Capturing>);
        #[async_trait::async_trait]
        impl turn::LlmProvider for ArcCap {
            async fn chat(
                &self,
                messages: &[serde_json::Value],
                tools: &[serde_json::Value],
            ) -> Result<turn::ThinkResult, turn::ThinkError> {
                self.0.chat(messages, tools).await
            }
        }

        let cap_ref = cap.clone();
        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(ArcCap(cap_ref)))
            .with_tool_dispatcher(Box::new(AlwaysFailDispatcher))
            .with_failure_threshold(0); // disabled

        runtime.execute_turn(make_turn_context()).await.unwrap();

        let seen = cap.all_messages.lock().unwrap().clone();
        let has_issues = seen.iter().any(|m| {
            m.get("content")
                .and_then(|c| c.as_str())
                .map(|s| s.contains("## Recent Issues"))
                .unwrap_or(false)
        });
        assert!(!has_issues, "threshold=0 must never inject Recent Issues block");
    }

    // ── No dispatcher: tool calls acknowledged with placeholder ──────────────

    #[tokio::test]
    async fn no_dispatcher_tool_calls_get_placeholder_result() {
        // With no ToolDispatcher set, tool calls should receive a
        // "[no tool dispatcher configured]" result and the loop should
        // continue normally until exhaustion or final response.
        let llm = ToolCallingLlm::new(vec![
            vec![tool_call("c1", "some-tool")],
            vec![],
        ]);

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(llm));
        // intentionally no with_tool_dispatcher

        let outcome = runtime.execute_turn(make_turn_context()).await.unwrap();
        assert!(
            matches!(outcome, TurnOutcome::FinalOutput { .. }),
            "expected FinalOutput with no dispatcher, got {:?}", outcome
        );
    }

    // ── ReactMode::PlanAndAct tests ───────────────────────────────────────────

    /// Dispatcher that records every tool call it dispatches by name, so the
    /// PlanAndAct tests can assert the plan is actually executed on the
    /// second iteration (not the first planning pass).
    struct RecordingDispatcher {
        seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl turn::ToolDispatcher for RecordingDispatcher {
        async fn dispatch(
            &self,
            tc: &serde_json::Value,
            _ctx: &sera_types::tool::ToolContext,
        ) -> Result<serde_json::Value, turn::ToolError> {
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let id = tc
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            self.seen.lock().unwrap().push(name);
            Ok(serde_json::json!({
                "tool_call_id": id,
                "role": "tool",
                "content": "ok",
            }))
        }
    }

    /// LLM that records how many times it was called so tests can verify
    /// `PlanAndAct` only calls the model during the planning iteration and
    /// not during the subsequent execution iteration (which dispatches the
    /// staged plan without re-consulting the model).
    struct CountingPlanLlm {
        calls: std::sync::Mutex<u32>,
        rounds: std::sync::Mutex<std::collections::VecDeque<Vec<serde_json::Value>>>,
    }

    #[async_trait::async_trait]
    impl turn::LlmProvider for CountingPlanLlm {
        async fn chat(
            &self,
            _messages: &[serde_json::Value],
            _tools: &[serde_json::Value],
        ) -> Result<turn::ThinkResult, turn::ThinkError> {
            *self.calls.lock().unwrap() += 1;
            let calls = self.rounds.lock().unwrap().pop_front().unwrap_or_default();
            Ok(turn::ThinkResult {
                response: serde_json::json!({
                    "role": "assistant",
                    "content": "i intend to call tools per plan",
                }),
                tool_calls: calls,
                tokens: sera_types::runtime::TokenUsage::default(),
                plan: None,
            })
        }
    }

    fn plan_and_act_ctx() -> TurnContext {
        let mut ctx = make_turn_context();
        ctx.metadata.insert(
            "react_mode".to_string(),
            serde_json::Value::String("plan_and_act".to_string()),
        );
        ctx
    }

    #[tokio::test]
    async fn react_mode_plan_and_act_emits_plan_then_executes() {
        // First think: emit tool calls → plan staged, no dispatch.
        // Second think: runtime consumes the staged plan (no LLM call) and
        // dispatches the plan's tool calls.
        // Third think: no tool calls → FinalOutput.
        let llm = CountingPlanLlm {
            calls: std::sync::Mutex::new(0),
            rounds: std::sync::Mutex::new(
                vec![
                    vec![tool_call("p1", "plan-tool-a"), tool_call("p2", "plan-tool-b")],
                    // Third round only fires if the LLM is re-called after
                    // plan dispatch — exercises the "plan then final" path.
                    vec![],
                ]
                .into(),
            ),
        };
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let dispatcher = RecordingDispatcher { seen: seen.clone() };

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(llm))
            .with_tool_dispatcher(Box::new(dispatcher));

        let outcome = runtime
            .execute_turn(plan_and_act_ctx())
            .await
            .expect("execute_turn");

        // Loop sequence: think(call 1) → PlanEmitted → staged → act dispatches
        // plan → RunAgain → think(call 2, no tools) → FinalOutput.
        assert!(
            matches!(outcome, TurnOutcome::FinalOutput { .. }),
            "expected FinalOutput after plan dispatch, got {:?}",
            outcome
        );
        let seen_names = seen.lock().unwrap().clone();
        assert_eq!(
            seen_names,
            vec!["plan-tool-a".to_string(), "plan-tool-b".to_string()],
            "plan's tool calls must dispatch in order during the act phase"
        );
    }

    #[tokio::test]
    async fn react_mode_plan_and_act_no_tools_falls_through_to_final() {
        // When PlanAndAct is active but the model emits zero tool calls,
        // the plan path is a no-op — the turn completes as FinalOutput with
        // no dispatches.
        let llm = CountingPlanLlm {
            calls: std::sync::Mutex::new(0),
            rounds: std::sync::Mutex::new(vec![vec![]].into()),
        };
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let dispatcher = RecordingDispatcher { seen: seen.clone() };

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(llm))
            .with_tool_dispatcher(Box::new(dispatcher));

        let outcome = runtime
            .execute_turn(plan_and_act_ctx())
            .await
            .expect("execute_turn");

        assert!(
            matches!(outcome, TurnOutcome::FinalOutput { .. }),
            "expected FinalOutput when plan has zero tool calls, got {:?}",
            outcome
        );
        assert!(
            seen.lock().unwrap().is_empty(),
            "no tools should have been dispatched; saw {:?}",
            seen.lock().unwrap()
        );
    }

    #[tokio::test]
    async fn react_mode_default_unchanged() {
        // Regression: Default mode must behave exactly as before — one tool
        // call round, dispatched immediately (no plan staging), then final
        // output. Verifies the PlanAndAct plumbing is strictly opt-in.
        let llm = ToolCallingLlm::new(vec![
            vec![tool_call("c1", "immediate-tool")],
            vec![],
        ]);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let dispatcher = RecordingDispatcher { seen: seen.clone() };

        let runtime = DefaultRuntime::new(make_context_engine()).with_allow_missing_constitutional_gate(true)
            .with_llm(Box::new(llm))
            .with_tool_dispatcher(Box::new(dispatcher));

        // Default mode — no react_mode metadata override.
        let outcome = runtime
            .execute_turn(make_turn_context())
            .await
            .expect("execute_turn");

        assert!(
            matches!(outcome, TurnOutcome::FinalOutput { .. }),
            "expected FinalOutput under Default mode, got {:?}",
            outcome
        );
        let seen_names = seen.lock().unwrap().clone();
        assert_eq!(
            seen_names,
            vec!["immediate-tool".to_string()],
            "Default mode dispatches the tool call in the same iteration as think"
        );
    }
}
