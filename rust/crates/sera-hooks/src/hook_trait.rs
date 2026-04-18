use async_trait::async_trait;
use sera_types::hook::{HookContext, HookMetadata, HookResult};

use crate::error::HookError;

/// Trait that every in-process hook must implement.
///
/// Hooks are registered in a [`crate::registry::HookRegistry`] and executed by a
/// [`crate::executor::ChainExecutor`]. WASM hooks are a future concern — this
/// trait covers native Rust hooks only.
#[async_trait]
pub trait Hook: Send + Sync {
    /// Static metadata describing this hook module.
    fn metadata(&self) -> HookMetadata;

    /// Called once when the hook is first registered or reconfigured.
    ///
    /// `config` is the per-instance JSON config block from the chain manifest.
    async fn init(&mut self, config: serde_json::Value) -> Result<(), HookError>;

    /// Execute the hook against the given context.
    ///
    /// Returns a [`HookResult`] indicating whether the chain should continue,
    /// short-circuit with rejection, or redirect to another target.
    async fn execute(&self, ctx: &HookContext) -> Result<HookResult, HookError>;
}
