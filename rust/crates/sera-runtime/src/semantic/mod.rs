//! Semantic-memory scaffolding for Tier-2.
//!
//! This module hosts the concrete [`sera_types::EmbeddingService`]
//! implementations. The trait itself lives in `sera-types` to keep the
//! leaf crate free of HTTP-client dependencies; providers live here so
//! they can share `reqwest` with the rest of the runtime.
//!
//! The trait is wired into the turn loop in a follow-up bead (sera-0yqq);
//! for now this module only makes the providers *available* to callers
//! that construct them directly.

pub mod providers;
