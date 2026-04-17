//! Async cancellation for in-flight hook chains.
//!
//! # Motivation
//!
//! Long-running hooks (LLM checks, external policy RPCs, WASM fuel-bounded
//! loops) need a cooperative cancellation path so that the outer caller can
//! tell the executor "stop now" without waiting for the per-chain timeout to
//! elapse. For example: a request is cancelled on the gateway side, or a
//! circuit-breaker trips between hook invocations.
//!
//! [`HookCancellation`] is a cloneable handle backed by
//! [`tokio_util::sync::CancellationToken`]. The executor checks it **between**
//! hook invocations and also awaits it concurrently with each hook's own
//! execution future — cancellation therefore aborts as quickly as the
//! currently-executing hook allows.
//!
//! # Distinction from `HookAbortSignal`
//!
//! - [`HookAbortSignal`](crate::error::HookAbortSignal) is an *intra-hook*
//!   value: a hook returns `HookError::Aborted` to tell the runtime the entire
//!   pipeline (not just the chain) must abort.
//! - [`HookCancellation`] is *extra-chain*: the caller signals the executor
//!   before or during chain execution that it should stop.
//!
//! Both are supported; they do not interact.

use tokio_util::sync::CancellationToken;

/// An async cancellation signal for a hook chain.
///
/// Produced by [`HookCancellation::new`] and passed to
/// [`ChainExecutor::execute_chain_cancellable`](crate::ChainExecutor::execute_chain_cancellable).
/// Clones share the same underlying token — cancelling any clone cancels them
/// all.
///
/// ```ignore
/// let cancel = sera_hooks::HookCancellation::new();
/// let handle = cancel.clone();
/// tokio::spawn(async move {
///     tokio::time::sleep(std::time::Duration::from_millis(100)).await;
///     handle.cancel();
/// });
/// let result = executor.execute_chain_cancellable(&chain, ctx, cancel).await?;
/// ```
#[derive(Debug, Clone, Default)]
pub struct HookCancellation {
    token: CancellationToken,
}

impl HookCancellation {
    /// Create a new, un-cancelled signal.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return a signal that is already cancelled. Useful in tests.
    pub fn already_cancelled() -> Self {
        let token = CancellationToken::new();
        token.cancel();
        Self { token }
    }

    /// Trigger cancellation. Idempotent.
    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// Whether cancellation has been triggered.
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Return a future that completes when cancellation fires.
    ///
    /// Cheap to call repeatedly; each call returns an independent future.
    pub async fn cancelled(&self) {
        self.token.cancelled().await;
    }

    /// Borrow the underlying token for interop with other
    /// `tokio_util`-aware APIs.
    pub fn token(&self) -> &CancellationToken {
        &self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_starts_uncancelled() {
        let c = HookCancellation::new();
        assert!(!c.is_cancelled());
    }

    #[test]
    fn cancel_flips_flag() {
        let c = HookCancellation::new();
        c.cancel();
        assert!(c.is_cancelled());
    }

    #[test]
    fn already_cancelled_helper() {
        let c = HookCancellation::already_cancelled();
        assert!(c.is_cancelled());
    }

    #[test]
    fn clones_share_state() {
        let a = HookCancellation::new();
        let b = a.clone();
        b.cancel();
        assert!(a.is_cancelled());
    }

    #[tokio::test]
    async fn cancelled_future_fires() {
        let c = HookCancellation::new();
        let c2 = c.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            c2.cancel();
        });
        // Should complete promptly, not hang.
        tokio::time::timeout(std::time::Duration::from_millis(200), c.cancelled())
            .await
            .expect("cancelled future should fire");
        assert!(c.is_cancelled());
    }
}
