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

    /// A hook raised a [`HookAbortSignal`] — the entire pipeline must abort,
    /// not just the current chain. Distinct from `HookResult::Reject`, which
    /// only short-circuits the current chain.
    #[error("hook '{hook}' aborted pipeline: {reason}")]
    Aborted {
        hook: String,
        reason: String,
        #[source]
        signal: HookAbortSignal,
    },
}

/// Signal raised from inside a hook to abort the entire hook pipeline.
///
/// A `Reject` `HookResult` short-circuits one chain but leaves subsequent
/// chains at other points free to run. `HookAbortSignal` propagates out as
/// [`HookError::Aborted`] and must be treated as a terminal pipeline failure
/// by the caller (e.g. the runtime should not keep dispatching downstream
/// hook points).
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[error("hook abort: {reason} (code: {code:?})")]
pub struct HookAbortSignal {
    /// Human-readable reason.
    pub reason: String,
    /// Optional machine-readable code (e.g. `"policy_violation"`).
    pub code: Option<String>,
}

impl HookAbortSignal {
    /// Create a new abort signal with a reason.
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            code: None,
        }
    }

    /// Create an abort signal with a machine-readable code.
    pub fn with_code(reason: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            code: Some(code.into()),
        }
    }
}
