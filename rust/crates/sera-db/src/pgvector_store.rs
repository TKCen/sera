//! Thin re-export stub — implementation lives in `sera-memory::PgVectorStore`.
//!
//! Downstream code that previously imported from `sera-db::pgvector_store`
//! continues to work; new callers should import from `sera_memory` directly.
pub use sera_memory::{PgVectorStore, DEFAULT_SEMANTIC_DIMENSIONS};
