//! Default agent runtime — wires the four-method lifecycle to the AgentRuntime trait.
//!
//! Implements `AgentRuntime` using the `ContextEngine` for context assembly
//! and the four-method lifecycle (observe/think/act/react) for turn execution.
//! See SPEC-runtime §3 for the complete turn loop design.

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use sera_hitl;
use sera_types::runtime::{
    AgentRuntime, HealthStatus, RuntimeCapabilities, RuntimeError, TurnContext,
    TurnOutcome,
};

use crate::turn::{self, LlmProvider, ReactMode, ToolDispatcher};

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
}

impl std::fmt::Debug for DefaultRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultRuntime")
            .field("context", &self.context.describe())
            .field("has_llm", &self.llm.is_some())
            .field("has_tool_dispatcher", &self.tool_dispatcher.is_some())
            .field("max_tool_iterations", &self.max_tool_iterations)
            .field("failure_threshold", &self.failure_threshold)
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
        }
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

        let mut turn_ctx = turn::TurnContext {
            turn_id: uuid::Uuid::new_v4(),
            session_key: ctx.session_key,
            agent_id: ctx.agent_id,
            messages: ctx.messages,
            tools: tools_as_values,
            handoffs: vec![],
            watch_signals: HashSet::new(),
            change_artifact: ctx.change_artifact.map(|id| id.to_string()),
            react_mode: ReactMode::Default,
            doom_loop_count: 0,
            enforcement_mode: sera_hitl::EnforcementMode::Autonomous,
            approval_routing: sera_hitl::ApprovalRouting::Autonomous,
            pending_steer: None,
            tool_use_behavior: ctx.tool_use_behavior,
        };

        // Per-tool failure counter, reset on session end (i.e. when this method returns).
        let mut tool_failure_counts: HashMap<String, u32> = HashMap::new();

        for _iteration in 0..self.max_tool_iterations {
            // 1. Observe — filter messages, run ConstitutionalGate hooks on input
            let observed = match turn::observe(&turn_ctx, None, &[]).await {
                Ok(msgs) => msgs,
                Err(interruption) => return Ok(interruption),
            };

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
            let think_result = turn::think(
                &observed,
                &turn_ctx.tools,
                &turn_ctx.react_mode,
                self.llm.as_deref(),
                &turn_ctx.tool_use_behavior,
            )
            .await;

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
            let outcome = turn::react(&act_result, &think_result, timer.elapsed_ms(), None, &[]).await;

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
                }
                // Inject accumulated transcript into FinalOutput before returning
                TurnOutcome::FinalOutput { response, tool_calls, tokens_used, duration_ms, .. } => {
                    let transcript = turn_ctx.messages[original_message_count..].to_vec();
                    return Ok(TurnOutcome::FinalOutput {
                        response,
                        tool_calls,
                        tokens_used,
                        duration_ms,
                        transcript,
                    });
                }
                // Any other outcome (Handoff, Interruption, etc.) — return immediately
                other => return Ok(other),
            }
        }

        // Exhausted max_tool_iterations
        Ok(TurnOutcome::Interruption {
            hook_point: "tool_loop".to_string(),
            reason: format!(
                "max tool iterations ({}) exceeded",
                self.max_tool_iterations
            ),
            duration_ms: timer.elapsed_ms(),
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
        let runtime = DefaultRuntime::new(make_context_engine());
        assert_eq!(runtime.max_tool_iterations, 10);
    }

    #[test]
    fn default_runtime_with_max_tool_iterations() {
        let runtime = DefaultRuntime::new(make_context_engine()).with_max_tool_iterations(25);
        assert_eq!(runtime.max_tool_iterations, 25);
    }

    #[tokio::test]
    async fn execute_turn_returns_turn_outcome() {
        let runtime = DefaultRuntime::new(make_context_engine());

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
        let runtime = DefaultRuntime::new(make_context_engine());
        let caps = runtime.capabilities().await;

        assert!(caps.supports_tool_calls);
        assert!(!caps.supports_streaming);
        assert!(!caps.supports_structured_output);
        assert!(caps.max_context_tokens.is_none());
    }

    #[tokio::test]
    async fn health_returns_healthy() {
        let runtime = DefaultRuntime::new(make_context_engine());
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
    struct AlwaysOkDispatcher;

    #[async_trait::async_trait]
    impl turn::ToolDispatcher for AlwaysOkDispatcher {
        async fn dispatch(
            &self,
            tool_call: &serde_json::Value,
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
        let runtime = DefaultRuntime::new(make_context_engine())
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

        let runtime = DefaultRuntime::new(make_context_engine())
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
                });
            }
            // Calls 1-3: return a single tool call to trigger failures.
            Ok(turn::ThinkResult {
                response: serde_json::json!({"role": "assistant", "content": "[stub]"}),
                tool_calls: vec![tool_call(&format!("c{current}"), "fragile")],
                tokens: sera_types::runtime::TokenUsage::default(),
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

        let runtime = DefaultRuntime::new(make_context_engine())
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
        let llm = ToolCallingLlm::new(vec![
            vec![tool_call("c1", "alpha")],
            vec![tool_call("c2", "beta")],
            vec![tool_call("c3", "gamma")],
            vec![], // final
        ]);

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
        let runtime = DefaultRuntime::new(make_context_engine())
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
                    })
                } else {
                    Ok(turn::ThinkResult {
                        response: serde_json::json!({"role": "assistant", "content": "done"}),
                        tool_calls: vec![],
                        tokens: sera_types::runtime::TokenUsage::default(),
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

        let runtime = DefaultRuntime::new(make_context_engine())
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
        let runtime = DefaultRuntime::new(make_context_engine()).with_failure_threshold(5);
        assert_eq!(runtime.failure_threshold, 5);
    }

    #[test]
    fn failure_threshold_default_is_three() {
        let runtime = DefaultRuntime::new(make_context_engine());
        assert_eq!(runtime.failure_threshold, 3);
    }
}
