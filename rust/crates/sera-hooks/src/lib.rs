//! SERA Hook Runtime — executes chainable hook pipelines.
//!
//! Hook types (HookChain, HookInstance, HookResult, HookContext, etc.) live in
//! `sera-domain::hook` so any crate can reference them without pulling in the
//! runtime. This crate provides:
//!
//! - `HookExecutor` trait — the interface for executing individual hooks
//! - `ChainExecutor` — runs a HookChain by invoking each enabled hook in order
//! - `InProcessHookRegistry` — registers and manages in-process hook functions
//!
//! WASM-based execution (wasmtime) will be added when the wasmtime dependency
//! is wired in. For now, in-process hooks provide the full chain execution
//! semantics that the spec requires.
//!
//! See SPEC-hooks for the full design.

pub mod chain;
pub mod error;
pub mod registry;

pub use chain::ChainExecutor;
pub use error::HookError;
pub use registry::InProcessHookRegistry;
