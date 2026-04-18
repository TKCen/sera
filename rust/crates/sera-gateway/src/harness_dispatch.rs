//! Harness dispatch — re-exports the shared AgentHarness trait and dispatch logic.
//!
//! The canonical definitions live in `sera_types::harness` so that both gateway
//! and standalone runtime can implement the trait without a circular dependency.
//! This module is a thin shim that preserves the existing public path
//! (`sera_gateway::harness_dispatch::*`) for callers inside the gateway crate.

pub use sera_types::harness::*;
