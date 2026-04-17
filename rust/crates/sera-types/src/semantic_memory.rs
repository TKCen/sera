//! Tier-2 semantic-memory storage abstractions.
//!
//! Per SPEC-memory §13, sera-runtime wires the concrete backends
//! (`PgVectorStore` in sera-db, `InMemorySemanticStore` in sera-testing)
//! while the trait lives here so `sera-types` stays the dependency-free
//! leaf crate.
//!
//! ## Contract
//!
//! - `put(SemanticEntry)` persists an embedding + content row and returns
//!   the assigned [`MemoryId`]. Callers may pre-populate `entry.id` or let
//!   the backend generate one.
//! - `query(SemanticQuery)` performs a scoped similarity search. All queries
//!   MUST filter on `agent_id` first (multi-tenant isolation per
//!   SPEC-memory §13.1).
//! - `delete` removes a row by id; returns [`SemanticError::NotFound`] if
//!   the id is unknown.
//! - `evict` prunes rows according to the supplied [`EvictionPolicy`] and
//!   returns the number of rows removed.
//! - `stats` returns a cheap aggregate snapshot for operator dashboards.
//!
//! ## Error Policy
//!
//! Backends MUST fail loudly. Any backend-level issue (Postgres down,
//! pgvector missing, deserialization error) surfaces as a
//! [`SemanticError`]. There are no silent fallbacks or default vectors.
//! See the sister module `sera-types::embedding` for the same policy on
//! the embedding provider side.
//!
//! ## Hybrid scoring
//!
//! [`ScoredEntry`] carries per-signal sub-scores (`index_score`,
//! `vector_score`, `recency_score`). The Tier-2 HybridScorer (landing in a
//! follow-up bead) reranks results without a second round trip by
//! combining these sub-scores with its own policy.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::memory::SegmentKind;

/// Stable identifier for a semantic-memory row.
///
/// Backends are free to choose the id space (UUIDv4 in `PgVectorStore`;
/// hash-derived in `InMemorySemanticStore`); the wrapper keeps the public
/// surface opaque so callers don't accidentally couple to a specific
/// backing representation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryId(pub String);

impl MemoryId {
    /// Wrap any string-like into a [`MemoryId`].
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the inner id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MemoryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for MemoryId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for MemoryId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// A single semantic-memory row — content + embedding + metadata.
///
/// Rows are scoped to a single `agent_id`. Queries MUST filter on this
/// field before any vector-similarity work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEntry {
    /// Stable identifier. Callers MAY set this pre-put; otherwise the
    /// backend generates a fresh id.
    pub id: MemoryId,
    /// Owning agent instance. Used for multi-tenant isolation.
    pub agent_id: String,
    /// Original text that was embedded — kept so callers can render the
    /// recall without re-inflating from a separate store.
    pub content: String,
    /// Dense embedding. Length must match the backend's configured
    /// dimensionality; mismatches surface as
    /// [`SemanticError::DimensionMismatch`].
    pub embedding: Vec<f32>,
    /// Which tier-segment produced this row.
    pub tier: SegmentKind,
    /// Opaque tags for operator filtering / recall-shaping heuristics.
    #[serde(default)]
    pub tags: Vec<String>,
    /// When the row was created.
    pub created_at: DateTime<Utc>,
    /// Last time the row was returned by a query. Updated opportunistically
    /// — backends may or may not persist this on every recall.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_accessed_at: Option<DateTime<Utc>>,
    /// If true the row is exempt from row-cap / TTL eviction and survives
    /// policies with `promoted_exempt = true`.
    #[serde(default)]
    pub promoted: bool,
}

/// Parameters for [`SemanticMemoryStore::query`].
///
/// Must always carry `agent_id`; the optional fields progressively narrow
/// the result set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticQuery {
    /// Mandatory tenant scope.
    pub agent_id: String,
    /// Only match rows whose `tier` equals this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier_filter: Option<SegmentKind>,
    /// Original query text. Optional — some callers provide only the
    /// pre-computed embedding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Pre-computed query embedding. When `None`, the backend MAY embed
    /// `text` using a bound [`crate::EmbeddingService`]; naked backends
    /// return [`SemanticError::Backend`] if neither field is supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_embedding: Option<Vec<f32>>,
    /// Maximum number of rows to return (after threshold filtering).
    pub top_k: usize,
    /// Cosine-similarity floor. Rows whose composite `score` is below this
    /// value are dropped before `top_k` truncation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similarity_threshold: Option<f32>,
}

/// One row returned from [`SemanticMemoryStore::query`], decorated with
/// the per-signal sub-scores so downstream scorers can rerank without a
/// second round trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredEntry {
    /// The matched row.
    pub entry: SemanticEntry,
    /// Composite score used for ordering at the backend. Higher is better.
    pub score: f32,
    /// Lexical / inverted-index component (may be `0.0` in vector-only
    /// backends).
    pub index_score: f32,
    /// Vector-similarity component (cosine or inner-product).
    pub vector_score: f32,
    /// Recency component, normalised to `[0.0, 1.0]`.
    pub recency_score: f32,
}

/// Policy governing [`SemanticMemoryStore::evict`].
///
/// All fields compose. `None` means "no cap on this dimension".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvictionPolicy {
    /// If set, each agent's row count is capped at this value (oldest
    /// first).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_agent: Option<usize>,
    /// If set, rows older than this many days are removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_days: Option<u32>,
    /// When `true`, rows with `promoted = true` are skipped by both
    /// `max_per_agent` and `ttl_days` passes.
    #[serde(default)]
    pub promoted_exempt: bool,
}

/// Aggregate snapshot for operator dashboards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticStats {
    /// Total rows across all agents.
    pub total_rows: usize,
    /// Up to N `(agent_id, row_count)` pairs, ordered by count desc.
    pub per_agent_top: Vec<(String, usize)>,
    /// Oldest `created_at` in the store (epoch if empty).
    pub oldest: DateTime<Utc>,
    /// Newest `created_at` in the store (epoch if empty).
    pub newest: DateTime<Utc>,
}

/// Errors raised by a [`SemanticMemoryStore`] implementation.
#[derive(Debug, Error)]
pub enum SemanticError {
    /// No row with the supplied id exists.
    #[error("semantic memory id not found: {0}")]
    NotFound(MemoryId),

    /// A supplied embedding has the wrong number of dimensions for the
    /// configured backend.
    #[error("semantic memory dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: usize, got: usize },

    /// Generic backend failure — unavailable service, unexpected SQL
    /// error, pgvector extension missing, etc.
    #[error("semantic memory backend error: {0}")]
    Backend(String),

    /// Content or tag serialization failed.
    #[error("semantic memory serialization error: {0}")]
    Serialization(String),
}

/// Tier-2 semantic-memory backend.
///
/// Implementations live in:
///
/// * `sera-db::pgvector_store::PgVectorStore` — production, Postgres +
///   pgvector.
/// * `sera-testing::semantic_memory::InMemorySemanticStore` — tests and
///   dev environments without pgvector.
///
/// Callers should hold a `Box<dyn SemanticMemoryStore>` or
/// `Arc<dyn SemanticMemoryStore>` and depend only on this trait.
#[async_trait]
pub trait SemanticMemoryStore: Send + Sync + 'static {
    /// Persist `entry` and return its canonical [`MemoryId`]. If
    /// `entry.id` is already populated, backends SHOULD use that value
    /// (useful for idempotent writes from replays).
    async fn put(&self, entry: SemanticEntry) -> Result<MemoryId, SemanticError>;

    /// Scoped similarity search. Results are ordered by descending
    /// [`ScoredEntry::score`] with ties broken by `created_at` desc.
    async fn query(&self, query: SemanticQuery) -> Result<Vec<ScoredEntry>, SemanticError>;

    /// Remove the row identified by `id`. Returns
    /// [`SemanticError::NotFound`] if the id is not in the store.
    async fn delete(&self, id: &MemoryId) -> Result<(), SemanticError>;

    /// Apply `policy` to the store and return the number of rows removed.
    async fn evict(&self, policy: &EvictionPolicy) -> Result<usize, SemanticError>;

    /// Return a fresh aggregate snapshot.
    async fn stats(&self) -> Result<SemanticStats, SemanticError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_id_roundtrips_string() {
        let id = MemoryId::new("abc-123");
        assert_eq!(id.as_str(), "abc-123");
        assert_eq!(id.to_string(), "abc-123");
    }

    #[test]
    fn memory_id_from_variants() {
        let a: MemoryId = "xyz".into();
        let b: MemoryId = String::from("xyz").into();
        assert_eq!(a, b);
    }

    #[test]
    fn semantic_error_dimension_mismatch_display() {
        let e = SemanticError::DimensionMismatch {
            expected: 1536,
            got: 768,
        };
        let s = e.to_string();
        assert!(s.contains("1536"));
        assert!(s.contains("768"));
    }

    #[test]
    fn semantic_error_not_found_display() {
        let e = SemanticError::NotFound(MemoryId::new("row-1"));
        assert!(e.to_string().contains("row-1"));
    }

    #[test]
    fn eviction_policy_default_is_noop() {
        let p = EvictionPolicy::default();
        assert!(p.max_per_agent.is_none());
        assert!(p.ttl_days.is_none());
        assert!(!p.promoted_exempt);
    }

    #[test]
    fn semantic_entry_json_roundtrip_preserves_fields() {
        let e = SemanticEntry {
            id: MemoryId::new("r-1"),
            agent_id: "agent-a".into(),
            content: "hello".into(),
            embedding: vec![0.1, 0.2, 0.3],
            tier: SegmentKind::MemoryRecall("recall-7".into()),
            tags: vec!["pinned".into()],
            created_at: Utc::now(),
            last_accessed_at: None,
            promoted: true,
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: SemanticEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, e.id);
        assert_eq!(back.agent_id, e.agent_id);
        assert_eq!(back.embedding, e.embedding);
        assert!(back.promoted);
    }
}
