use std::sync::Arc;
use std::time::Instant;

use sera_domain::hook::{ChainResult, HookChain, HookContext, HookPoint, HookResult};
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

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
    /// - Disabled hooks are skipped entirely.
    /// - A `Continue` result merges `context_updates` into `ctx.metadata`.
    /// - `Reject` / `Redirect` short-circuit the chain immediately.
    /// - Hook errors are handled according to `chain.fail_open`:
    ///   - `fail_open = true`  → log warning, skip hook, continue.
    ///   - `fail_open = false` → propagate the error.
    /// - If the total elapsed time exceeds `chain.timeout_ms` a
    ///   [`HookError::ChainTimeout`] is returned.
    pub async fn execute_chain(
        &self,
        chain: &HookChain,
        mut ctx: HookContext,
    ) -> Result<ChainResult, HookError> {
        let chain_start = Instant::now();
        let deadline = Duration::from_millis(chain.timeout_ms);
        let mut hooks_executed: usize = 0;

        for instance in &chain.hooks {
            // Respect the enabled flag.
            if !instance.enabled {
                debug!(hook = %instance.hook_ref, chain = %chain.name, "skipping disabled hook");
                continue;
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

            // Execute with per-chain remaining-time budget.
            let result = timeout(remaining, hook.execute(&ctx)).await;

            hooks_executed += 1;

            let hook_result = match result {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
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
                HookResult::Continue { context_updates } => {
                    ctx.apply_updates(context_updates);
                }
                terminal => {
                    // Short-circuit.
                    let duration_ms = chain_start.elapsed().as_millis() as u64;
                    return Ok(ChainResult {
                        context: ctx,
                        outcome: terminal,
                        hooks_executed,
                        duration_ms,
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
        let matching: Vec<&HookChain> = chains.iter().filter(|c| c.point == point).collect();

        if matching.is_empty() {
            // Nothing to do — return a pass-through result.
            return Ok(ChainResult {
                context: ctx,
                outcome: HookResult::pass(),
                hooks_executed: 0,
                duration_ms: 0,
            });
        }

        let mut current_ctx = ctx;
        let mut total_executed = 0usize;
        let start = Instant::now();

        for chain in matching {
            let result = self.execute_chain(chain, current_ctx).await?;
            total_executed += result.hooks_executed;

            if result.outcome.is_terminal() {
                return Ok(ChainResult {
                    context: result.context,
                    outcome: result.outcome,
                    hooks_executed: total_executed,
                    duration_ms: start.elapsed().as_millis() as u64,
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
        })
    }
}
