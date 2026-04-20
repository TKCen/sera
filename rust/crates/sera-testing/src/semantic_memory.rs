//! Thin re-export stub — implementation lives in `sera-memory::InMemorySemanticStore`.
//!
//! Downstream code that previously imported from `sera-testing::semantic_memory`
//! continues to work; new callers should import from `sera_memory` directly.
pub use sera_memory::InMemorySemanticStore;
