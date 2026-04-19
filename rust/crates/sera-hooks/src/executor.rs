use std::sync::Arc;
use std::time::Instant;

use sera_types::hook::{
    CHAIN_ABORTED_CODE, ChainResult, HookChain, HookContext, HookPoint, HookResult,
};
use tokio::time::{Duration, timeout};
use tracing::{debug, warn};

use crate::cancel::HookCancellation;
use crate::error::HookError;
use crate::registry::HookRegistry;

/// Executes hook chains stored in a shared [`HookRegistry`].
pub struct ChainExecutor {
    registry: Arc<HookRegistry>,
}

impl ChainExecutor {
    /// Create an executor backed by the given registry.
    pub fn new(registry: Arc<HookRegistry>) -> Self {
        Self { registry }
    }

    /// Execute a single [`HookChain`] against the provided context.
    ///
    /// Backward-compatible entry point with no async cancellation signal —
    /// see [`ChainExecutor::execute_chain_cancellable`] to pass a
    /// [`HookCancellation`].
    ///
    /// - Disabled hooks are skipped entirely.
    /// - A `Continue` result merges `context_updates` into `ctx.metadata`,
    ///   applies any `permission_overrides`, and threads `updated_input`
    ///   through to the next hook in the chain via `ctx.updated_input`.
    /// - `Reject` / `Redirect` short-circuit the chain immediately.
    /// - Hook errors are handled according to `chain.fail_open`:
    ///   - `fail_open = true`  → log warning, skip hook, continue.
    ///   - `fail_open = false` → propagate the error.
    /// - If the total elapsed time exceeds `chain.timeout_ms` a
    ///   [`HookError::ChainTimeout`] is returned.
    pub async fn execute_chain(
        &self,
        chain: &HookChain,
        ctx: HookContext,
    ) -> Result<ChainResult, HookError> {
        self.execute_chain_cancellable(chain, ctx, HookCancellation::new())
            .await
    }

    /// Execute a chain with an external cancellation signal.
    ///
    /// If `cancel` fires before the first hook runs, the chain exits cleanly
    /// with a `Reject { code: "chain_aborted" }` outcome and `hooks_executed
    /// = 0`. If it fires mid-chain, the current hook's future is dropped
    /// (via `tokio::select!`) and the same aborted outcome is returned with
    /// `hooks_executed` reflecting hooks that completed before cancellation.
    pub async fn execute_chain_cancellable(
        &self,
        chain: &HookChain,
        mut ctx: HookContext,
        cancel: HookCancellation,
    ) -> Result<ChainResult, HookError> {
        let chain_start = Instant::now();
        let deadline = Duration::from_millis(chain.timeout_ms);
        let mut hooks_executed: usize = 0;
        let mut updated_input: Option<serde_json::Value> = None;

        // Pre-flight: already-cancelled signals exit with zero hooks run.
        if cancel.is_cancelled() {
            return Ok(aborted_result(ctx, 0, chain_start.elapsed().as_millis() as u64, None));
        }

        for instance in &chain.hooks {
            // Respect the enabled flag.
            if !instance.enabled {
                debug!(hook = %instance.hook_ref, chain = %chain.name, "skipping disabled hook");
                continue;
            }

            // Check cancellation between hooks — cheap and does not require
            // racing a future.
            if cancel.is_cancelled() {
                return Ok(aborted_result(
                    ctx,
                    hooks_executed,
                    chain_start.elapsed().as_millis() as u64,
                    updated_input,
                ));
            }

            // Check deadline before executing each hook.
            let elapsed = chain_start.elapsed();
            if elapsed >= deadline {
                return Err(HookError::ChainTimeout {
                    chain: chain.name.clone(),
                    elapsed_ms: elapsed.as_millis() as u64,
                });
            }

            let remaining = deadline - elapsed;

            // Resolve the hook from the registry.
            let hook = match self.registry.get(&instance.hook_ref) {
                Some(h) => h,
                None => {
                    let err = HookError::HookNotFound {
                        name: instance.hook_ref.clone(),
                    };
                    if chain.fail_open {
                        warn!(hook = %instance.hook_ref, "hook not found, fail_open — skipping");
                        continue;
                    } else {
                        return Err(err);
                    }
                }
            };

            // Race the hook against the remaining timeout budget AND the
            // external cancellation signal. Whichever completes first wins.
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    // External cancellation fired mid-hook — return the
                    // aborted outcome. `hooks_executed` does not count the
                    // in-flight hook (it never completed).
                    return Ok(aborted_result(
                        ctx,
                        hooks_executed,
                        chain_start.elapsed().as_millis() as u64,
                        updated_input,
                    ));
                }
                r = timeout(remaining, hook.execute(&ctx)) => r,
            };

            hooks_executed += 1;

            let hook_result = match result {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    // A pipeline-abort signal always propagates — fail_open
                    // must not swallow it, by contract.
                    if matches!(e, HookError::Aborted { .. }) {
                        return Err(e);
                    }
                    if chain.fail_open {
                        warn!(
                            hook = %instance.hook_ref,
                            error = %e,
                            "hook execution failed, fail_open — skipping"
                        );
                        continue;
                    } else {
                        return Err(e);
                    }
                }
                Err(_elapsed) => {
                    let err = HookError::ChainTimeout {
                        chain: chain.name.clone(),
                        elapsed_ms: chain_start.elapsed().as_millis() as u64,
                    };
                    if chain.fail_open {
                        warn!(hook = %instance.hook_ref, "hook timed out, fail_open — skipping");
                        continue;
                    } else {
                        return Err(err);
                    }
                }
            };

            match hook_result {
                HookResult::Continue {
                    context_updates,
                    updated_input: hook_input,
                    permission_overrides,
                } => {
                    ctx.apply_updates(context_updates);
                    if let Some(overrides) = permission_overrides.as_ref() {
                        ctx.apply_permission_overrides(overrides);
                    }
                    // Last hook in the chain that sets updated_input wins at
                    // the ChainResult level, but every set value is propagated
                    // into the next hook's context within the chain via the
                    // reserved metadata slot.
                    if let Some(new_input) = hook_input {
                        updated_input = Some(new_input.clone());
                        ctx.set_updated_input(new_input);
                    }
                }
                terminal => {
                    // Short-circuit.
                    let duration_ms = chain_start.elapsed().as_millis() as u64;
                    return Ok(ChainResult {
                        context: ctx,
                        outcome: terminal,
                        hooks_executed,
                        duration_ms,
                        updated_input,
                    });
                }
            }
        }

        let duration_ms = chain_start.elapsed().as_millis() as u64;

        // Check one final time whether we blew the budget on the last hook.
        if chain_start.elapsed() > deadline {
            return Err(HookError::ChainTimeout {
                chain: chain.name.clone(),
                elapsed_ms: duration_ms,
            });
        }

        Ok(ChainResult {
            context: ctx,
            outcome: HookResult::pass(),
            hooks_executed,
            duration_ms,
            updated_input,
        })
    }

    /// Execute all chains registered for `point` against `ctx`, sequentially.
    ///
    /// Chains are executed in the order they appear in `chains`. If any chain
    /// short-circuits (Reject/Redirect) execution stops immediately and that
    /// result is returned. The final `ctx` after all chains is included in the
    /// returned [`ChainResult`].
    pub async fn execute_at_point(
        &self,
        point: HookPoint,
        chains: &[HookChain],
        ctx: HookContext,
    ) -> Result<ChainResult, HookError> {
        self.execute_at_point_cancellable(point, chains, ctx, HookCancellation::new())
            .await
    }

    /// Cancellation-aware variant of [`ChainExecutor::execute_at_point`].
    pub async fn execute_at_point_cancellable(
        &self,
        point: HookPoint,
        chains: &[HookChain],
        ctx: HookContext,
        cancel: HookCancellation,
    ) -> Result<ChainResult, HookError> {
        let matching: Vec<&HookChain> = chains.iter().filter(|c| c.point == point).collect();

        if matching.is_empty() {
            // Nothing to do — return a pass-through result.
            return Ok(ChainResult {
                context: ctx,
                outcome: HookResult::pass(),
                hooks_executed: 0,
                duration_ms: 0,
                updated_input: None,
            });
        }

        let mut current_ctx = ctx;
        let mut total_executed = 0usize;
        let mut updated_input: Option<serde_json::Value> = None;
        let start = Instant::now();

        for chain in matching {
            if cancel.is_cancelled() {
                return Ok(aborted_result(
                    current_ctx,
                    total_executed,
                    start.elapsed().as_millis() as u64,
                    updated_input,
                ));
            }

            let result = self
                .execute_chain_cancellable(chain, current_ctx, cancel.clone())
                .await?;
            total_executed += result.hooks_executed;

            // Propagate updated_input: last chain that sets it wins.
            if result.updated_input.is_some() {
                updated_input = result.updated_input.clone();
            }

            if result.is_aborted() {
                return Ok(ChainResult {
                    context: result.context,
                    outcome: result.outcome,
                    hooks_executed: total_executed,
                    duration_ms: start.elapsed().as_millis() as u64,
                    updated_input,
                });
            }

            if result.outcome.is_terminal() {
                return Ok(ChainResult {
                    context: result.context,
                    outcome: result.outcome,
                    hooks_executed: total_executed,
                    duration_ms: start.elapsed().as_millis() as u64,
                    updated_input,
                });
            }

            // Continue — carry updated context into next chain.
            current_ctx = result.context;
        }

        Ok(ChainResult {
            context: current_ctx,
            outcome: HookResult::pass(),
            hooks_executed: total_executed,
            duration_ms: start.elapsed().as_millis() as u64,
            updated_input,
        })
    }
}

/// Build the canonical "chain aborted by external cancellation" result.
fn aborted_result(
    ctx: HookContext,
    hooks_executed: usize,
    duration_ms: u64,
    updated_input: Option<serde_json::Value>,
) -> ChainResult {
    ChainResult {
        context: ctx,
        outcome: HookResult::reject_with_code("chain aborted by cancellation", CHAIN_ABORTED_CODE),
        hooks_executed,
        duration_ms,
        updated_input,
    }
}
