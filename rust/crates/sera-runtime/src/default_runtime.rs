//! Default agent runtime — skeleton turn loop.
//!
//! Implements `AgentRuntime` using the `ContextPipeline` for context assembly.
//! The model call and tool call loop are stubs pending full integration.
//! See SPEC-runtime §3 for the complete turn loop design.

use async_trait::async_trait;
use sera_types::runtime::{
    AgentRuntime, HealthStatus, RuntimeCapabilities, RuntimeError, TokenUsage, TurnContext,
    TurnResult,
};

use crate::context_pipeline::{ContextPipeline, TurnContext as PipelineTurnContext};

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
/// Wires the `ContextPipeline` into the `AgentRuntime` trait.
/// The model call and tool call loop are currently stubs — the pipeline runs
/// but returns a placeholder response. Full model integration comes in a later
/// phase.
pub struct DefaultRuntime {
    pipeline: ContextPipeline,
    max_tool_iterations: u32,
}

impl std::fmt::Debug for DefaultRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultRuntime")
            .field("pipeline", &self.pipeline)
            .field("max_tool_iterations", &self.max_tool_iterations)
            .finish()
    }
}

impl DefaultRuntime {
    /// Create a new `DefaultRuntime` with the given pipeline.
    ///
    /// `max_tool_iterations` defaults to 10 (SPEC-runtime §3).
    pub fn new(pipeline: ContextPipeline) -> Self {
        Self {
            pipeline,
            max_tool_iterations: 10,
        }
    }

    /// Override the maximum number of tool-call loop iterations.
    pub fn with_max_tool_iterations(mut self, max: u32) -> Self {
        self.max_tool_iterations = max;
        self
    }
}

#[async_trait]
impl AgentRuntime for DefaultRuntime {
    /// Execute one agent turn — skeleton implementation.
    ///
    /// Turn loop (SPEC-runtime §3):
    /// 1. Build a `PipelineTurnContext` from the incoming `TurnContext`.
    /// 2. Run the context assembly pipeline (sorts steps by stability_rank).
    /// 3. TODO: Call model — returns placeholder response for now.
    /// 4. TODO: Process tool calls — returns empty list for now.
    /// 5. Return `TurnResult`.
    async fn execute_turn(&self, ctx: TurnContext) -> Result<TurnResult, RuntimeError> {
        let timer = TurnTimer::new();

        // Step 1: Build the mutable pipeline context from the incoming turn context.
        let mut pipeline_ctx = PipelineTurnContext {
            agent_id: ctx.agent_id,
            session_key: ctx.session_key,
            messages: ctx.messages,
            tools: ctx.available_tools,
            metadata: ctx.metadata,
        };

        // Step 2: Run context assembly pipeline (KV-cache-optimized step ordering).
        self.pipeline
            .run(&mut pipeline_ctx)
            .await
            .map_err(|e| RuntimeError::Internal(e.to_string()))?;

        // Step 3: TODO — call model via ModelAdapter.
        // Placeholder: return a synthetic response so the skeleton compiles and runs.
        let response = "[turn executed - model call pending]".to_string();

        // Step 4: TODO — process tool calls returned by the model.
        // Placeholder: empty tool call list.
        let tool_calls = vec![];

        Ok(TurnResult {
            response,
            tool_calls,
            tokens_used: TokenUsage::default(),
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
    use std::collections::HashMap;

    fn make_turn_context() -> TurnContext {
        TurnContext {
            event_id: "evt-001".to_string(),
            agent_id: "agent-sera".to_string(),
            session_key: "session:agent-sera:user-1".to_string(),
            messages: vec![serde_json::json!({"role": "user", "content": "Hello"})],
            available_tools: vec![],
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn default_runtime_creation() {
        let pipeline = ContextPipeline::new();
        let runtime = DefaultRuntime::new(pipeline);
        assert_eq!(runtime.max_tool_iterations, 10);
    }

    #[test]
    fn default_runtime_with_max_tool_iterations() {
        let pipeline = ContextPipeline::new();
        let runtime = DefaultRuntime::new(pipeline).with_max_tool_iterations(25);
        assert_eq!(runtime.max_tool_iterations, 25);
    }

    #[tokio::test]
    async fn execute_turn_returns_placeholder_result() {
        let pipeline = ContextPipeline::new();
        let runtime = DefaultRuntime::new(pipeline);

        let result = runtime.execute_turn(make_turn_context()).await.unwrap();

        assert_eq!(result.response, "[turn executed - model call pending]");
        assert!(result.tool_calls.is_empty());
        assert_eq!(result.tokens_used.prompt_tokens, 0);
        assert_eq!(result.tokens_used.completion_tokens, 0);
        assert_eq!(result.tokens_used.total_tokens, 0);
    }

    #[tokio::test]
    async fn capabilities_reports_correctly() {
        let runtime = DefaultRuntime::new(ContextPipeline::new());
        let caps = runtime.capabilities().await;

        assert!(caps.supports_tool_calls);
        assert!(!caps.supports_streaming);
        assert!(!caps.supports_structured_output);
        assert!(caps.max_context_tokens.is_none());
    }

    #[tokio::test]
    async fn health_returns_healthy() {
        let runtime = DefaultRuntime::new(ContextPipeline::new());
        assert_eq!(runtime.health().await, HealthStatus::Healthy);
    }

    #[test]
    fn turn_timer_measures_elapsed_time() {
        let timer = TurnTimer::new();
        // elapsed_ms() must return a non-negative value (u64 is always >= 0)
        // and must be callable without panicking.
        let elapsed = timer.elapsed_ms();
        // After construction the elapsed time should be very small (< 1s).
        assert!(elapsed < 1000, "elapsed_ms={elapsed} should be < 1000ms");
    }

    #[test]
    fn turn_timer_default() {
        let timer = TurnTimer::default();
        let elapsed = timer.elapsed_ms();
        assert!(elapsed < 1000);
    }
}
