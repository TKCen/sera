//! Thin re-export stub — implementation lives in `sera-memory::SqliteMemoryStore`.
//!
//! Downstream code that previously imported from `sera-db::sqlite_memory_store`
//! continues to work; new callers should import from `sera_memory` directly.
pub use sera_memory::{SqliteMemoryStore, DEFAULT_SQLITE_VEC_DIMENSIONS};
