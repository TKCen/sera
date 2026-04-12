use sera_types::hook::HookPoint;
use thiserror::Error;

/// Errors produced by the hook registry and chain executor.
#[derive(Debug, Error)]
pub enum HookError {
    /// The named hook is not registered.
    #[error("hook not found: {name}")]
    HookNotFound { name: String },

    /// The hook's `init()` call failed.
    #[error("hook '{hook}' init failed: {reason}")]
    InitFailed { hook: String, reason: String },

    /// The hook's `execute()` call returned an error.
    #[error("hook '{hook}' execution failed: {reason}")]
    ExecutionFailed { hook: String, reason: String },

    /// The entire chain exceeded its timeout budget.
    #[error("chain '{chain}' timed out after {elapsed_ms}ms")]
    ChainTimeout { chain: String, elapsed_ms: u64 },

    /// A single hook exceeded its execution timeout.
    #[error("hook '{hook}' timed out after {elapsed_ms}ms")]
    HookTimeout { hook: String, elapsed_ms: u64 },

    /// The hook was wired to a hook point it does not support.
    #[error("hook '{hook}' does not support point {point:?}; supported: {supported:?}")]
    InvalidHookPoint {
        hook: String,
        point: HookPoint,
        supported: Vec<HookPoint>,
    },
}
