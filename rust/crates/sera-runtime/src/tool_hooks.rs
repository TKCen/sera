//! Pre/post tool execution hooks for the agent runtime (GH#544 — bead sera-ddz).
//!
//! The existing `sera-hooks` [`Hook`] trait targets the 20-point hook pipeline
//! with WASM-capable [`HookContext`] values. That shape is overkill for
//! in-process observation/sanitization of a single tool call — which is why
//! this module defines a thinner, typed `ToolHook` contract.
//!
//! Hooks live in a [`ToolHookRegistry`]. The dispatcher (see
//! `tools::dispatcher`) consults the registry before + after every tool
//! execution when one is attached.
//!
//! # Relationship to `sera-hooks`
//!
//! A future enhancement can bridge a `ToolHook` into the
//! [`sera_hooks::Hook`] chain at [`HookPoint::PreTool`] / [`PostTool`] via a
//! thin adapter — the shapes are deliberately compatible (pre observes the
//! call, post observes the result, both can short-circuit with a reason).
//! That wiring is NOT done here to keep the initial landing tight; see
//! bead sera-ddz's note.
//!
//! [`Hook`]: sera_hooks::Hook
//! [`HookContext`]: sera_types::hook::HookContext
//! [`HookPoint::PreTool`]: sera_types::hook::HookPoint::PreTool
//! [`PostTool`]: sera_types::hook::HookPoint::PostTool

use std::sync::Arc;

use async_trait::async_trait;
use sera_types::tool::{ToolContext, ToolInput, ToolOutput};
use tokio::sync::RwLock;
use tracing::debug;

// ── ToolCallCtx ─────────────────────────────────────────────────────────────

/// Read-only view of a tool call handed to a hook.
///
/// Kept separate from [`ToolContext`] so the registry doesn't depend on
/// construction internals of the dispatcher.
#[derive(Debug, Clone)]
pub struct ToolCallCtx<'a> {
    /// The tool input (name + JSON args + call ID).
    pub input: &'a ToolInput,
    /// The tool execution context (principal, session, policy, authz).
    pub tool_ctx: &'a ToolContext,
}

impl<'a> ToolCallCtx<'a> {
    /// Construct a new hook context view.
    pub fn new(input: &'a ToolInput, tool_ctx: &'a ToolContext) -> Self {
        Self { input, tool_ctx }
    }
}

// ── ToolHookOutcome ─────────────────────────────────────────────────────────

/// Result of running a pre or post hook.
#[derive(Debug, Clone)]
pub enum ToolHookOutcome {
    /// Allow the operation to continue.
    Continue,
    /// Short-circuit the operation with a human-readable reason.
    ///
    /// In the pre phase this prevents the tool from running and surfaces a
    /// `ToolError::AbortedByHook`. In the post phase this is an observation
    /// only — post hooks MUST NOT change the execution result; an `Abort` in
    /// the post phase is logged but the caller still receives the original
    /// result (see [`ToolHookRegistry::post_all`]).
    Abort(String),
    /// Replace the tool input before execution (pre phase only). Ignored in
    /// the post phase. The variant exists so call-site code can branch on it
    /// once a concrete hook opts in; default built-in hooks never emit it.
    MutateInput(ToolInput),
}

impl ToolHookOutcome {
    /// True if the outcome permits the chain to continue.
    pub fn is_continue(&self) -> bool {
        matches!(self, ToolHookOutcome::Continue | ToolHookOutcome::MutateInput(_))
    }
}

// ── ToolHook trait ──────────────────────────────────────────────────────────

/// Pre + post tool execution hook.
///
/// Implementors typically produce audit / telemetry side effects. A hook that
/// returns [`ToolHookOutcome::Abort`] in [`ToolHook::pre`] cancels the call
/// and surfaces the reason to the caller. Post hooks are observation-only.
#[async_trait]
pub trait ToolHook: Send + Sync {
    /// A stable identifier used for unregister + diagnostics.
    fn id(&self) -> &str;

    /// Called before the tool executes.
    async fn pre(&self, ctx: &ToolCallCtx<'_>) -> ToolHookOutcome;

    /// Called after the tool executes, with the result.
    ///
    /// The default impl is a no-op so trivial audit-only hooks only need to
    /// override `pre`.
    async fn post(
        &self,
        _ctx: &ToolCallCtx<'_>,
        _result: &ToolOutput,
    ) -> ToolHookOutcome {
        ToolHookOutcome::Continue
    }
}

// ── Registry ────────────────────────────────────────────────────────────────

/// Registered entry — stores the hook behind `Arc` so `pre_all` / `post_all`
/// can release the outer lock while awaiting hook futures.
#[derive(Clone)]
struct RegisteredHook {
    id: String,
    hook: Arc<dyn ToolHook>,
}

/// Registry of [`ToolHook`] implementors consulted by the dispatcher.
///
/// Registration order is preserved; `pre_all` / `post_all` iterate in that
/// order and short-circuit on the first non-continue outcome (pre only).
#[derive(Default, Clone)]
pub struct ToolHookRegistry {
    hooks: Arc<RwLock<Vec<RegisteredHook>>>,
}

impl ToolHookRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a hook. Duplicate IDs replace the earlier registration.
    pub async fn register(&self, hook: Arc<dyn ToolHook>) {
        let id = hook.id().to_string();
        let mut guard = self.hooks.write().await;
        if let Some(pos) = guard.iter().position(|h| h.id == id) {
            guard[pos] = RegisteredHook { id, hook };
        } else {
            guard.push(RegisteredHook { id, hook });
        }
    }

    /// Remove a hook by ID. Returns `true` if removed.
    pub async fn unregister(&self, id: &str) -> bool {
        let mut guard = self.hooks.write().await;
        let before = guard.len();
        guard.retain(|h| h.id != id);
        guard.len() != before
    }

    /// Number of registered hooks.
    pub async fn len(&self) -> usize {
        self.hooks.read().await.len()
    }

    /// True if no hooks are registered.
    pub async fn is_empty(&self) -> bool {
        self.hooks.read().await.is_empty()
    }

    /// Run all `pre` hooks in registration order.
    ///
    /// Returns:
    /// - `ToolHookOutcome::Continue` if all hooks allowed through.
    /// - `ToolHookOutcome::Abort(reason)` on the first abort (remaining hooks
    ///   are skipped).
    /// - `ToolHookOutcome::MutateInput(..)` if a hook mutated the input; the
    ///   caller should use the returned input for execution. Subsequent
    ///   mutations chain on top of each other.
    pub async fn pre_all(&self, ctx: &ToolCallCtx<'_>) -> ToolHookOutcome {
        // Snapshot the list so we can release the lock before awaiting hook
        // futures (hooks MUST NOT be able to acquire the registry lock).
        let snapshot: Vec<_> = self.hooks.read().await.clone();
        let mut final_outcome = ToolHookOutcome::Continue;

        for entry in snapshot {
            let outcome = entry.hook.pre(ctx).await;
            match outcome {
                ToolHookOutcome::Continue => {}
                ToolHookOutcome::MutateInput(ref new_input) => {
                    debug!(hook = %entry.id, "tool-hook mutated input");
                    // Preserve the latest mutation; later hooks see the new
                    // input only if the caller re-enters `pre_all` with it.
                    final_outcome = ToolHookOutcome::MutateInput(new_input.clone());
                }
                ToolHookOutcome::Abort(reason) => {
                    debug!(hook = %entry.id, %reason, "tool-hook aborted call");
                    return ToolHookOutcome::Abort(reason);
                }
            }
        }
        final_outcome
    }

    /// Run all `post` hooks in registration order.
    ///
    /// Post hooks are observation-only: an `Abort` is logged but the caller
    /// still receives the original result. The returned outcome reflects
    /// whether any hook requested abort — callers that want strict enforcement
    /// can opt in by inspecting it.
    pub async fn post_all(
        &self,
        ctx: &ToolCallCtx<'_>,
        result: &ToolOutput,
    ) -> ToolHookOutcome {
        let snapshot: Vec<_> = self.hooks.read().await.clone();
        let mut saw_abort: Option<String> = None;

        for entry in snapshot {
            let outcome = entry.hook.post(ctx, result).await;
            if let ToolHookOutcome::Abort(reason) = outcome {
                debug!(hook = %entry.id, %reason, "tool-hook post requested abort (observation-only)");
                if saw_abort.is_none() {
                    saw_abort = Some(reason);
                }
            }
        }

        match saw_abort {
            Some(reason) => ToolHookOutcome::Abort(reason),
            None => ToolHookOutcome::Continue,
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::tool::{ToolContext, ToolInput};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_input(name: &str) -> ToolInput {
        ToolInput {
            name: name.to_string(),
            arguments: serde_json::json!({}),
            call_id: "call-1".to_string(),
        }
    }

    fn make_ctx() -> ToolContext {
        ToolContext::default()
    }

    // ── Counting hook ─────────────────────────────────────────────────────

    struct CountingHook {
        id: &'static str,
        pre_count: Arc<AtomicUsize>,
        post_count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ToolHook for CountingHook {
        fn id(&self) -> &str {
            self.id
        }
        async fn pre(&self, _ctx: &ToolCallCtx<'_>) -> ToolHookOutcome {
            self.pre_count.fetch_add(1, Ordering::SeqCst);
            ToolHookOutcome::Continue
        }
        async fn post(
            &self,
            _ctx: &ToolCallCtx<'_>,
            _result: &ToolOutput,
        ) -> ToolHookOutcome {
            self.post_count.fetch_add(1, Ordering::SeqCst);
            ToolHookOutcome::Continue
        }
    }

    // ── Abort hook ────────────────────────────────────────────────────────

    struct AbortOnName {
        id: &'static str,
        target: &'static str,
    }

    #[async_trait]
    impl ToolHook for AbortOnName {
        fn id(&self) -> &str {
            self.id
        }
        async fn pre(&self, ctx: &ToolCallCtx<'_>) -> ToolHookOutcome {
            if ctx.input.name == self.target {
                ToolHookOutcome::Abort(format!("{} is blocked", self.target))
            } else {
                ToolHookOutcome::Continue
            }
        }
    }

    // ── Post-abort (observation-only) ─────────────────────────────────────

    struct PostAbort;

    #[async_trait]
    impl ToolHook for PostAbort {
        fn id(&self) -> &str {
            "post-abort"
        }
        async fn pre(&self, _ctx: &ToolCallCtx<'_>) -> ToolHookOutcome {
            ToolHookOutcome::Continue
        }
        async fn post(
            &self,
            _ctx: &ToolCallCtx<'_>,
            _result: &ToolOutput,
        ) -> ToolHookOutcome {
            ToolHookOutcome::Abort("post said no".to_string())
        }
    }

    #[tokio::test]
    async fn register_and_count() {
        let registry = ToolHookRegistry::new();
        assert!(registry.is_empty().await);

        let pre = Arc::new(AtomicUsize::new(0));
        let post = Arc::new(AtomicUsize::new(0));
        registry
            .register(Arc::new(CountingHook {
                id: "c1",
                pre_count: pre.clone(),
                post_count: post.clone(),
            }))
            .await;
        assert_eq!(registry.len().await, 1);

        let input = make_input("foo");
        let tc = make_ctx();
        let ctx = ToolCallCtx::new(&input, &tc);

        for _ in 0..3 {
            let outcome = registry.pre_all(&ctx).await;
            assert!(outcome.is_continue());
        }
        assert_eq!(pre.load(Ordering::SeqCst), 3);

        let result = ToolOutput::success("ok");
        for _ in 0..3 {
            let outcome = registry.post_all(&ctx, &result).await;
            assert!(outcome.is_continue());
        }
        assert_eq!(post.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn unregister_removes_hook() {
        let registry = ToolHookRegistry::new();
        let pre = Arc::new(AtomicUsize::new(0));
        let post = Arc::new(AtomicUsize::new(0));
        registry
            .register(Arc::new(CountingHook {
                id: "c1",
                pre_count: pre.clone(),
                post_count: post.clone(),
            }))
            .await;
        assert!(registry.unregister("c1").await);
        assert!(registry.is_empty().await);
        // Unregistering again returns false.
        assert!(!registry.unregister("c1").await);
    }

    #[tokio::test]
    async fn pre_continue_allows_through() {
        let registry = ToolHookRegistry::new();
        registry
            .register(Arc::new(AbortOnName {
                id: "abort-only-bad",
                target: "bad-tool",
            }))
            .await;

        let input = make_input("ok-tool");
        let tc = make_ctx();
        let ctx = ToolCallCtx::new(&input, &tc);
        let outcome = registry.pre_all(&ctx).await;
        assert!(outcome.is_continue());
    }

    #[tokio::test]
    async fn pre_abort_short_circuits() {
        let registry = ToolHookRegistry::new();
        let pre = Arc::new(AtomicUsize::new(0));
        let post = Arc::new(AtomicUsize::new(0));
        // First, the abort hook (fires).
        registry
            .register(Arc::new(AbortOnName {
                id: "abort",
                target: "blocked",
            }))
            .await;
        // Then, a counting hook that should be skipped on abort.
        registry
            .register(Arc::new(CountingHook {
                id: "c1",
                pre_count: pre.clone(),
                post_count: post.clone(),
            }))
            .await;

        let input = make_input("blocked");
        let tc = make_ctx();
        let ctx = ToolCallCtx::new(&input, &tc);
        let outcome = registry.pre_all(&ctx).await;
        match outcome {
            ToolHookOutcome::Abort(reason) => assert!(reason.contains("blocked")),
            other => panic!("expected Abort, got {other:?}"),
        }
        // Counting hook must not have been invoked.
        assert_eq!(pre.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn post_runs_after_success() {
        let registry = ToolHookRegistry::new();
        let pre = Arc::new(AtomicUsize::new(0));
        let post = Arc::new(AtomicUsize::new(0));
        registry
            .register(Arc::new(CountingHook {
                id: "c1",
                pre_count: pre.clone(),
                post_count: post.clone(),
            }))
            .await;

        let input = make_input("foo");
        let tc = make_ctx();
        let ctx = ToolCallCtx::new(&input, &tc);
        let result = ToolOutput::success("all good");
        registry.post_all(&ctx, &result).await;
        assert_eq!(post.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn post_runs_after_error_as_observation() {
        // Post hooks see errored results too — the dispatcher passes the
        // error string through as a ToolOutput{is_error:true}.
        let registry = ToolHookRegistry::new();
        let pre = Arc::new(AtomicUsize::new(0));
        let post = Arc::new(AtomicUsize::new(0));
        registry
            .register(Arc::new(CountingHook {
                id: "c1",
                pre_count: pre.clone(),
                post_count: post.clone(),
            }))
            .await;

        let input = make_input("foo");
        let tc = make_ctx();
        let ctx = ToolCallCtx::new(&input, &tc);
        let errored = ToolOutput::error("boom");
        let outcome = registry.post_all(&ctx, &errored).await;
        assert!(outcome.is_continue());
        assert_eq!(post.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn post_abort_is_observation_only() {
        // Registering a post-aborting hook must not change the result the
        // caller receives — the outcome surfaces the abort reason but the
        // dispatcher still returns the original result (checked in the
        // dispatcher integration test).
        let registry = ToolHookRegistry::new();
        registry.register(Arc::new(PostAbort)).await;

        let input = make_input("foo");
        let tc = make_ctx();
        let ctx = ToolCallCtx::new(&input, &tc);
        let result = ToolOutput::success("kept");
        let outcome = registry.post_all(&ctx, &result).await;
        match outcome {
            ToolHookOutcome::Abort(reason) => assert_eq!(reason, "post said no"),
            other => panic!("expected Abort, got {other:?}"),
        }
    }
}
