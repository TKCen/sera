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
//! When enabled, [`wasm_adapter::WasmHookAdapter`] provides WASM runtime support.
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

pub mod error;
pub mod executor;
pub mod hook_trait;
pub mod registry;

// WASM adapter is only compiled with the wasm feature
#[cfg(feature = "wasm")]
pub mod wasm_adapter;

// Convenient re-exports.
pub use error::HookError;
pub use executor::ChainExecutor;
pub use hook_trait::Hook;
pub use registry::HookRegistry;

#[cfg(test)]
mod tests;
