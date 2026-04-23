//! `sera-hooks` — in-process hook registry and chain executor for SERA.
//!
//! # Overview
//!
//! Provides the plumbing for SERA's hook system (SPEC-hooks):
//!
//! - [`hook_trait::Hook`] — implement this to create an in-process hook.
//! - [`registry::HookRegistry`] — register / look up hooks by name.
//! - [`executor::ChainExecutor`] — execute chains of hooks at a given point.
//! - [`error::HookError`] — all failure modes.
//!
//! WASM hook execution is supported via the `wasm` feature flag.
//! When enabled, [`component_adapter::ComponentAdapter`] provides WIT component-model runtime support.
//!
//! # Example
//!
//! ```rust,ignore
//! use sera_hooks::{ChainExecutor, HookRegistry};
//! use sera_types::hook::{HookChain, HookContext, HookPoint};
//!
//! let mut registry = HookRegistry::new();
//! registry.register(Box::new(MyHook::new()));
//!
//! let executor = ChainExecutor::new(Arc::new(registry));
//! let ctx = HookContext::new(HookPoint::PreRoute);
//! let result = executor.execute_at_point(HookPoint::PreRoute, &chains, ctx).await?;
//! ```

pub mod cancel;
pub mod error;
pub mod executor;
pub mod hook_trait;
pub mod manifest;
pub mod registry;
pub mod sera_errors;

// Component-model adapter (WIT-based, sandboxed capability injection).
#[cfg(feature = "wasm")]
pub mod component_adapter;

// Convenient re-exports.
pub use cancel::HookCancellation;
pub use error::{HookAbortSignal, HookError, ManifestError};
pub use executor::ChainExecutor;
pub use hook_trait::Hook;
pub use manifest::{
    HookChainManifest, HookChainManifestMetadata, HookChainManifestSpec, HookInstanceManifest,
};
pub use registry::{HookRegistry, HookTier};

#[cfg(feature = "wasm")]
pub use component_adapter::{
    ComponentAdapter, ComponentCapabilities, ComponentError, WasmHookMetadata,
};

#[cfg(test)]
mod tests;
