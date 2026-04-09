//! Context Assembly Pipeline — KV-cache-optimized context window assembly.
//!
//! Steps are sorted by `stability_rank()` before execution, ensuring stable
//! segments (persona, tools) appear at the prefix of the context window.
//! This maximises KV cache prefix hits across turns in LLM serving engines
//! such as vLLM, SGLang, and TensorRT-LLM.

use std::collections::HashMap;

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;

use sera_domain::tool::ToolDefinition;

/// Context for a single agent turn, mutated in place by each pipeline step.
#[derive(Debug, Clone)]
pub struct TurnContext {
    pub agent_id: String,
    pub session_key: String,
    pub messages: Vec<Value>,
    pub tools: Vec<ToolDefinition>,
    pub metadata: HashMap<String, Value>,
}

/// Error type for pipeline step failures.
#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("step '{step}' failed: {source}")]
    StepFailed {
        step: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("pipeline timed out")]
    Timeout,
    #[error("pipeline was cancelled")]
    Cancelled,
}

/// A single step in the context assembly pipeline.
#[async_trait]
pub trait ContextStep: Send + Sync {
    /// Human-readable name for this step (used in error messages and tracing).
    fn name(&self) -> &str;

    /// Position hint for KV cache optimization — lower = more stable = placed earlier.
    fn stability_rank(&self) -> u32;

    /// Execute the step, mutating `ctx` in place.
    async fn execute(&self, ctx: &mut TurnContext) -> Result<(), PipelineError>;
}

/// Ordered context assembly pipeline.
///
/// Steps are sorted by `stability_rank()` on each `run()` call so that custom
/// steps inserted at any time are always executed in the correct KV-cache order.
#[derive(Default)]
pub struct ContextPipeline {
    pub steps: Vec<Box<dyn ContextStep>>,
}

impl std::fmt::Debug for ContextPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.steps.iter().map(|s| s.name()).collect();
        f.debug_struct("ContextPipeline")
            .field("steps", &names)
            .finish()
    }
}

impl ContextPipeline {
    /// Create an empty pipeline.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a step to the pipeline.
    pub fn add_step(&mut self, step: Box<dyn ContextStep>) {
        self.steps.push(step);
    }

    /// Sort steps by `stability_rank`, then execute each in order.
    pub async fn run(&self, ctx: &mut TurnContext) -> Result<(), PipelineError> {
        // Collect indices sorted by stability_rank (stable sort preserves insertion order for ties).
        let mut order: Vec<usize> = (0..self.steps.len()).collect();
        order.sort_by_key(|&i| self.steps[i].stability_rank());

        for i in order {
            self.steps[i].execute(ctx).await?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Default step stubs
// ---------------------------------------------------------------------------

/// Injects the agent persona / system prompt (stability rank 10 — most stable).
#[derive(Debug, Default, Clone)]
pub struct PersonaStep;

#[async_trait]
impl ContextStep for PersonaStep {
    fn name(&self) -> &str {
        "persona"
    }
    fn stability_rank(&self) -> u32 {
        10
    }
    async fn execute(&self, _ctx: &mut TurnContext) -> Result<(), PipelineError> {
        Ok(())
    }
}

/// Injects available tool schemas (stability rank 20).
#[derive(Debug, Default, Clone)]
pub struct ToolInjectionStep;

#[async_trait]
impl ContextStep for ToolInjectionStep {
    fn name(&self) -> &str {
        "tool_injection"
    }
    fn stability_rank(&self) -> u32 {
        20
    }
    async fn execute(&self, _ctx: &mut TurnContext) -> Result<(), PipelineError> {
        Ok(())
    }
}

/// Injects active skill context (stability rank 30).
#[derive(Debug, Default, Clone)]
pub struct SkillInjectionStep;

#[async_trait]
impl ContextStep for SkillInjectionStep {
    fn name(&self) -> &str {
        "skill_injection"
    }
    fn stability_rank(&self) -> u32 {
        30
    }
    async fn execute(&self, _ctx: &mut TurnContext) -> Result<(), PipelineError> {
        Ok(())
    }
}

/// Injects long-term memory excerpts (stability rank 40).
#[derive(Debug, Default, Clone)]
pub struct MemoryInjectionStep;

#[async_trait]
impl ContextStep for MemoryInjectionStep {
    fn name(&self) -> &str {
        "memory_injection"
    }
    fn stability_rank(&self) -> u32 {
        40
    }
    async fn execute(&self, _ctx: &mut TurnContext) -> Result<(), PipelineError> {
        Ok(())
    }
}

/// Injects the session transcript sliding window (stability rank 50).
#[derive(Debug, Default, Clone)]
pub struct HistoryInjectionStep;

#[async_trait]
impl ContextStep for HistoryInjectionStep {
    fn name(&self) -> &str {
        "history_injection"
    }
    fn stability_rank(&self) -> u32 {
        50
    }
    async fn execute(&self, _ctx: &mut TurnContext) -> Result<(), PipelineError> {
        Ok(())
    }
}

/// Injects the current user message and dynamic per-turn context (stability rank 60).
#[derive(Debug, Default, Clone)]
pub struct CurrentTurnStep;

#[async_trait]
impl ContextStep for CurrentTurnStep {
    fn name(&self) -> &str {
        "current_turn"
    }
    fn stability_rank(&self) -> u32 {
        60
    }
    async fn execute(&self, _ctx: &mut TurnContext) -> Result<(), PipelineError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn make_ctx() -> TurnContext {
        TurnContext {
            agent_id: "agent-1".to_string(),
            session_key: "sess-1".to_string(),
            messages: vec![],
            tools: vec![],
            metadata: HashMap::new(),
        }
    }

    /// A test step that records its name in the shared execution log.
    struct RecordingStep {
        name: String,
        rank: u32,
        log: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl ContextStep for RecordingStep {
        fn name(&self) -> &str {
            &self.name
        }
        fn stability_rank(&self) -> u32 {
            self.rank
        }
        async fn execute(&self, _ctx: &mut TurnContext) -> Result<(), PipelineError> {
            self.log.lock().unwrap().push(self.name.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_empty_pipeline_succeeds() {
        let pipeline = ContextPipeline::new();
        let mut ctx = make_ctx();
        assert!(pipeline.run(&mut ctx).await.is_ok());
    }

    #[tokio::test]
    async fn test_pipeline_sorts_by_stability_rank() {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));

        let mut pipeline = ContextPipeline::new();
        // Insert in reverse order — pipeline must re-sort.
        pipeline.add_step(Box::new(RecordingStep {
            name: "high".to_string(),
            rank: 50,
            log: Arc::clone(&log),
        }));
        pipeline.add_step(Box::new(RecordingStep {
            name: "low".to_string(),
            rank: 10,
            log: Arc::clone(&log),
        }));
        pipeline.add_step(Box::new(RecordingStep {
            name: "mid".to_string(),
            rank: 30,
            log: Arc::clone(&log),
        }));

        let mut ctx = make_ctx();
        pipeline.run(&mut ctx).await.unwrap();

        let order = log.lock().unwrap().clone();
        assert_eq!(order, vec!["low", "mid", "high"]);
    }

    #[tokio::test]
    async fn test_pipeline_executes_steps_in_order() {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));

        let mut pipeline = ContextPipeline::new();
        for (name, rank) in [("a", 10u32), ("b", 20), ("c", 30)] {
            pipeline.add_step(Box::new(RecordingStep {
                name: name.to_string(),
                rank,
                log: Arc::clone(&log),
            }));
        }

        let mut ctx = make_ctx();
        pipeline.run(&mut ctx).await.unwrap();

        assert_eq!(*log.lock().unwrap(), vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn test_default_steps_have_correct_ranks() {
        assert_eq!(PersonaStep.stability_rank(), 10);
        assert_eq!(ToolInjectionStep.stability_rank(), 20);
        assert_eq!(SkillInjectionStep.stability_rank(), 30);
        assert_eq!(MemoryInjectionStep.stability_rank(), 40);
        assert_eq!(HistoryInjectionStep.stability_rank(), 50);
        assert_eq!(CurrentTurnStep.stability_rank(), 60);
    }
}
