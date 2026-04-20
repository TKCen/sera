//! `sera-memory` — Tier-2 semantic-memory trait + built-in backends.
//!
//! ## Public surface
//!
//! * [`SemanticMemoryStore`] trait — the canonical Tier-2 interface.
//! * Types: [`PutRequest`], [`SemanticEntry`], [`SemanticQuery`], [`ScoredEntry`],
//!   [`EvictionPolicy`], [`SemanticStats`], [`SemanticError`], [`Scope`],
//!   [`Damping`], [`ScopeHierarchy`], [`MemoryHit`], [`MemoryId`].
//! * [`PgVectorStore`] — Postgres + pgvector backend (feature `pgvector`).
//! * [`SqliteMemoryStore`] — SQLite + FTS5 + optional sqlite-vec (feature `sqlite`).
//! * [`InMemorySemanticStore`] — in-process fake for tests (feature `testing`).
//!
//! `sera-types` exports `EmbeddingService` and `memory::SegmentKind`; this
//! crate depends on those two items but is NOT re-exported from `sera-types`
//! to avoid a dependency cycle.

pub mod store;

#[cfg(feature = "pgvector")]
pub mod pgvector_store;

#[cfg(feature = "sqlite")]
pub mod sqlite_store;

#[cfg(feature = "testing")]
pub mod in_memory;

// ── Flat re-exports ───────────────────────────────────────────────────────────

pub use store::{
    Damping, EvictionPolicy, MemoryHit, MemoryId, PutRequest, Scope, ScopeHierarchy, ScoredEntry,
    SemanticEntry, SemanticError, SemanticMemoryStore, SemanticQuery, SemanticStats,
};

#[cfg(feature = "pgvector")]
pub use pgvector_store::{PgVectorStore, DEFAULT_SEMANTIC_DIMENSIONS};

#[cfg(feature = "sqlite")]
pub use sqlite_store::{SqliteMemoryStore, DEFAULT_SQLITE_VEC_DIMENSIONS};

#[cfg(feature = "testing")]
pub use in_memory::InMemorySemanticStore;
