//! Integration tests for MemoryBlockAssembler wired into DefaultRuntime.
//!
//! Covers the full path: assembler → turn loop → system message prepended → LLM sees it.

use std::sync::Mutex;

use async_trait::async_trait;
use sera_runtime::context_engine::pipeline::ContextPipeline;
use sera_runtime::default_runtime::DefaultRuntime;
use sera_runtime::memory_assembler::MemoryBlockAssembler;
use sera_runtime::turn::{self, ThinkError, ThinkResult};
use sera_types::memory::{MemoryBlock, MemorySegment, SegmentKind};
use sera_types::runtime::{AgentRuntime, TurnContext, TurnOutcome};

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_context_engine() -> Box<dyn sera_runtime::context_engine::ContextEngine> {
    Box::new(ContextPipeline::new())
}

fn make_turn_context() -> TurnContext {
    TurnContext {
        event_id: "evt-mem-001".to_string(),
        agent_id: "agent-mem-test".to_string(),
        session_key: "session:agent-mem-test:u1".to_string(),
        messages: vec![serde_json::json!({"role": "user", "content": "Hello"})],
        available_tools: vec![],
        metadata: std::collections::HashMap::new(),
        change_artifact: None,
        parent_session_key: None,
        tool_use_behavior: Default::default(),
    }
}

fn soul_seg(id: &str, content: &str) -> MemorySegment {
    MemorySegment {
        id: id.to_string(),
        content: content.to_string(),
        priority: 0,
        recency_boost: 1.0,
        char_budget: usize::MAX,
        kind: SegmentKind::Soul,
    }
}

fn evictable_seg(id: &str, content: &str, priority: u8) -> MemorySegment {
    MemorySegment {
        id: id.to_string(),
        content: content.to_string(),
        priority,
        recency_boost: 1.0,
        char_budget: usize::MAX,
        kind: SegmentKind::Custom("test".to_string()),
    }
}

// ── Capturing LLM ─────────────────────────────────────────────────────────────

/// Records every message slice passed to `chat` so tests can inspect them.
struct CapturingLlm {
    all_messages: Mutex<Vec<Vec<serde_json::Value>>>,
}

impl CapturingLlm {
    fn new() -> Self {
        Self { all_messages: Mutex::new(vec![]) }
    }
    fn calls(&self) -> Vec<Vec<serde_json::Value>> {
        self.all_messages.lock().unwrap().clone()
    }
}

#[async_trait]
impl turn::LlmProvider for CapturingLlm {
    async fn chat(
        &self,
        messages: &[serde_json::Value],
        _tools: &[serde_json::Value],
    ) -> Result<ThinkResult, ThinkError> {
        self.all_messages.lock().unwrap().push(messages.to_vec());
        Ok(ThinkResult {
            response: serde_json::json!({"role": "assistant", "content": "done"}),
            tool_calls: vec![],
            tokens: sera_types::runtime::TokenUsage::default(),
            plan: None,
        })
    }
}

// ── Test: memory block prepended as system message ───────────────────────────

#[tokio::test]
async fn memory_block_prepended_as_system_message() {
    let llm = std::sync::Arc::new(CapturingLlm::new());

    struct ArcLlm(std::sync::Arc<CapturingLlm>);
    #[async_trait]
    impl turn::LlmProvider for ArcLlm {
        async fn chat(&self, msgs: &[serde_json::Value], tools: &[serde_json::Value]) -> Result<ThinkResult, ThinkError> {
            self.0.chat(msgs, tools).await
        }
    }

    let mut block = MemoryBlock::new(4096);
    block.push(soul_seg("soul", "You are SERA, a helpful assistant."));
    block.push(evictable_seg("ctx", "Current project: SERA 2.0.", 1));

    let asm = MemoryBlockAssembler::new(block);
    let runtime = DefaultRuntime::new(make_context_engine())
        .with_allow_missing_constitutional_gate(true)
        .with_llm(Box::new(ArcLlm(llm.clone())))
        .with_memory_assembler(asm);

    runtime.execute_turn(make_turn_context()).await.unwrap();

    let calls = llm.calls();
    assert!(!calls.is_empty(), "LLM must have been called");

    let first_call = &calls[0];
    // The first message must be the memory block (role=system).
    let first_msg = &first_call[0];
    assert_eq!(
        first_msg.get("role").and_then(|r| r.as_str()),
        Some("system"),
        "first message must be system (memory block)"
    );
    let content = first_msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
    assert!(content.contains("You are SERA"), "soul content must be in memory block");
    assert!(content.contains("Current project"), "evictable content must be present");
}

// ── Test: no memory assembler → no extra system message ──────────────────────

#[tokio::test]
async fn no_memory_assembler_no_extra_system_message() {
    let llm = std::sync::Arc::new(CapturingLlm::new());

    struct ArcLlm(std::sync::Arc<CapturingLlm>);
    #[async_trait]
    impl turn::LlmProvider for ArcLlm {
        async fn chat(&self, msgs: &[serde_json::Value], tools: &[serde_json::Value]) -> Result<ThinkResult, ThinkError> {
            self.0.chat(msgs, tools).await
        }
    }

    let runtime = DefaultRuntime::new(make_context_engine())
        .with_allow_missing_constitutional_gate(true)
        .with_llm(Box::new(ArcLlm(llm.clone())));
    // No memory assembler attached.

    runtime.execute_turn(make_turn_context()).await.unwrap();

    let calls = llm.calls();
    assert!(!calls.is_empty());
    let first_call = &calls[0];
    // Must start with the user message (no memory system message prepended).
    let first_msg = &first_call[0];
    assert_eq!(
        first_msg.get("role").and_then(|r| r.as_str()),
        Some("user"),
        "without assembler, first message must be user"
    );
}

// ── Test: empty memory block is a no-op ──────────────────────────────────────

#[tokio::test]
async fn empty_memory_block_is_noop() {
    let llm = std::sync::Arc::new(CapturingLlm::new());

    struct ArcLlm(std::sync::Arc<CapturingLlm>);
    #[async_trait]
    impl turn::LlmProvider for ArcLlm {
        async fn chat(&self, msgs: &[serde_json::Value], tools: &[serde_json::Value]) -> Result<ThinkResult, ThinkError> {
            self.0.chat(msgs, tools).await
        }
    }

    // Empty block — no segments.
    let asm = MemoryBlockAssembler::new(MemoryBlock::new(4096));
    let runtime = DefaultRuntime::new(make_context_engine())
        .with_allow_missing_constitutional_gate(true)
        .with_llm(Box::new(ArcLlm(llm.clone())))
        .with_memory_assembler(asm);

    let outcome = runtime.execute_turn(make_turn_context()).await.unwrap();
    assert!(matches!(outcome, TurnOutcome::FinalOutput { .. }), "expected FinalOutput: {:?}", outcome);

    // First message must still be the user message (empty block → no prepend).
    let first_call = &llm.calls()[0];
    let first_msg = &first_call[0];
    assert_eq!(
        first_msg.get("role").and_then(|r| r.as_str()),
        Some("user"),
        "empty block must not prepend system message"
    );
}

// ── Test: overflow N < flush_min_turns does NOT trigger pressure ──────────────

#[tokio::test]
async fn overflow_below_threshold_does_not_trigger_pressure() {
    // Budget=5, flush_min_turns=6. Soul content always over budget.
    // Run 5 turns — pressure must not fire.
    let mut block = MemoryBlock::with_flush_min_turns(5, 6);
    block.push(soul_seg("soul", "This content is way longer than five chars."));
    let asm = MemoryBlockAssembler::new(block);

    let runtime = DefaultRuntime::new(make_context_engine())
        .with_allow_missing_constitutional_gate(true)
        .with_memory_assembler(asm);

    // 5 turns — each iteration overflow_turns increments but never reaches 6.
    for _ in 0..5 {
        let outcome = runtime.execute_turn(make_turn_context()).await.unwrap();
        // Must complete normally (think stub → FinalOutput).
        assert!(matches!(outcome, TurnOutcome::FinalOutput { .. }), "expected FinalOutput");
    }

    // Verify overflow_turns is 5 (not yet at flush_min_turns=6).
    let lock = runtime.memory_assembler().unwrap();
    let guard = lock.lock().unwrap();
    assert_eq!(guard.block().overflow_turns, 5, "overflow_turns must be 5 after 5 turns");
}

// ── Test: overflow for flush_min_turns consecutive turns logs pressure ────────

#[tokio::test]
async fn overflow_at_flush_min_turns_logs_pressure() {
    // Budget=5, flush_min_turns=3. Soul content always over budget.
    // On turn 3, record_turn returns true → tracing::info! fires.
    // We can't intercept the log in tests, so just verify overflow_turns == 3.
    let mut block = MemoryBlock::with_flush_min_turns(5, 3);
    block.push(soul_seg("soul", "Longer than five."));
    let asm = MemoryBlockAssembler::new(block);

    let runtime = DefaultRuntime::new(make_context_engine())
        .with_allow_missing_constitutional_gate(true)
        .with_memory_assembler(asm);

    for _ in 0..3 {
        runtime.execute_turn(make_turn_context()).await.unwrap();
    }

    let lock = runtime.memory_assembler().unwrap();
    let guard = lock.lock().unwrap();
    // After 3 turns over budget, overflow_turns == 3 == flush_min_turns.
    assert_eq!(guard.block().overflow_turns, 3, "overflow_turns must be 3 after 3 over-budget turns");
}

// ── Test: pressure counter resets when block returns under budget ─────────────

#[tokio::test]
async fn pressure_counter_resets_when_under_budget() {
    let mut block = MemoryBlock::with_flush_min_turns(5, 3);
    block.push(soul_seg("soul", "Way longer than five chars for sure."));
    let asm = MemoryBlockAssembler::new(block);

    let runtime = DefaultRuntime::new(make_context_engine())
        .with_allow_missing_constitutional_gate(true)
        .with_memory_assembler(asm);

    // 2 over-budget turns.
    runtime.execute_turn(make_turn_context()).await.unwrap();
    runtime.execute_turn(make_turn_context()).await.unwrap();

    {
        let lock = runtime.memory_assembler().unwrap();
        let mut guard = lock.lock().unwrap();
        assert_eq!(guard.block().overflow_turns, 2);
        // Replace over-budget soul with a short under-budget segment.
        guard.block_mut().segments.clear();
        guard.block_mut().push(evictable_seg("tiny", "Hi.", 1));
    }

    // One more turn — block has a segment and is under budget → overflow_turns resets to 0.
    runtime.execute_turn(make_turn_context()).await.unwrap();

    let lock = runtime.memory_assembler().unwrap();
    let guard = lock.lock().unwrap();
    assert_eq!(guard.block().overflow_turns, 0, "overflow_turns must reset to 0 after under-budget turn");
}
