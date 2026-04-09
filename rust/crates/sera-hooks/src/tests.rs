//! Integration-style tests for the hook registry and chain executor.
//!
//! Test helpers defined here:
//! - `PassthroughHook` — always returns Continue with no updates.
//! - `RejectHook`      — always returns Reject.
//! - `ModifyingHook`   — adds `{"modified": true}` to context metadata.
//! - `FailingHook`     — returns Err from execute().

use std::collections::HashMap;
use std::sync::Arc;

use sera_domain::hook::{
    HookChain, HookContext, HookInstance, HookMetadata, HookPoint, HookResult,
};

use crate::error::HookError;
use crate::executor::ChainExecutor;
use crate::hook_trait::Hook;
use crate::registry::HookRegistry;

// ── Test hook implementations ────────────────────────────────────────────────

struct PassthroughHook;

#[async_trait::async_trait]
impl Hook for PassthroughHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "passthrough".to_string(),
            description: "Always continues".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }
    async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
        Ok(HookResult::pass())
    }
}

struct RejectHook;

#[async_trait::async_trait]
impl Hook for RejectHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "reject".to_string(),
            description: "Always rejects".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }
    async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
        Ok(HookResult::reject("blocked by test"))
    }
}

struct RedirectHook;

#[async_trait::async_trait]
impl Hook for RedirectHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "redirect".to_string(),
            description: "Always redirects".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }
    async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
        Ok(HookResult::redirect("agent:fallback"))
    }
}

struct ModifyingHook;

#[async_trait::async_trait]
impl Hook for ModifyingHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "modifying".to_string(),
            description: "Adds modified=true to metadata".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }
    async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
        let mut updates = HashMap::new();
        updates.insert("modified".to_string(), serde_json::json!(true));
        Ok(HookResult::pass_with(updates))
    }
}

struct FailingHook;

#[async_trait::async_trait]
impl Hook for FailingHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "failing".to_string(),
            description: "Always errors".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }
    async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
        Err(HookError::ExecutionFailed {
            hook: "failing".to_string(),
            reason: "intentional test failure".to_string(),
        })
    }
}

// ── Helper builders ──────────────────────────────────────────────────────────

fn make_registry() -> HookRegistry {
    let mut r = HookRegistry::new();
    r.register(Box::new(PassthroughHook));
    r.register(Box::new(RejectHook));
    r.register(Box::new(RedirectHook));
    r.register(Box::new(ModifyingHook));
    r.register(Box::new(FailingHook));
    r
}

fn instance(hook_ref: &str) -> HookInstance {
    HookInstance {
        hook_ref: hook_ref.to_string(),
        config: serde_json::Value::Null,
        enabled: true,
    }
}

fn disabled_instance(hook_ref: &str) -> HookInstance {
    HookInstance {
        hook_ref: hook_ref.to_string(),
        config: serde_json::Value::Null,
        enabled: false,
    }
}

fn chain(name: &str, point: HookPoint, hooks: Vec<HookInstance>) -> HookChain {
    HookChain {
        name: name.to_string(),
        point,
        hooks,
        timeout_ms: 5000,
        fail_open: false,
    }
}

fn chain_fail_open(name: &str, point: HookPoint, hooks: Vec<HookInstance>) -> HookChain {
    HookChain {
        name: name.to_string(),
        point,
        hooks,
        timeout_ms: 5000,
        fail_open: true,
    }
}

// ── Registry tests ───────────────────────────────────────────────────────────

#[test]
fn registry_register_and_contains() {
    let mut r = HookRegistry::new();
    assert!(!r.contains("passthrough"));
    r.register(Box::new(PassthroughHook));
    assert!(r.contains("passthrough"));
}

#[test]
fn registry_get_returns_hook() {
    let mut r = HookRegistry::new();
    r.register(Box::new(PassthroughHook));
    assert!(r.get("passthrough").is_some());
    assert!(r.get("nonexistent").is_none());
}

#[test]
fn registry_unregister() {
    let mut r = HookRegistry::new();
    r.register(Box::new(PassthroughHook));
    assert!(r.unregister("passthrough"));
    assert!(!r.contains("passthrough"));
    // Unregistering again returns false.
    assert!(!r.unregister("passthrough"));
}

#[test]
fn registry_list() {
    let mut r = HookRegistry::new();
    r.register(Box::new(PassthroughHook));
    r.register(Box::new(RejectHook));
    let names: Vec<String> = r.list().iter().map(|m| m.name.clone()).collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"passthrough".to_string()));
    assert!(names.contains(&"reject".to_string()));
}

#[test]
fn registry_duplicate_name_replaces() {
    let mut r = HookRegistry::new();
    r.register(Box::new(PassthroughHook));
    r.register(Box::new(PassthroughHook)); // second registration
    // Still only one entry.
    assert_eq!(r.list().len(), 1);
}

// ── Chain executor tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn executor_empty_chain_succeeds() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain("empty", HookPoint::PreRoute, vec![]);
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    assert!(result.is_success());
    assert_eq!(result.hooks_executed, 0);
}

#[tokio::test]
async fn executor_single_passthrough_hook() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain("pass-chain", HookPoint::PreRoute, vec![instance("passthrough")]);
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    assert!(result.is_success());
    assert_eq!(result.hooks_executed, 1);
}

#[tokio::test]
async fn executor_multiple_passthrough_hooks() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain(
        "multi",
        HookPoint::PreRoute,
        vec![
            instance("passthrough"),
            instance("passthrough"),
            instance("passthrough"),
        ],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    assert!(result.is_success());
    assert_eq!(result.hooks_executed, 3);
}

#[tokio::test]
async fn executor_short_circuit_on_reject() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain(
        "reject-chain",
        HookPoint::PreRoute,
        vec![
            instance("passthrough"),
            instance("reject"),
            instance("passthrough"), // should not run
        ],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    assert!(result.is_rejected());
    assert_eq!(result.hooks_executed, 2);
}

#[tokio::test]
async fn executor_short_circuit_on_redirect() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain(
        "redirect-chain",
        HookPoint::PreRoute,
        vec![
            instance("passthrough"),
            instance("redirect"),
            instance("passthrough"), // should not run
        ],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    assert!(result.is_redirected());
    assert_eq!(result.hooks_executed, 2);
}

#[tokio::test]
async fn executor_fail_open_skips_erroring_hook() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain_fail_open(
        "fail-open",
        HookPoint::PreRoute,
        vec![
            instance("passthrough"),
            instance("failing"),
            instance("passthrough"),
        ],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    assert!(result.is_success());
    // passthrough + passthrough ran; failing was skipped (counted)
    assert_eq!(result.hooks_executed, 3);
}

#[tokio::test]
async fn executor_fail_closed_propagates_error() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain(
        "fail-closed",
        HookPoint::PreRoute,
        vec![instance("passthrough"), instance("failing")],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let err = executor.execute_chain(&c, ctx).await.unwrap_err();
    assert!(matches!(err, HookError::ExecutionFailed { .. }));
}

#[tokio::test]
async fn executor_disabled_hooks_skipped() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain(
        "disabled",
        HookPoint::PreRoute,
        vec![
            disabled_instance("reject"), // should be skipped
            instance("passthrough"),
        ],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    // reject was skipped; only passthrough ran
    assert!(result.is_success());
    assert_eq!(result.hooks_executed, 1);
}

#[tokio::test]
async fn executor_context_modifications_propagate() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain(
        "modify-chain",
        HookPoint::PreRoute,
        vec![instance("modifying"), instance("passthrough")],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    assert!(result.is_success());
    assert_eq!(
        result.context.metadata.get("modified"),
        Some(&serde_json::json!(true))
    );
}

#[tokio::test]
async fn executor_hook_not_found_fail_closed() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain(
        "missing",
        HookPoint::PreRoute,
        vec![instance("nonexistent")],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let err = executor.execute_chain(&c, ctx).await.unwrap_err();
    assert!(matches!(err, HookError::HookNotFound { .. }));
}

#[tokio::test]
async fn executor_hook_not_found_fail_open() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain_fail_open(
        "missing-open",
        HookPoint::PreRoute,
        vec![instance("nonexistent"), instance("passthrough")],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    assert!(result.is_success());
    assert_eq!(result.hooks_executed, 1);
}

#[tokio::test]
async fn executor_chain_timeout() {
    use tokio::time::sleep;

    struct SlowHook;
    #[async_trait::async_trait]
    impl Hook for SlowHook {
        fn metadata(&self) -> HookMetadata {
            HookMetadata {
                name: "slow".to_string(),
                description: "Sleeps forever".to_string(),
                version: "0.1.0".to_string(),
                supported_points: HookPoint::ALL.to_vec(),
                author: None,
            }
        }
        async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
            Ok(())
        }
        async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
            sleep(std::time::Duration::from_secs(60)).await;
            Ok(HookResult::pass())
        }
    }

    let mut registry = HookRegistry::new();
    registry.register(Box::new(SlowHook));
    let executor = ChainExecutor::new(Arc::new(registry));

    let c = HookChain {
        name: "timeout-chain".to_string(),
        point: HookPoint::PreRoute,
        hooks: vec![instance("slow")],
        timeout_ms: 50, // very short timeout
        fail_open: false,
    };
    let ctx = HookContext::new(HookPoint::PreRoute);
    let err = executor.execute_chain(&c, ctx).await.unwrap_err();
    assert!(matches!(err, HookError::ChainTimeout { .. }));
}

// ── execute_at_point tests ───────────────────────────────────────────────────

#[tokio::test]
async fn execute_at_point_filters_by_point() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));

    let chains = vec![
        chain("pre-route-chain", HookPoint::PreRoute, vec![instance("passthrough")]),
        chain("post-route-chain", HookPoint::PostRoute, vec![instance("reject")]),
    ];

    let ctx = HookContext::new(HookPoint::PreRoute);
    // Only the PreRoute chain should run; the PostRoute reject chain is ignored.
    let result = executor
        .execute_at_point(HookPoint::PreRoute, &chains, ctx)
        .await
        .unwrap();
    assert!(result.is_success());
    assert_eq!(result.hooks_executed, 1);
}

#[tokio::test]
async fn execute_at_point_no_matching_chains() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let chains: Vec<HookChain> = vec![];
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor
        .execute_at_point(HookPoint::PreRoute, &chains, ctx)
        .await
        .unwrap();
    assert!(result.is_success());
    assert_eq!(result.hooks_executed, 0);
}

#[tokio::test]
async fn execute_at_point_multiple_matching_chains_sequential() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));

    let chains = vec![
        chain("chain-a", HookPoint::PreRoute, vec![instance("modifying")]),
        chain("chain-b", HookPoint::PreRoute, vec![instance("passthrough")]),
    ];

    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor
        .execute_at_point(HookPoint::PreRoute, &chains, ctx)
        .await
        .unwrap();
    assert!(result.is_success());
    assert_eq!(result.hooks_executed, 2);
    // Context modification from chain-a propagated into chain-b's context.
    assert_eq!(
        result.context.metadata.get("modified"),
        Some(&serde_json::json!(true))
    );
}

#[tokio::test]
async fn execute_at_point_stops_on_reject() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));

    let chains = vec![
        chain("reject-chain", HookPoint::PreRoute, vec![instance("reject")]),
        chain("never-runs", HookPoint::PreRoute, vec![instance("passthrough")]),
    ];

    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor
        .execute_at_point(HookPoint::PreRoute, &chains, ctx)
        .await
        .unwrap();
    assert!(result.is_rejected());
    assert_eq!(result.hooks_executed, 1);
}
