//! Integration-style tests for the hook registry and chain executor.
//!
//! Test helpers defined here:
//! - `PassthroughHook` — always returns Continue with no updates.
//! - `RejectHook`      — always returns Reject.
//! - `ModifyingHook`   — adds `{"modified": true}` to context metadata.
//! - `FailingHook`     — returns Err from execute().

use std::collections::HashMap;
use std::sync::Arc;

use sera_types::hook::{
    CHAIN_ABORTED_CODE, HookChain, HookContext, HookInstance, HookMetadata, HookPoint, HookResult,
    PermissionOverrides,
};

use crate::cancel::HookCancellation;
use crate::error::HookError;
use crate::executor::ChainExecutor;
use crate::hook_trait::Hook;
use crate::registry::{HookRegistry, HookTier};

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
    let c = chain(
        "pass-chain",
        HookPoint::PreRoute,
        vec![instance("passthrough")],
    );
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
        chain(
            "pre-route-chain",
            HookPoint::PreRoute,
            vec![instance("passthrough")],
        ),
        chain(
            "post-route-chain",
            HookPoint::PostRoute,
            vec![instance("reject")],
        ),
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
        chain(
            "chain-b",
            HookPoint::PreRoute,
            vec![instance("passthrough")],
        ),
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
        chain(
            "reject-chain",
            HookPoint::PreRoute,
            vec![instance("reject")],
        ),
        chain(
            "never-runs",
            HookPoint::PreRoute,
            vec![instance("passthrough")],
        ),
    ];

    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor
        .execute_at_point(HookPoint::PreRoute, &chains, ctx)
        .await
        .unwrap();
    assert!(result.is_rejected());
    assert_eq!(result.hooks_executed, 1);
}

// ── HookAbortSignal tests ────────────────────────────────────────────────────

use crate::HookAbortSignal;

struct AbortingHook;

#[async_trait::async_trait]
impl Hook for AbortingHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "aborting".to_string(),
            description: "Raises a pipeline abort".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }
    async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
        Err(HookError::Aborted {
            hook: "aborting".to_string(),
            reason: "policy violation".to_string(),
            signal: HookAbortSignal::with_code("policy violation", "policy_violation"),
        })
    }
}

#[tokio::test]
async fn hook_abort_signal_propagates_even_with_fail_open() {
    // A normal execution error would be swallowed by fail_open, but
    // HookError::Aborted is a pipeline-abort signal that must propagate.
    let mut r = HookRegistry::new();
    r.register(Box::new(AbortingHook));
    r.register(Box::new(PassthroughHook));
    let executor = ChainExecutor::new(Arc::new(r));

    let chain = chain_fail_open(
        "abort-chain",
        HookPoint::PreRoute,
        vec![instance("aborting"), instance("passthrough")],
    );

    let ctx = HookContext::new(HookPoint::PreRoute);
    let err = executor.execute_chain(&chain, ctx).await.unwrap_err();
    match err {
        HookError::Aborted {
            hook,
            reason,
            signal,
        } => {
            assert_eq!(hook, "aborting");
            assert_eq!(reason, "policy violation");
            assert_eq!(signal.code.as_deref(), Some("policy_violation"));
        }
        other => panic!("expected HookError::Aborted, got {:?}", other),
    }
}

#[test]
fn hook_abort_signal_constructors() {
    let plain = HookAbortSignal::new("stop");
    assert_eq!(plain.reason, "stop");
    assert!(plain.code.is_none());

    let coded = HookAbortSignal::with_code("stop", "E_STOP");
    assert_eq!(coded.reason, "stop");
    assert_eq!(coded.code.as_deref(), Some("E_STOP"));
}

// ── PermissionOverrides tests ────────────────────────────────────────────────

/// A hook that grants a fixed set of permissions.
struct GrantingHook {
    grants: Vec<String>,
}

#[async_trait::async_trait]
impl Hook for GrantingHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "granting".to_string(),
            description: "Grants permissions".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }
    async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
        Ok(HookResult::pass_with_permissions(
            PermissionOverrides::grant(self.grants.clone()),
        ))
    }
}

/// A hook that revokes a fixed set of permissions.
struct RevokingHook {
    revokes: Vec<String>,
}

#[async_trait::async_trait]
impl Hook for RevokingHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "revoking".to_string(),
            description: "Revokes permissions".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }
    async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
        Ok(HookResult::pass_with_permissions(
            PermissionOverrides::revoke(self.revokes.clone()),
        ))
    }
}

/// Asserts that a specific set of permissions is present and returns a
/// passthrough result. Used to verify that downstream hooks see merged grants.
struct AssertPermissionsHook {
    expected_present: Vec<String>,
    expected_absent: Vec<String>,
}

#[async_trait::async_trait]
impl Hook for AssertPermissionsHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "assert-permissions".to_string(),
            description: "Asserts expected permissions are active".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }
    async fn execute(&self, ctx: &HookContext) -> Result<HookResult, HookError> {
        for p in &self.expected_present {
            if !ctx.has_permission(p) {
                return Err(HookError::ExecutionFailed {
                    hook: "assert-permissions".to_string(),
                    reason: format!(
                        "expected permission '{p}' to be present; active: {:?}",
                        ctx.active_permissions()
                    ),
                });
            }
        }
        for p in &self.expected_absent {
            if ctx.has_permission(p) {
                return Err(HookError::ExecutionFailed {
                    hook: "assert-permissions".to_string(),
                    reason: format!("expected permission '{p}' to be absent but was present"),
                });
            }
        }
        Ok(HookResult::pass())
    }
}

#[tokio::test]
async fn permission_overrides_merge_into_downstream_context() {
    // Chain: grant [a,b] -> assert [a,b] present -> revoke [a] -> assert a absent, b present
    let mut r = HookRegistry::new();
    r.register(Box::new(GrantingHook {
        grants: vec!["tool:bash:read".to_string(), "tool:bash:write".to_string()],
    }));
    // register the assert hooks under distinct names
    struct AssertPresentAB;
    #[async_trait::async_trait]
    impl Hook for AssertPresentAB {
        fn metadata(&self) -> HookMetadata {
            HookMetadata {
                name: "assert-present-ab".to_string(),
                description: "Assert grants a and b".to_string(),
                version: "0.1.0".to_string(),
                supported_points: HookPoint::ALL.to_vec(),
                author: None,
            }
        }
        async fn init(&mut self, _c: serde_json::Value) -> Result<(), HookError> {
            Ok(())
        }
        async fn execute(&self, ctx: &HookContext) -> Result<HookResult, HookError> {
            assert!(ctx.has_permission("tool:bash:read"));
            assert!(ctx.has_permission("tool:bash:write"));
            Ok(HookResult::pass())
        }
    }
    r.register(Box::new(AssertPresentAB));
    r.register(Box::new(RevokingHook {
        revokes: vec!["tool:bash:read".to_string()],
    }));
    r.register(Box::new(AssertPermissionsHook {
        expected_present: vec!["tool:bash:write".to_string()],
        expected_absent: vec!["tool:bash:read".to_string()],
    }));

    let executor = ChainExecutor::new(Arc::new(r));
    let c = chain(
        "perm-merge",
        HookPoint::PreRoute,
        vec![
            instance("granting"),
            instance("assert-present-ab"),
            instance("revoking"),
            instance("assert-permissions"),
        ],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    assert!(result.is_success());
    assert_eq!(result.hooks_executed, 4);
    assert!(result.context.has_permission("tool:bash:write"));
    assert!(!result.context.has_permission("tool:bash:read"));
}

#[tokio::test]
async fn permission_overrides_no_op_when_absent() {
    // Passthrough hook returns no overrides — context.permissions should
    // remain empty and untouched.
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain(
        "perm-noop",
        HookPoint::PreRoute,
        vec![instance("passthrough"), instance("modifying")],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    assert!(result.is_success());
    assert!(
        result.context.active_permissions().is_empty(),
        "permissions should remain empty, got {:?}",
        result.context.active_permissions()
    );
}

#[tokio::test]
async fn permission_overrides_with_ttl_tracked() {
    // The executor exposes ttl via PermissionOverrides but does not yet
    // prune expired entries (documented behaviour). Verify that a hook
    // returning a TTL'd grant results in the permission being active after
    // the hook runs — TTL expiry is a downstream pruning concern.
    use std::time::Duration;

    struct GrantingWithTtl;
    #[async_trait::async_trait]
    impl Hook for GrantingWithTtl {
        fn metadata(&self) -> HookMetadata {
            HookMetadata {
                name: "granting-ttl".to_string(),
                description: "Grants with TTL".to_string(),
                version: "0.1.0".to_string(),
                supported_points: HookPoint::ALL.to_vec(),
                author: None,
            }
        }
        async fn init(&mut self, _c: serde_json::Value) -> Result<(), HookError> {
            Ok(())
        }
        async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
            Ok(HookResult::pass_with_permissions(
                PermissionOverrides::grant(["ephemeral:token"]).with_ttl(Duration::from_millis(50)),
            ))
        }
    }

    let mut r = HookRegistry::new();
    r.register(Box::new(GrantingWithTtl));
    r.register(Box::new(PassthroughHook));
    let executor = ChainExecutor::new(Arc::new(r));
    let c = chain(
        "ttl-chain",
        HookPoint::PreRoute,
        vec![instance("granting-ttl"), instance("passthrough")],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    assert!(result.is_success());
    // The grant is active after the hook ran.
    assert!(result.context.has_permission("ephemeral:token"));
}

// ── updated_input propagation tests ──────────────────────────────────────────

struct InputSetterHook {
    value: serde_json::Value,
}

#[async_trait::async_trait]
impl Hook for InputSetterHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "input-setter".to_string(),
            description: "Sets updated_input".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _c: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }
    async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
        Ok(HookResult::pass_with_input(self.value.clone()))
    }
}

struct InputAssertHook {
    expected: serde_json::Value,
}

#[async_trait::async_trait]
impl Hook for InputAssertHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "input-assert".to_string(),
            description: "Asserts updated_input".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _c: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }
    async fn execute(&self, ctx: &HookContext) -> Result<HookResult, HookError> {
        match ctx.updated_input() {
            Some(v) if v == &self.expected => Ok(HookResult::pass()),
            other => Err(HookError::ExecutionFailed {
                hook: "input-assert".to_string(),
                reason: format!(
                    "expected updated_input={:?}, got {:?}",
                    self.expected, other
                ),
            }),
        }
    }
}

#[tokio::test]
async fn updated_input_flows_from_hook_n_to_hook_n_plus_one() {
    let mut r = HookRegistry::new();
    r.register(Box::new(InputSetterHook {
        value: serde_json::json!({"transformed": true, "count": 1}),
    }));
    r.register(Box::new(InputAssertHook {
        expected: serde_json::json!({"transformed": true, "count": 1}),
    }));
    let executor = ChainExecutor::new(Arc::new(r));
    let c = chain(
        "input-chain",
        HookPoint::PreRoute,
        vec![instance("input-setter"), instance("input-assert")],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    assert!(result.is_success());
    assert_eq!(result.hooks_executed, 2);
    assert_eq!(
        result.updated_input,
        Some(serde_json::json!({"transformed": true, "count": 1}))
    );
    assert_eq!(
        result.context.updated_input().cloned(),
        Some(serde_json::json!({"transformed": true, "count": 1}))
    );
}

// ── HookCancellation tests ───────────────────────────────────────────────────

/// A hook that sleeps long enough to reliably observe a cancellation.
struct SleepyHook;

#[async_trait::async_trait]
impl Hook for SleepyHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: "sleepy".to_string(),
            description: "Sleeps for 2s".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _c: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }
    async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        Ok(HookResult::pass())
    }
}

#[tokio::test]
async fn cancellation_fires_mid_chain_returns_aborted_outcome() {
    let mut r = HookRegistry::new();
    r.register(Box::new(SleepyHook));
    let executor = ChainExecutor::new(Arc::new(r));

    let c = HookChain {
        name: "mid-cancel".to_string(),
        point: HookPoint::PreRoute,
        hooks: vec![instance("sleepy")],
        timeout_ms: 10_000, // long enough that cancellation wins over timeout
        fail_open: false,
    };

    let cancel = HookCancellation::new();
    let cancel_handle = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        cancel_handle.cancel();
    });

    let ctx = HookContext::new(HookPoint::PreRoute);
    let start = std::time::Instant::now();
    let result = executor
        .execute_chain_cancellable(&c, ctx, cancel)
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert!(
        result.is_aborted(),
        "expected aborted, got {:?}",
        result.outcome
    );
    // The in-flight hook never completed, so hooks_executed stays at 0.
    assert_eq!(result.hooks_executed, 0);
    // Sanity: we did not wait for the 2s sleep.
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "cancellation should abort promptly, took {:?}",
        elapsed
    );

    // Confirm the reject carries the reserved code.
    if let HookResult::Reject { code, .. } = &result.outcome {
        assert_eq!(code.as_deref(), Some(CHAIN_ABORTED_CODE));
    } else {
        panic!("expected Reject outcome, got {:?}", result.outcome);
    }
}

#[tokio::test]
async fn cancellation_fires_before_first_hook_exits_cleanly() {
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain(
        "pre-cancel",
        HookPoint::PreRoute,
        vec![instance("passthrough"), instance("passthrough")],
    );

    // Already-cancelled signal.
    let cancel = HookCancellation::already_cancelled();
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor
        .execute_chain_cancellable(&c, ctx, cancel)
        .await
        .unwrap();

    assert!(result.is_aborted());
    assert_eq!(result.hooks_executed, 0, "no hook should run");
}

#[tokio::test]
async fn cancellation_uncancelled_behaves_like_normal_execute() {
    // Passing an un-cancelled signal into the cancellable API must behave
    // exactly like the regular execute_chain.
    let executor = ChainExecutor::new(Arc::new(make_registry()));
    let c = chain(
        "no-cancel",
        HookPoint::PreRoute,
        vec![instance("modifying"), instance("passthrough")],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor
        .execute_chain_cancellable(&c, ctx, HookCancellation::new())
        .await
        .unwrap();

    assert!(result.is_success());
    assert_eq!(result.hooks_executed, 2);
    assert_eq!(
        result.context.metadata.get("modified"),
        Some(&serde_json::json!(true))
    );
}

#[tokio::test]
async fn execute_at_point_cancellable_propagates_abort() {
    // Covers the multi-chain cancellation path in execute_at_point_cancellable.
    let mut r = HookRegistry::new();
    r.register(Box::new(SleepyHook));
    r.register(Box::new(PassthroughHook));
    let executor = ChainExecutor::new(Arc::new(r));

    let chains = vec![
        HookChain {
            name: "slow".to_string(),
            point: HookPoint::PreRoute,
            hooks: vec![instance("sleepy")],
            timeout_ms: 10_000,
            fail_open: false,
        },
        HookChain {
            name: "unreached".to_string(),
            point: HookPoint::PreRoute,
            hooks: vec![instance("passthrough")],
            timeout_ms: 1_000,
            fail_open: false,
        },
    ];

    let cancel = HookCancellation::new();
    let handle = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        handle.cancel();
    });

    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor
        .execute_at_point_cancellable(HookPoint::PreRoute, &chains, ctx, cancel)
        .await
        .unwrap();
    assert!(result.is_aborted());
}

// ── HookTier tests ────────────────────────────────────────────────────────────

#[test]
fn register_with_tier_internal_and_plugin_are_separate_buckets() {
    let mut r = HookRegistry::new();
    r.register_with_tier(Box::new(PassthroughHook), HookTier::Internal);
    r.register_with_tier(Box::new(RejectHook), HookTier::Plugin);

    let internal = r.list_by_tier(HookTier::Internal);
    let plugin = r.list_by_tier(HookTier::Plugin);

    assert_eq!(internal.len(), 1);
    assert_eq!(internal[0].name, "passthrough");
    assert_eq!(plugin.len(), 1);
    assert_eq!(plugin[0].name, "reject");

    assert_eq!(r.tier("passthrough"), Some(HookTier::Internal));
    assert_eq!(r.tier("reject"), Some(HookTier::Plugin));
}

#[test]
fn back_compat_register_defaults_to_internal() {
    let mut r = HookRegistry::new();
    r.register(Box::new(PassthroughHook));
    assert_eq!(r.tier("passthrough"), Some(HookTier::Internal));
    assert_eq!(r.list_by_tier(HookTier::Internal).len(), 1);
    assert_eq!(r.list_by_tier(HookTier::Plugin).len(), 0);
}

/// A hook that appends its name to a shared execution log.
struct LoggingHook {
    name: String,
    log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl Hook for LoggingHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: self.name.clone(),
            description: "Logs execution order".to_string(),
            version: "0.1.0".to_string(),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }
    async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }
    async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
        self.log.lock().unwrap().push(self.name.clone());
        Ok(HookResult::pass())
    }
}

#[tokio::test]
async fn execute_chain_runs_internal_before_plugin() {
    // Chain order: plugin-first, internal-second — executor must reorder so
    // internal runs first.
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    let mut r = HookRegistry::new();
    r.register_with_tier(
        Box::new(LoggingHook {
            name: "plugin-hook".to_string(),
            log: log.clone(),
        }),
        HookTier::Plugin,
    );
    r.register_with_tier(
        Box::new(LoggingHook {
            name: "internal-hook".to_string(),
            log: log.clone(),
        }),
        HookTier::Internal,
    );

    let executor = ChainExecutor::new(Arc::new(r));

    // Deliberately put plugin-hook first in the chain definition.
    let c = chain(
        "tier-order",
        HookPoint::PreRoute,
        vec![instance("plugin-hook"), instance("internal-hook")],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();
    assert!(result.is_success());
    assert_eq!(result.hooks_executed, 2);

    let order = log.lock().unwrap().clone();
    assert_eq!(
        order,
        vec!["internal-hook", "plugin-hook"],
        "internal hook must run before plugin hook regardless of chain order"
    );
}

#[tokio::test]
async fn plugin_tier_cancel_does_not_block_remaining_plugin_hooks() {
    // A Plugin-tier hook that returns a terminal Reject result short-circuits
    // within the plugin pass, but Internal hooks have already completed.
    // Verify that: (a) internal hook ran, (b) rejecting plugin hook ran,
    // (c) subsequent plugin hook did NOT run (chain short-circuits on Reject).
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    struct LogAndRejectHook {
        log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }
    #[async_trait::async_trait]
    impl Hook for LogAndRejectHook {
        fn metadata(&self) -> HookMetadata {
            HookMetadata {
                name: "plugin-reject".to_string(),
                description: "Logs then rejects".to_string(),
                version: "0.1.0".to_string(),
                supported_points: HookPoint::ALL.to_vec(),
                author: None,
            }
        }
        async fn init(&mut self, _c: serde_json::Value) -> Result<(), HookError> {
            Ok(())
        }
        async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
            self.log.lock().unwrap().push("plugin-reject".to_string());
            Ok(HookResult::reject("plugin blocked"))
        }
    }

    let mut r = HookRegistry::new();
    r.register_with_tier(
        Box::new(LoggingHook {
            name: "internal-first".to_string(),
            log: log.clone(),
        }),
        HookTier::Internal,
    );
    r.register_with_tier(
        Box::new(LogAndRejectHook { log: log.clone() }),
        HookTier::Plugin,
    );
    r.register_with_tier(
        Box::new(LoggingHook {
            name: "plugin-after-reject".to_string(),
            log: log.clone(),
        }),
        HookTier::Plugin,
    );

    let executor = ChainExecutor::new(Arc::new(r));
    let c = chain(
        "plugin-cancel-test",
        HookPoint::PreRoute,
        vec![
            instance("plugin-reject"),       // listed first but is Plugin tier
            instance("internal-first"),      // listed second but is Internal tier
            instance("plugin-after-reject"), // Plugin, should not run after reject
        ],
    );
    let ctx = HookContext::new(HookPoint::PreRoute);
    let result = executor.execute_chain(&c, ctx).await.unwrap();

    // Chain is rejected by the plugin hook.
    assert!(result.is_rejected());

    let order = log.lock().unwrap().clone();
    // internal-first ran (Internal tier, before Plugin), then plugin-reject ran,
    // then plugin-after-reject did NOT run (chain short-circuited on Reject).
    assert_eq!(
        order,
        vec!["internal-first", "plugin-reject"],
        "internal hook must precede plugin hooks; plugin-after-reject must not run after reject"
    );
}
