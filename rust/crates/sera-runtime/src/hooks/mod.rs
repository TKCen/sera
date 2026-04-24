//! In-process hook implementations for the sera-runtime + sera-gateway
//! hook chain.
//!
//! Each module here defines one or more [`sera_hooks::Hook`] impls that
//! can be registered into a [`sera_hooks::HookRegistry`] at gateway
//! startup and dispatched by a [`sera_hooks::ChainExecutor`] when a
//! matching [`sera_types::hook::HookChain`] fires.

pub mod constitutional;
