//! Default agent runtime — wires the four-method lifecycle to the AgentRuntime trait.
//!
//! Implements `AgentRuntime` using the `ContextEngine` for context assembly
//! and the four-method lifecycle (observe/think/act/react) for turn execution.
//! See SPEC-runtime §3 for the complete turn loop design.

use std::collections::HashSet;

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
}

impl std::fmt::Debug for DefaultRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultRuntime")
            .field("context", &self.context.describe())
            .field("has_llm", &self.llm.is_some())
            .field("has_tool_dispatcher", &self.tool_dispatcher.is_some())
            .field("max_tool_iterations", &self.max_tool_iterations)
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
        };

        for _iteration in 0..self.max_tool_iterations {
            // 1. Observe — filter messages, run ConstitutionalGate hooks on input
            let observed = match turn::observe(&turn_ctx, None, &[]).await {
                Ok(msgs) => msgs,
                Err(interruption) => return Ok(interruption),
            };

            // 2. Think — call LLM
            let think_result = turn::think(
                &observed,
                &turn_ctx.tools,
                &turn_ctx.react_mode,
                self.llm.as_deref(),
            )
            .await;

            // 3. Act — dispatch tool calls, doom-loop detection
            let act_result = turn::act(&mut turn_ctx, &think_result, self.tool_dispatcher.as_deref()).await;

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
}
