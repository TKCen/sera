//! Integration test: hook ordering through ChainExecutor + HookRegistry.
//!
//! Builds a real ChainExecutor with a HookRegistry, registers recording hooks
//! at OnLlmStart, PreTool, OnLlmEnd, and PostTurn (turn-end equivalent), then
//! asserts that executing chains at each point fires in the expected order.
//! Also covers the reject short-circuit path.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sera_hooks::{ChainExecutor, Hook, HookRegistry};
use sera_types::hook::{
    HookChain, HookContext, HookInstance, HookMetadata, HookPoint, HookResult,
};

// ── Recording infrastructure ─────────────────────────────────────────────────

type Log = Arc<Mutex<Vec<&'static str>>>;

/// A hook that appends a fixed label to the shared log, then continues.
struct RecordingHook {
    label: &'static str,
    log: Log,
}

#[async_trait]
impl Hook for RecordingHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: self.label.to_string(),
            description: format!("Records '{}' to log", self.label),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
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
        self.log.lock().unwrap().push(self.label);
        Ok(HookResult::pass())
    }
}

/// A hook that appends its label then returns Reject, short-circuiting the chain.
struct RejectingRecordingHook {
    label: &'static str,
    log: Log,
}

#[async_trait]
impl Hook for RejectingRecordingHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: self.label.to_string(),
            description: format!("Records '{}' then rejects", self.label),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
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
        self.log.lock().unwrap().push(self.label);
        Ok(HookResult::reject("test reject"))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn instance(hook_ref: &str) -> HookInstance {
    HookInstance {
        hook_ref: hook_ref.to_string(),
        config: serde_json::Value::Null,
        enabled: true,
    }
}

fn chain_for(name: &str, point: HookPoint, hooks: Vec<HookInstance>) -> HookChain {
    HookChain {
        name: name.to_string(),
        point,
        hooks,
        timeout_ms: 5_000,
        fail_open: false,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Happy-path: execute chains at four points and verify the recorded order.
///
/// Expected sequence:
///   llm_start → tool_called → llm_end → turn_end
#[tokio::test]
async fn hook_ordering_across_multiple_points() {
    let log: Log = Arc::new(Mutex::new(Vec::new()));

    // Build registry with one hook per point.
    let mut registry = HookRegistry::new();
    registry.register(Box::new(RecordingHook {
        label: "llm_start",
        log: log.clone(),
    }));
    registry.register(Box::new(RecordingHook {
        label: "tool_called",
        log: log.clone(),
    }));
    registry.register(Box::new(RecordingHook {
        label: "llm_end",
        log: log.clone(),
    }));
    registry.register(Box::new(RecordingHook {
        label: "turn_end",
        log: log.clone(),
    }));

    let executor = ChainExecutor::new(Arc::new(registry));

    // All chains in a flat slice, ordered as they'd be processed in a turn loop.
    let chains = vec![
        chain_for("on-llm-start", HookPoint::OnLlmStart, vec![instance("llm_start")]),
        chain_for("pre-tool", HookPoint::PreTool, vec![instance("tool_called")]),
        chain_for("on-llm-end", HookPoint::OnLlmEnd, vec![instance("llm_end")]),
        chain_for("post-turn", HookPoint::PostTurn, vec![instance("turn_end")]),
    ];

    // Fire each point in lifecycle order.
    for &point in &[
        HookPoint::OnLlmStart,
        HookPoint::PreTool,
        HookPoint::OnLlmEnd,
        HookPoint::PostTurn,
    ] {
        let ctx = HookContext::new(point);
        let result = executor
            .execute_at_point(point, &chains, ctx)
            .await
            .unwrap();
        assert!(result.is_success(), "expected success at {:?}", point);
    }

    let recorded = log.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec!["llm_start", "tool_called", "llm_end", "turn_end"],
        "hooks fired out of expected order: {:?}",
        recorded
    );
}

/// Each chain can carry multiple hooks; they all fire in registration order.
#[tokio::test]
async fn hook_ordering_within_a_single_chain() {
    let log: Log = Arc::new(Mutex::new(Vec::new()));

    let mut registry = HookRegistry::new();
    registry.register(Box::new(RecordingHook {
        label: "first",
        log: log.clone(),
    }));
    registry.register(Box::new(RecordingHook {
        label: "second",
        log: log.clone(),
    }));
    registry.register(Box::new(RecordingHook {
        label: "third",
        log: log.clone(),
    }));

    let executor = ChainExecutor::new(Arc::new(registry));

    let chain = chain_for(
        "ordered",
        HookPoint::OnLlmStart,
        vec![
            instance("first"),
            instance("second"),
            instance("third"),
        ],
    );

    let ctx = HookContext::new(HookPoint::OnLlmStart);
    let result = executor.execute_chain(&chain, ctx).await.unwrap();
    assert!(result.is_success());
    assert_eq!(result.hooks_executed, 3);

    let recorded = log.lock().unwrap().clone();
    assert_eq!(recorded, vec!["first", "second", "third"]);
}

/// Reject path: a rejecting hook short-circuits; subsequent hooks must NOT fire.
#[tokio::test]
async fn hook_reject_short_circuits_chain() {
    let log: Log = Arc::new(Mutex::new(Vec::new()));

    let mut registry = HookRegistry::new();
    registry.register(Box::new(RecordingHook {
        label: "before_reject",
        log: log.clone(),
    }));
    registry.register(Box::new(RejectingRecordingHook {
        label: "reject_here",
        log: log.clone(),
    }));
    registry.register(Box::new(RecordingHook {
        label: "after_reject",
        log: log.clone(),
    }));

    let executor = ChainExecutor::new(Arc::new(registry));

    let chain = chain_for(
        "reject-chain",
        HookPoint::PreTool,
        vec![
            instance("before_reject"),
            instance("reject_here"),
            instance("after_reject"), // must NOT fire
        ],
    );

    let ctx = HookContext::new(HookPoint::PreTool);
    let result = executor.execute_chain(&chain, ctx).await.unwrap();

    assert!(result.is_rejected(), "expected Reject outcome");
    // Two hooks ran (before + the rejector); the third was suppressed.
    assert_eq!(result.hooks_executed, 2);

    let recorded = log.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec!["before_reject", "reject_here"],
        "after_reject must not appear; got: {:?}",
        recorded
    );
}

/// Reject in a multi-point scenario: rejection at OnLlmStart prevents
/// subsequent points from being reached when iterating manually.
///
/// (ChainExecutor::execute_at_point stops on Reject within the same point.
///  Cross-point gating is the caller's responsibility — verified here by
///  checking the log does not contain entries from later points.)
#[tokio::test]
async fn hook_reject_at_point_does_not_bleed_into_next_point() {
    let log: Log = Arc::new(Mutex::new(Vec::new()));

    let mut registry = HookRegistry::new();
    registry.register(Box::new(RejectingRecordingHook {
        label: "llm_start_reject",
        log: log.clone(),
    }));
    registry.register(Box::new(RecordingHook {
        label: "llm_end_should_not_fire",
        log: log.clone(),
    }));

    let executor = ChainExecutor::new(Arc::new(registry));

    let chains = vec![
        chain_for(
            "start-reject",
            HookPoint::OnLlmStart,
            vec![instance("llm_start_reject")],
        ),
        chain_for(
            "end-chain",
            HookPoint::OnLlmEnd,
            vec![instance("llm_end_should_not_fire")],
        ),
    ];

    // Simulate a caller that checks for rejection before advancing to the next point.
    let ctx_start = HookContext::new(HookPoint::OnLlmStart);
    let start_result = executor
        .execute_at_point(HookPoint::OnLlmStart, &chains, ctx_start)
        .await
        .unwrap();

    assert!(start_result.is_rejected());

    // Caller correctly gates on rejection — does not call OnLlmEnd.
    if !start_result.is_rejected() {
        let ctx_end = HookContext::new(HookPoint::OnLlmEnd);
        executor
            .execute_at_point(HookPoint::OnLlmEnd, &chains, ctx_end)
            .await
            .unwrap();
    }

    let recorded = log.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec!["llm_start_reject"],
        "only the rejecting hook should have fired; got: {:?}",
        recorded
    );
}
