//! Runtime acceptance tests — Lane D, P0-6.

use sera_hooks::ChainExecutor;
use sera_hooks::registry::HookRegistry;
use sera_types::hook::{HookChain, HookInstance, HookMetadata, HookPoint, HookResult};
use sera_types::runtime::{TokenUsage, TurnOutcome};

use sera_runtime::compaction::condensers::*;
use sera_runtime::compaction::{Condenser, PipelineCondenser};
use sera_runtime::context_engine::pipeline::ContextPipeline as ContextEnginePipeline;
use sera_runtime::context_engine::{
    ContextEngine, TokenBudget, MAX_COMPACTION_CHECKPOINTS_PER_SESSION,
};
use sera_runtime::turn::{
    self, ActResult, ReactMode, ThinkResult, TurnContext, DOOM_LOOP_THRESHOLD,
};

use std::collections::HashSet;
use uuid::Uuid;

// ── 1. TurnOutcome replaces TurnResult — compiles ───────────────────────────

#[test]
fn turn_outcome_replaces_turn_result_compiles() {
    let outcome = TurnOutcome::RunAgain {
        tool_calls: vec![],
        tokens_used: TokenUsage::default(),
        duration_ms: 100,
    };
    // Just check it's assignable
    match outcome {
        TurnOutcome::RunAgain { duration_ms, .. } => assert_eq!(duration_ms, 100),
        _ => panic!("expected RunAgain"),
    }
}

// ── 2. ContextEngine trait object safe ──────────────────────────────────────

#[test]
fn context_engine_trait_object_safe() {
    // Box<dyn ContextEngine> must compile
    let engine: Box<dyn ContextEngine> = Box::new(ContextEnginePipeline::new());
    let desc = engine.describe();
    assert_eq!(desc.name, "pipeline");
}

// ── 3. Four-method lifecycle callable ───────────────────────────────────────

#[tokio::test]
async fn four_method_lifecycle_callable() {
    let ctx = TurnContext {
        turn_id: Uuid::new_v4(),
        session_key: "sess-1".into(),
        agent_id: "agent-1".into(),
        messages: vec![serde_json::json!({"role": "user", "content": "hello"})],
        tools: vec![],
        handoffs: vec![],
        watch_signals: HashSet::new(),
        change_artifact: None,
        react_mode: ReactMode::Default,
        doom_loop_count: 0,
        enforcement_mode: sera_hitl::EnforcementMode::Autonomous,
        approval_routing: sera_hitl::ApprovalRouting::Autonomous,
        pending_steer: None,
        tool_use_behavior: Default::default(),
        tool_context: Default::default(),
    };

    let observed = turn::observe(&ctx, None, &[], true).await.unwrap();
    assert_eq!(observed.len(), 1);

    let think_result = turn::think(&observed, &ctx.tools, &ctx.react_mode, None, &Default::default()).await;
    assert!(think_result.tool_calls.is_empty());

    let act_result = turn::act(&mut ctx.clone(), &think_result, None).await;
    matches!(act_result, ActResult::ToolResults(_));

    let outcome = turn::react(&act_result, &think_result, 50, None, &[], true).await;
    matches!(outcome, TurnOutcome::FinalOutput { .. });
}

// ── 4. Doom loop triggers interruption ──────────────────────────────────────

#[tokio::test]
async fn doom_loop_triggers_interruption() {
    let mut ctx = TurnContext {
        turn_id: Uuid::new_v4(),
        session_key: "sess-1".into(),
        agent_id: "agent-1".into(),
        messages: vec![],
        tools: vec![],
        handoffs: vec![],
        watch_signals: HashSet::new(),
        change_artifact: None,
        react_mode: ReactMode::Default,
        doom_loop_count: DOOM_LOOP_THRESHOLD,
        enforcement_mode: sera_hitl::EnforcementMode::Autonomous,
        approval_routing: sera_hitl::ApprovalRouting::Autonomous,
        pending_steer: None,
        tool_use_behavior: Default::default(),
        tool_context: Default::default(),
    };

    let think_result = ThinkResult {
        response: serde_json::json!({}),
        tool_calls: vec![],
        tokens: TokenUsage::default(),
        plan: None,
    };

    let result = turn::act(&mut ctx, &think_result, None).await;
    match result {
        ActResult::Interruption { reason } => {
            assert!(reason.contains("doom loop"));
        }
        _ => panic!("expected Interruption"),
    }
}

// ── 5. NoOp condenser passthrough ───────────────────────────────────────────

#[tokio::test]
async fn no_op_condenser_passthrough() {
    let c = NoOpCondenser;
    let msgs: Vec<serde_json::Value> = (0..5)
        .map(|i| serde_json::json!({"role": "user", "content": format!("msg {i}")}))
        .collect();
    let result = c.condense(msgs.clone()).await;
    assert_eq!(result.len(), 5);
}

// ── 6. Conversation window retains pairs ────────────────────────────────────

#[tokio::test]
async fn conversation_window_condenser_retains_pairs() {
    let c = ConversationWindowCondenser::new(2);
    let msgs = vec![
        serde_json::json!({"role": "system", "content": "sys"}),
        serde_json::json!({"role": "user", "content": "u1"}),
        serde_json::json!({"role": "assistant", "content": "a1"}),
        serde_json::json!({"role": "user", "content": "u2"}),
        serde_json::json!({"role": "assistant", "content": "a2"}),
        serde_json::json!({"role": "user", "content": "u3"}),
        serde_json::json!({"role": "assistant", "content": "a3"}),
    ];
    let result = c.condense(msgs).await;
    // System msg + last 4 non-system msgs (2 pairs)
    assert_eq!(result.len(), 5);
    // No orphaned tool results
    assert!(result
        .iter()
        .all(|m| m.get("role").and_then(|r| r.as_str()) != Some("tool")));
}

// ── 7. Pipeline condenser applies in order ──────────────────────────────────

#[tokio::test]
async fn pipeline_condenser_applies_in_order() {
    let mut pipeline = PipelineCondenser::new();
    // First: keep recent 3, then: no-op
    pipeline.add(Box::new(RecentEventsCondenser::new(3)));
    pipeline.add(Box::new(NoOpCondenser));

    let msgs: Vec<serde_json::Value> = (0..10)
        .map(|i| serde_json::json!({"role": "user", "content": format!("msg {i}")}))
        .collect();

    let result = pipeline.run(msgs).await;
    assert_eq!(result.len(), 3);
}

// ── 8. Compaction checkpoint max per session ────────────────────────────────

#[test]
fn compaction_checkpoint_max_per_session() {
    assert_eq!(MAX_COMPACTION_CHECKPOINTS_PER_SESSION, 25);
}

// ── 9. Context pipeline wraps as context engine ─────────────────────────────

#[tokio::test]
async fn context_pipeline_wraps_as_context_engine() {
    let mut engine: Box<dyn ContextEngine> = Box::new(ContextEnginePipeline::new());
    engine
        .ingest(serde_json::json!({"role": "user", "content": "test"}))
        .await
        .unwrap();
    let window = engine
        .assemble(TokenBudget {
            max_tokens: 10000,
            reserved_for_output: 1000,
        })
        .await
        .unwrap();
    assert_eq!(window.messages.len(), 1);
}

// ── 10. Turn context has change_artifact field ──────────────────────────────

#[test]
fn turn_context_has_change_artifact_field() {
    let ctx = TurnContext {
        turn_id: Uuid::new_v4(),
        session_key: String::new(),
        agent_id: String::new(),
        messages: vec![],
        tools: vec![],
        handoffs: vec![],
        watch_signals: HashSet::new(),
        change_artifact: Some("ca-123".into()),
        react_mode: ReactMode::Default,
        doom_loop_count: 0,
        enforcement_mode: sera_hitl::EnforcementMode::Autonomous,
        approval_routing: sera_hitl::ApprovalRouting::Autonomous,
        pending_steer: None,
        tool_use_behavior: Default::default(),
        tool_context: Default::default(),
    };
    assert_eq!(ctx.change_artifact.as_deref(), Some("ca-123"));
}

// ── ConstitutionalGate enforcement tests (P0-6) ─────────────────────────────

/// Minimal passthrough hook — returns Continue for every invocation.
struct AllowGate;

#[async_trait::async_trait]
impl sera_hooks::hook_trait::Hook for AllowGate {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "allow-gate".to_string(),
            description: "Always allows".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _config: serde_json::Value) -> Result<(), sera_hooks::error::HookError> { Ok(()) }
    async fn execute(&self, _ctx: &sera_types::hook::HookContext) -> Result<HookResult, sera_hooks::error::HookError> {
        Ok(HookResult::pass())
    }
}

/// Minimal deny hook — returns Reject for every invocation.
struct DenyGate;

#[async_trait::async_trait]
impl sera_hooks::hook_trait::Hook for DenyGate {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "deny-gate".to_string(),
            description: "Always denies".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _config: serde_json::Value) -> Result<(), sera_hooks::error::HookError> { Ok(()) }
    async fn execute(&self, _ctx: &sera_types::hook::HookContext) -> Result<HookResult, sera_hooks::error::HookError> {
        Ok(HookResult::reject("constitutional violation"))
    }
}

fn make_ctx(session_key: &str) -> TurnContext {
    TurnContext {
        turn_id: Uuid::new_v4(),
        session_key: session_key.into(),
        agent_id: "agent-gate-test".into(),
        messages: vec![serde_json::json!({"role": "user", "content": "test"})],
        tools: vec![],
        handoffs: vec![],
        watch_signals: HashSet::new(),
        change_artifact: None,
        react_mode: ReactMode::Default,
        doom_loop_count: 0,
        enforcement_mode: sera_hitl::EnforcementMode::Autonomous,
        approval_routing: sera_hitl::ApprovalRouting::Autonomous,
        pending_steer: None,
        tool_use_behavior: Default::default(),
        tool_context: Default::default(),
    }
}

fn make_chain(hook_ref: &str) -> HookChain {
    HookChain {
        name: "gate-chain".to_string(),
        point: HookPoint::ConstitutionalGate,
        hooks: vec![HookInstance {
            hook_ref: hook_ref.to_string(),
            config: serde_json::Value::Null,
            enabled: true,
        }],
        timeout_ms: 5000,
        fail_open: false,
    }
}

// ── 12. Gate registered + permissive → turn proceeds ────────────────────────

#[tokio::test]
async fn constitutional_gate_allow_hook_permits_turn() {
    let mut registry = HookRegistry::new();
    registry.register(Box::new(AllowGate));
    let executor = ChainExecutor::new(std::sync::Arc::new(registry));
    let chains = vec![make_chain("allow-gate")];
    let ctx = make_ctx("sess-gate-allow");

    let result = turn::observe(&ctx, Some(&executor), &chains, false).await;
    assert!(result.is_ok(), "allow gate should permit observe: {result:?}");
}

// ── 13. Gate registered + Deny → Interruption emitted ───────────────────────

#[tokio::test]
async fn constitutional_gate_deny_hook_emits_interruption() {
    let mut registry = HookRegistry::new();
    registry.register(Box::new(DenyGate));
    let executor = ChainExecutor::new(std::sync::Arc::new(registry));
    let chains = vec![make_chain("deny-gate")];
    let ctx = make_ctx("sess-gate-deny");

    let result = turn::observe(&ctx, Some(&executor), &chains, false).await;
    match result {
        Err(TurnOutcome::Interruption { hook_point, reason, .. }) => {
            assert_eq!(hook_point, "constitutional_gate");
            assert!(reason.contains("constitutional violation"), "unexpected reason: {reason}");
        }
        other => panic!("expected Interruption, got {other:?}"),
    }
}

// ── 14. No hook registered + default config → Interruption (fail-closed) ────

#[tokio::test]
async fn constitutional_gate_missing_hook_fail_closed() {
    // No executor, no chains, allow_missing = false (production default).
    let ctx = make_ctx("sess-gate-missing");
    let result = turn::observe(&ctx, None, &[], false).await;
    match result {
        Err(TurnOutcome::Interruption { hook_point, .. }) => {
            assert_eq!(hook_point, "constitutional_gate");
        }
        other => panic!("expected fail-closed Interruption, got {other:?}"),
    }
}

// ── 15. No hook registered + allow_missing = true → turn proceeds ────────────

#[tokio::test]
async fn constitutional_gate_missing_hook_permissive_mode_proceeds() {
    let ctx = make_ctx("sess-gate-permissive");
    let result = turn::observe(&ctx, None, &[], true).await;
    assert!(result.is_ok(), "permissive mode should allow missing gate: {result:?}");
    assert_eq!(result.unwrap().len(), 1);
}

// ── Minimal ModelAdapter for compile-time condenser tests ───────────────────

struct NoOpModelAdapter;

#[async_trait::async_trait]
impl sera_types::model::ModelAdapter for NoOpModelAdapter {
    async fn chat_completion(
        &self,
        _request: sera_types::model::ModelRequest,
    ) -> Result<sera_types::model::ModelResponse, sera_types::model::ModelError> {
        Err(sera_types::model::ModelError::Timeout)
    }
    fn model_name(&self) -> &str { "noop" }
    fn supports_tools(&self) -> bool { false }
    fn supports_streaming(&self) -> bool { false }
    fn max_context_tokens(&self) -> u32 { 4096 }
}

// ── 11. Each condenser compiles and has non-empty name ──────────────────────

#[test]
fn each_condenser_compiles() {
    let condensers: Vec<Box<dyn Condenser>> = vec![
        Box::new(NoOpCondenser),
        Box::new(RecentEventsCondenser::new(10)),
        Box::new(ConversationWindowCondenser::new(5)),
        Box::new(AmortizedForgettingCondenser::new(0.5)),
        Box::new(ObservationMaskingCondenser::new(5)),
        Box::new(BrowserOutputCondenser::new(1000)),
        Box::new(LlmSummarizingCondenser::new(std::sync::Arc::new(NoOpModelAdapter))),
        Box::new(LlmAttentionCondenser::new(std::sync::Arc::new(NoOpModelAdapter))),
        Box::new(StructuredSummaryCondenser::new(std::sync::Arc::new(NoOpModelAdapter))),
    ];
    assert_eq!(condensers.len(), 9);
    for c in &condensers {
        assert!(!c.name().is_empty(), "condenser name must be non-empty");
    }
}
