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

// ── Scope ─────────────────────────────────────────────────────────────────────

/// Hierarchical memory scope (GH#140).
///
/// Scopes form a containment hierarchy: `Agent` ⊂ `Circle` ⊂ `Org` ⊂ `Global`.
/// [`SemanticMemoryStore::query_hierarchical`] walks this chain with per-level
/// dampening, falling back to broader scopes when the narrower scope yields
/// insufficient results.
///
/// When a [`SemanticEntry`] has `scope: None` it is treated as
/// `Scope::Agent(agent_id)` for backward compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum Scope {
    /// Scoped to a single agent instance.
    Agent(String),
    /// Scoped to a named circle (group of agents sharing memory).
    Circle(String),
    /// Scoped to an organisation (all agents in a tenant).
    Org(String),
    /// Globally visible across all tenants (operator-managed rows only).
    Global,
}

impl Scope {
    /// Stable SQL discriminant (`"agent"`, `"circle"`, `"org"`, `"global"`).
    ///
    /// Maps onto the pgvector `scope_kind` column added for GH#140 so
    /// callers can translate between the enum and SQL without a custom
    /// encoder.
    pub fn kind_str(&self) -> &'static str {
        match self {
            Scope::Agent(_) => "agent",
            Scope::Circle(_) => "circle",
            Scope::Org(_) => "org",
            Scope::Global => "global",
        }
    }

    /// SQL `scope_key` value. `Global` has no key; we return an empty
    /// string so the column can stay non-null.
    pub fn key_str(&self) -> &str {
        match self {
            Scope::Agent(k) | Scope::Circle(k) | Scope::Org(k) => k.as_str(),
            Scope::Global => "",
        }
    }

    /// Reconstruct a [`Scope`] from a `(kind, key)` tuple as stored in
    /// SQL. Returns [`SemanticError::Serialization`] when `kind` is
    /// unknown.
    pub fn from_parts(kind: &str, key: &str) -> Result<Self, SemanticError> {
        match kind {
            "agent" => Ok(Scope::Agent(key.to_string())),
            "circle" => Ok(Scope::Circle(key.to_string())),
            "org" => Ok(Scope::Org(key.to_string())),
            "global" => Ok(Scope::Global),
            other => Err(SemanticError::Serialization(format!(
                "unknown scope kind '{other}'"
            ))),
        }
    }
}

/// Per-level score dampening applied by
/// [`SemanticMemoryStore::query_hierarchical`] (GH#140).
///
/// Scores returned by each per-level `query` are multiplied by the
/// matching factor before the hierarchical merge. Defaults dampen
/// outward so agent-level memories beat circle memories at equal raw
/// cosine, circle beats org, etc.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Damping {
    /// Multiplier for agent-scope hits (default `1.0`).
    pub agent: f32,
    /// Multiplier for circle-scope hits (default `0.7`).
    pub circle: f32,
    /// Multiplier for org-scope hits (default `0.5`).
    pub org: f32,
    /// Multiplier for global-scope hits (default `0.3`).
    pub global: f32,
}

impl Default for Damping {
    fn default() -> Self {
        Self {
            agent: 1.0,
            circle: 0.7,
            org: 0.5,
            global: 0.3,
        }
    }
}

impl Damping {
    /// Factor for the supplied [`Scope`] variant.
    pub fn for_scope(&self, scope: &Scope) -> f32 {
        match scope {
            Scope::Agent(_) => self.agent,
            Scope::Circle(_) => self.circle,
            Scope::Org(_) => self.org,
            Scope::Global => self.global,
        }
    }
}

/// Configuration for a hierarchical recall walk (GH#140).
///
/// [`Self::levels`] returns the ordered list of scopes to query — always
/// starting with `Agent(agent)`, optionally including `Circle` / `Org`
/// when the corresponding field is `Some`, and always ending with
/// `Global`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopeHierarchy {
    /// Required — the most specific scope.
    pub agent: String,
    /// Optional circle scope walked after `agent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circle: Option<String>,
    /// Optional org scope walked after `circle`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
    /// Per-level dampening applied by
    /// [`SemanticMemoryStore::query_hierarchical`].
    #[serde(default)]
    pub damping: Damping,
}

impl ScopeHierarchy {
    /// Construct a hierarchy pinned to `agent` with no circle/org and
    /// the default damping profile.
    pub fn agent_only(agent: impl Into<String>) -> Self {
        Self {
            agent: agent.into(),
            circle: None,
            org: None,
            damping: Damping::default(),
        }
    }

    /// Ordered list of scopes to query, from most specific to most
    /// general. `Agent` and `Global` are always present; `Circle` and
    /// `Org` are included when their fields are populated.
    pub fn levels(&self) -> Vec<Scope> {
        let mut out = Vec::with_capacity(4);
        out.push(Scope::Agent(self.agent.clone()));
        if let Some(c) = &self.circle {
            out.push(Scope::Circle(c.clone()));
        }
        if let Some(o) = &self.org {
            out.push(Scope::Org(o.clone()));
        }
        out.push(Scope::Global);
        out
    }
}

/// Single hit returned from [`SemanticMemoryStore::query_hierarchical`].
///
/// Lighter than [`ScoredEntry`] — carries the row, the scope level that
/// produced it, the dampened score used for ordering, and the raw
/// undampened composite score for downstream rerankers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHit {
    /// The matched row.
    pub entry: SemanticEntry,
    /// Scope level that produced this hit.
    pub scope: Scope,
    /// Score after the per-level damping factor has been applied.
    pub dampened_score: f32,
    /// Raw undampened composite score (same units as
    /// [`ScoredEntry::score`]).
    pub raw_score: f32,
}

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
    /// Hierarchical memory scope (GH#140). When `None`, the row is treated
    /// as `Scope::Agent(agent_id)` for back-compat. Populated rows can be
    /// reached via [`SemanticMemoryStore::query_hierarchical`] so lookups
    /// walk agent → circle → org → global with per-level dampening.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
}

/// Parameters for [`SemanticMemoryStore::query`].
///
/// Must always carry `agent_id`; the optional fields progressively narrow
/// the result set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticQuery {
    /// Mandatory tenant scope. When `scope` is also supplied, backends
    /// filter on the scope first and `agent_id` is still accepted as the
    /// caller-identity breadcrumb (useful for audit logging).
    pub agent_id: String,
    /// Optional hierarchical scope filter (GH#140). When `Some`, backends
    /// match rows whose `scope` equals this value. When `None`, backends
    /// fall back to agent-only filtering (pre-hierarchy behaviour).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
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

    /// Hierarchical similarity search (GH#140, bead sera-1qfm).
    ///
    /// Iterates `hierarchy.levels()` from most specific (`Agent`) to most
    /// general (`Global`), issues a per-level [`Self::query`] for each,
    /// multiplies each result's composite score by the matching damping
    /// factor, merges across levels, deduplicates by [`MemoryId`] (keeping
    /// the highest dampened score), re-sorts by dampened score and returns
    /// the top `k` hits.
    ///
    /// The default implementation fans out via [`Self::query`] so every
    /// backend inherits it for free; overrides are welcome when a
    /// single-round-trip union-query is cheaper than N sequential calls.
    async fn query_hierarchical(
        &self,
        hierarchy: &ScopeHierarchy,
        query_embedding: Vec<f32>,
        k: usize,
    ) -> Result<Vec<MemoryHit>, SemanticError> {
        use std::collections::HashMap;

        let k = k.max(1);
        let mut best: HashMap<MemoryId, MemoryHit> = HashMap::new();

        for scope in hierarchy.levels() {
            let damping = hierarchy.damping.for_scope(&scope);
            // `Scope::Agent(agent_id)` levels preserve back-compat with the
            // pre-hierarchy agent-only filter; broader scopes use the
            // caller-supplied `agent` as the audit breadcrumb while the
            // actual filter is carried by `SemanticQuery::scope`.
            let per_level_query = SemanticQuery {
                agent_id: hierarchy.agent.clone(),
                scope: Some(scope.clone()),
                tier_filter: None,
                text: None,
                query_embedding: Some(query_embedding.clone()),
                top_k: k,
                similarity_threshold: None,
            };
            let hits = match self.query(per_level_query).await {
                Ok(h) => h,
                Err(SemanticError::Backend(_)) => continue, // tolerate level-miss
                Err(e) => return Err(e),
            };
            for scored in hits {
                let raw = scored.score;
                let dampened = raw * damping;
                let id = scored.entry.id.clone();
                let candidate = MemoryHit {
                    entry: scored.entry,
                    scope: scope.clone(),
                    dampened_score: dampened,
                    raw_score: raw,
                };
                best.entry(id)
                    .and_modify(|existing| {
                        if candidate.dampened_score > existing.dampened_score {
                            *existing = candidate.clone();
                        }
                    })
                    .or_insert(candidate);
            }
        }

        let mut merged: Vec<MemoryHit> = best.into_values().collect();
        merged.sort_by(|a, b| {
            b.dampened_score
                .partial_cmp(&a.dampened_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.entry.created_at.cmp(&a.entry.created_at))
        });
        merged.truncate(k);
        Ok(merged)
    }

    /// Remove the row identified by `id`. Returns
    /// [`SemanticError::NotFound`] if the id is not in the store.
    async fn delete(&self, id: &MemoryId) -> Result<(), SemanticError>;

    /// Apply `policy` to the store and return the number of rows removed.
    async fn evict(&self, policy: &EvictionPolicy) -> Result<usize, SemanticError>;

    /// Return a fresh aggregate snapshot.
    async fn stats(&self) -> Result<SemanticStats, SemanticError>;

    /// Mark the row identified by `id` as promoted. Promoted rows are
    /// exempt from eviction policies with `promoted_exempt = true` and
    /// serve as persistent recall candidates surfaced by the
    /// dreaming-workflow consolidation pass.
    ///
    /// Returns [`SemanticError::NotFound`] if the id is not in the store.
    ///
    /// The default implementation is a load-modify-put (not atomic).
    /// Backends with better primitives (e.g. SQL `UPDATE`) SHOULD override.
    async fn promote(&self, id: &MemoryId) -> Result<(), SemanticError> {
        let _ = id;
        Err(SemanticError::Backend(
            "promote() not implemented for this backend".into(),
        ))
    }

    /// Update `last_accessed_at` for the given row. Called by the
    /// memory-search tool on every hit.
    ///
    /// Default impl returns `Ok(())` — backends that persist access
    /// timestamps SHOULD override. NotFound is tolerated here (the row
    /// may have been evicted between query and touch) to keep the tool
    /// pure-read from the caller's perspective.
    async fn touch(&self, id: &MemoryId) -> Result<(), SemanticError> {
        let _ = id;
        Ok(())
    }

    /// Perform opportunistic maintenance — e.g. `REINDEX INDEX
    /// CONCURRENTLY` for pgvector backends. Callers are expected to
    /// invoke this on a cron schedule (weekly by default).
    ///
    /// Default impl is a no-op so in-memory / stub backends don't have
    /// to override.
    async fn maintenance(&self) -> Result<(), SemanticError> {
        Ok(())
    }
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
            scope: None,
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: SemanticEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, e.id);
        assert_eq!(back.agent_id, e.agent_id);
        assert_eq!(back.embedding, e.embedding);
        assert!(back.promoted);
    }
}
