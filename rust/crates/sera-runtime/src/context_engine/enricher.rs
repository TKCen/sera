//! Tier-2 semantic-memory enricher.
//!
//! Runs before every turn's `MemoryBlockAssembler::assemble()` call. Embeds the
//! current user message, queries the semantic memory store for the top-K nearest
//! rows, reranks them via the shared [`HybridScorer`] (BM25 + cosine + recency),
//! and promotes the top 1–3 hits as `SegmentKind::MemoryRecall` segments with
//! priority `4` (below Soul=0, SystemPrompt=1, Persona=2, Skill=3).
//!
//! ## Failure Policy (SPEC-memory §13.6)
//!
//! Enrichment is non-essential — the turn must complete even when the embedding
//! service or semantic store is unavailable. On any error or timeout, the
//! enricher logs a `warn!` with context, increments the
//! `semantic_enrichment_failures_total` metric placeholder (emitted as a
//! structured log field for now), and returns an empty segment list.
//!
//! ## Feature Flag
//!
//! When [`ContextEnricherConfig::enabled`] is `false`, [`ContextEnricher::enrich`]
//! returns an empty vector without calling the embedding service or semantic
//! store. The intent is to keep configuration-driven disable a pure no-op so
//! failing backends cannot leak cost into disabled deployments.
//!
//! ## Budget Check
//!
//! Callers pass the character budget remaining after Soul + SystemPrompt +
//! Persona + Skill segments. When that value is `0`, the enricher short-circuits
//! without running a query. Otherwise it promotes segments whose combined
//! `char_budget` fits the remaining space; later segments are truncated to fit.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sera_types::memory::{MemorySegment, SegmentKind};
use sera_types::{
    EmbeddingService, ScopeHierarchy, ScoredEntry, SemanticMemoryStore, SemanticQuery,
};

use super::hybrid::{tokenise, Candidate, HybridRetrievalConfig, HybridScorer};

/// Maximum number of recall segments promoted per turn.
pub const MAX_RECALL_SEGMENTS: usize = 3;

/// Priority assigned to promoted `MemoryRecall` segments.
///
/// Orders below Soul (0), SystemPrompt (1), Persona (2), and Skill (3) so that
/// Tier-2 recalls are the first evictables under char-budget pressure.
pub const MEMORY_RECALL_PRIORITY: u8 = 4;

/// Configuration for [`ContextEnricher`].
#[derive(Debug, Clone)]
pub struct ContextEnricherConfig {
    /// Master feature flag. When `false`, enrichment is a pure no-op.
    pub enabled: bool,
    /// How many rows to fetch from the store before reranking.
    ///
    /// The store returns up to `top_k * 4` candidates so the rerank has a
    /// meaningful pool to reorder. Final output is capped at
    /// [`MAX_RECALL_SEGMENTS`].
    pub top_k: usize,
    /// Minimum score required for a store hit to survive initial filtering.
    ///
    /// Forwarded to [`SemanticQuery::similarity_threshold`]. Backends that do
    /// not honour the threshold drop this field silently.
    pub similarity_threshold: Option<f32>,
    /// Hard wall-clock timeout on embedding + store query. On timeout the
    /// enricher returns an empty segment list.
    pub timeout_ms: u64,
    /// Per-segment character budget for promoted recalls.
    pub recall_char_budget: usize,
    /// Hybrid rerank configuration. Must pass [`HybridRetrievalConfig::validate`].
    pub hybrid: HybridRetrievalConfig,
    /// GH#140 — when `true`, the enricher calls
    /// [`SemanticMemoryStore::query_hierarchical`] with [`Self::scope_hierarchy`]
    /// instead of the agent-only [`SemanticMemoryStore::query`]. Leaving this
    /// `false` preserves the pre-GH#140 behaviour byte-for-byte.
    pub hierarchical_scopes_enabled: bool,
    /// Scope chain used by the hierarchical query when
    /// [`Self::hierarchical_scopes_enabled`] is `true`. `None` together with
    /// the flag set means "no hierarchy configured"; the enricher falls back
    /// to the agent-only path for safety.
    pub scope_hierarchy: Option<ScopeHierarchy>,
}

impl Default for ContextEnricherConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            top_k: 3,
            similarity_threshold: None,
            timeout_ms: 150,
            recall_char_budget: 512,
            hybrid: HybridRetrievalConfig::default(),
            hierarchical_scopes_enabled: false,
            scope_hierarchy: None,
        }
    }
}

/// Reference to a semantic-memory enrichment pipeline.
///
/// Holds the embedding service, the store, and a configuration block. All
/// fields are `Arc`-wrapped so the enricher is cheap to clone into async
/// tasks if the runtime later spawns recall in parallel with other prep work.
#[derive(Clone)]
pub struct ContextEnricher {
    embedding: Arc<dyn EmbeddingService>,
    store: Arc<dyn SemanticMemoryStore>,
    config: ContextEnricherConfig,
    /// Agent identifier — every store query must carry this (multi-tenant
    /// isolation per SPEC-memory §13.1).
    agent_id: String,
}

impl std::fmt::Debug for ContextEnricher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextEnricher")
            .field("embedding_model", &self.embedding.model_id())
            .field("embedding_dimensions", &self.embedding.dimensions())
            .field("config", &self.config)
            .field("agent_id", &self.agent_id)
            .finish()
    }
}

impl ContextEnricher {
    /// Construct a new enricher. All arguments are retained for the life of
    /// the returned value.
    pub fn new(
        embedding: Arc<dyn EmbeddingService>,
        store: Arc<dyn SemanticMemoryStore>,
        config: ContextEnricherConfig,
        agent_id: impl Into<String>,
    ) -> Self {
        Self {
            embedding,
            store,
            config,
            agent_id: agent_id.into(),
        }
    }

    /// Borrow the configured embedding service.
    pub fn embedding(&self) -> &Arc<dyn EmbeddingService> {
        &self.embedding
    }

    /// Borrow the configured store.
    pub fn store(&self) -> &Arc<dyn SemanticMemoryStore> {
        &self.store
    }

    /// Borrow the configuration.
    pub fn config(&self) -> &ContextEnricherConfig {
        &self.config
    }

    /// Produce up to [`MAX_RECALL_SEGMENTS`] `MemoryRecall` segments for
    /// `user_message`. Never errors — failures are logged and surfaced as an
    /// empty return.
    ///
    /// `budget_remaining` is the character budget left in the turn's
    /// [`sera_types::memory::MemoryBlock`] after Soul, SystemPrompt, Persona,
    /// and Skill segments have been accounted for. When the value is `0`,
    /// the enricher short-circuits without any upstream calls.
    ///
    /// Returns `(segments, query_embedding)` so callers can also feed the
    /// query embedding into the [`HybridScorer`] used by `ContextPipeline`.
    /// The embedding is `None` when enrichment was skipped, disabled, or
    /// failed.
    pub async fn enrich(
        &self,
        user_message: &str,
        budget_remaining: usize,
    ) -> EnrichmentResult {
        if !self.config.enabled {
            return EnrichmentResult::disabled();
        }
        if budget_remaining == 0 {
            tracing::debug!(
                "semantic enrichment skipped: budget_remaining=0"
            );
            return EnrichmentResult::empty();
        }
        if user_message.trim().is_empty() {
            return EnrichmentResult::empty();
        }

        let timeout = Duration::from_millis(self.config.timeout_ms);
        let query_tokens = tokenise(user_message);

        // Run embedding + store query under a single wall-clock timeout so the
        // turn-critical path cannot stall on a slow provider.
        let work = self.fetch_candidates(user_message);
        match tokio::time::timeout(timeout, work).await {
            Ok(Ok((query_embedding, hits))) => {
                let segments = self.rerank_and_promote(
                    hits,
                    query_tokens,
                    &query_embedding,
                    budget_remaining,
                );
                EnrichmentResult {
                    segments,
                    query_embedding: Some(query_embedding),
                }
            }
            Ok(Err(EnrichmentFailure::Embedding(err))) => {
                tracing::warn!(
                    error = %err,
                    metric = "semantic_enrichment_failures_total",
                    reason = "embedding",
                    "semantic enrichment degraded: embedding call failed"
                );
                EnrichmentResult::empty()
            }
            Ok(Err(EnrichmentFailure::Store(err))) => {
                tracing::warn!(
                    error = %err,
                    metric = "semantic_enrichment_failures_total",
                    reason = "store",
                    "semantic enrichment degraded: store query failed"
                );
                EnrichmentResult::empty()
            }
            Err(_elapsed) => {
                tracing::warn!(
                    timeout_ms = self.config.timeout_ms,
                    metric = "semantic_enrichment_failures_total",
                    reason = "timeout",
                    "semantic enrichment degraded: timed out"
                );
                EnrichmentResult::empty()
            }
        }
    }

    async fn fetch_candidates(
        &self,
        user_message: &str,
    ) -> Result<(Vec<f32>, Vec<ScoredEntry>), EnrichmentFailure> {
        let vectors = self
            .embedding
            .embed(&[user_message.to_string()])
            .await
            .map_err(|e| EnrichmentFailure::Embedding(e.to_string()))?;
        let query_embedding = vectors
            .into_iter()
            .next()
            .ok_or_else(|| {
                EnrichmentFailure::Embedding(
                    "embedding service returned no vectors for a single-input batch"
                        .to_string(),
                )
            })?;

        let pool_size = self.config.top_k.saturating_mul(4).max(self.config.top_k);

        // GH#140: walk Agent → Circle → Org → Global when the flag is on AND a
        // hierarchy is configured. Otherwise fall back to the agent-only path
        // so existing deployments stay byte-identical.
        let hits = if self.config.hierarchical_scopes_enabled
            && let Some(hierarchy) = self.config.scope_hierarchy.as_ref()
        {
            let merged = self
                .store
                .query_hierarchical(hierarchy, query_embedding.clone(), pool_size)
                .await
                .map_err(|e| EnrichmentFailure::Store(e.to_string()))?;
            // Rebuild `ScoredEntry`s from `MemoryHit`s so the downstream
            // hybrid scorer sees the dampened scores. Per-signal sub-scores
            // are flattened onto the composite since the hierarchy merge
            // has already picked the best per-id row.
            merged
                .into_iter()
                .map(|hit| ScoredEntry {
                    score: hit.dampened_score,
                    index_score: 0.0,
                    vector_score: hit.raw_score,
                    recency_score: 0.0,
                    entry: hit.entry,
                })
                .collect()
        } else {
            let query = SemanticQuery {
                agent_id: self.agent_id.clone(),
                tier_filter: None,
                text: Some(user_message.to_string()),
                query_embedding: Some(query_embedding.clone()),
                top_k: pool_size,
                similarity_threshold: self.config.similarity_threshold,
                scope: None,
            };
            self.store
                .query(query)
                .await
                .map_err(|e| EnrichmentFailure::Store(e.to_string()))?
        };
        Ok((query_embedding, hits))
    }

    fn rerank_and_promote(
        &self,
        hits: Vec<ScoredEntry>,
        query_tokens: Vec<String>,
        query_embedding: &[f32],
        budget_remaining: usize,
    ) -> Vec<MemorySegment> {
        if hits.is_empty() {
            return Vec::new();
        }

        let now = Utc::now();
        let candidates: Vec<Candidate> = hits
            .iter()
            .map(|h| Candidate {
                id: h.entry.id.as_str().to_string(),
                tokens: tokenise(&h.entry.content),
                embedding: h.entry.embedding.clone().unwrap_or_default(),
                created_at: h.entry.created_at,
            })
            .collect();

        let scorer = HybridScorer::new(
            self.config.hybrid.clone(),
            query_tokens,
            query_embedding.to_vec(),
            &candidates,
            now,
        );
        let ranked = scorer.rank();

        let max = self.config.top_k.min(MAX_RECALL_SEGMENTS);
        let per_seg_budget = self.config.recall_char_budget;

        let mut out: Vec<MemorySegment> = Vec::with_capacity(max);
        let mut remaining = budget_remaining;
        for scored in ranked.into_iter().take(max) {
            if remaining == 0 {
                break;
            }
            // Find the matching ScoredEntry via id. `ranked` holds references
            // into `candidates`, which were cloned from `hits` in order; the
            // ids are preserved so we can round-trip without cloning hits.
            let hit = match hits.iter().find(|h| h.entry.id.as_str() == scored.candidate.id) {
                Some(h) => h,
                None => continue,
            };

            let allowed = per_seg_budget.min(remaining);
            let content = truncate_to(&hit.entry.content, allowed);
            let content_len = content.len();
            out.push(MemorySegment {
                id: format!("recall:{}", hit.entry.id.as_str()),
                content,
                priority: MEMORY_RECALL_PRIORITY,
                // Carry the combined hybrid score as a tiebreaker boost. A
                // default of 1.0 keeps behaviour identical when the scorer
                // produces a flat ranking (e.g. single candidate).
                recency_boost: (scored.combined_score as f32).max(f32::EPSILON),
                char_budget: per_seg_budget,
                kind: SegmentKind::MemoryRecall(hit.entry.id.as_str().to_string()),
            });
            remaining = remaining.saturating_sub(content_len);
        }
        out
    }
}

/// Output of a single [`ContextEnricher::enrich`] call.
///
/// `segments` is always populated (possibly empty). `query_embedding` is
/// `Some` only when enrichment actually ran — disabled, timed-out, or
/// no-op cases yield `None` so callers can distinguish lexical-only fallback
/// from a successful-but-empty result.
#[derive(Debug, Clone)]
pub struct EnrichmentResult {
    /// Promoted `MemoryRecall` segments (0..=[`MAX_RECALL_SEGMENTS`]).
    pub segments: Vec<MemorySegment>,
    /// Query embedding used for the store query. Forwarded to
    /// [`HybridScorer`] so the context pipeline reranks with real vector
    /// similarity instead of the zero-vector fallback.
    pub query_embedding: Option<Vec<f32>>,
}

impl EnrichmentResult {
    fn empty() -> Self {
        Self {
            segments: Vec::new(),
            query_embedding: None,
        }
    }

    fn disabled() -> Self {
        Self::empty()
    }
}

#[derive(Debug)]
enum EnrichmentFailure {
    Embedding(String),
    Store(String),
}

/// Truncate `s` to at most `max_chars` bytes, splitting on a UTF-8 char
/// boundary so we never produce invalid UTF-8. Returns a borrowed slice
/// when the input already fits.
fn truncate_to(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    // Walk down from max_chars until we hit a char boundary. `str::is_char_boundary`
    // is O(1) so this terminates in at most 4 iterations for UTF-8.
    let mut end = max_chars;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use chrono::Utc;
    use sera_types::{
        EmbeddingError, EmbeddingHealth, EmbeddingService, EvictionPolicy, MemoryId, PutRequest,
        ScoredEntry, SemanticEntry, SemanticError, SemanticMemoryStore, SemanticQuery,
        SemanticStats,
    };

    use super::*;

    // ── Mocks ────────────────────────────────────────────────────────────────

    /// Deterministic embedding that returns a single fixed vector per call.
    struct FixedEmbedding {
        vector: Vec<f32>,
        calls: AtomicUsize,
    }

    impl FixedEmbedding {
        fn new(vector: Vec<f32>) -> Self {
            Self {
                vector,
                calls: AtomicUsize::new(0),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl EmbeddingService for FixedEmbedding {
        fn model_id(&self) -> &str {
            "fixed"
        }
        fn dimensions(&self) -> usize {
            self.vector.len()
        }
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(texts.iter().map(|_| self.vector.clone()).collect())
        }
        async fn health(&self) -> Result<EmbeddingHealth, EmbeddingError> {
            Ok(EmbeddingHealth {
                available: true,
                detail: "ok".into(),
                latency_ms: Some(0),
            })
        }
    }

    /// Embedding service that always returns a provider error.
    struct FailingEmbedding;

    #[async_trait]
    impl EmbeddingService for FailingEmbedding {
        fn model_id(&self) -> &str {
            "failing"
        }
        fn dimensions(&self) -> usize {
            4
        }
        async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            Err(EmbeddingError::Provider("backend unavailable".into()))
        }
        async fn health(&self) -> Result<EmbeddingHealth, EmbeddingError> {
            Err(EmbeddingError::Provider("down".into()))
        }
    }

    /// Embedding service that panics if called (used to assert disabled-flag
    /// guarantees).
    struct PanicEmbedding;

    #[async_trait]
    impl EmbeddingService for PanicEmbedding {
        fn model_id(&self) -> &str {
            "panic"
        }
        fn dimensions(&self) -> usize {
            4
        }
        async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            panic!("embed() must not be called when enrichment is disabled");
        }
        async fn health(&self) -> Result<EmbeddingHealth, EmbeddingError> {
            panic!("health() must not be called when enrichment is disabled");
        }
    }

    /// Embedding service that sleeps longer than any configured timeout.
    struct SlowEmbedding {
        delay_ms: u64,
    }

    #[async_trait]
    impl EmbeddingService for SlowEmbedding {
        fn model_id(&self) -> &str {
            "slow"
        }
        fn dimensions(&self) -> usize {
            4
        }
        async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            Ok(vec![vec![0.0; 4]])
        }
        async fn health(&self) -> Result<EmbeddingHealth, EmbeddingError> {
            Ok(EmbeddingHealth {
                available: true,
                detail: "slow".into(),
                latency_ms: Some(0),
            })
        }
    }

    /// Store that returns a caller-supplied hit list verbatim.
    struct CannedStore {
        hits: Vec<ScoredEntry>,
    }

    #[async_trait]
    impl SemanticMemoryStore for CannedStore {
        async fn put(&self, _req: PutRequest) -> Result<MemoryId, SemanticError> {
            Ok(MemoryId::new("canned"))
        }
        async fn query(&self, _q: SemanticQuery) -> Result<Vec<ScoredEntry>, SemanticError> {
            Ok(self.hits.clone())
        }
        async fn delete(&self, _id: &MemoryId) -> Result<(), SemanticError> {
            Ok(())
        }
        async fn evict(&self, _p: &EvictionPolicy) -> Result<usize, SemanticError> {
            Ok(0)
        }
        async fn stats(&self) -> Result<SemanticStats, SemanticError> {
            Ok(SemanticStats {
                total_rows: self.hits.len(),
                per_agent_top: vec![],
                oldest: Utc::now(),
                newest: Utc::now(),
            })
        }
    }

    /// Store that always errors on `query`.
    struct FailingStore;

    #[async_trait]
    impl SemanticMemoryStore for FailingStore {
        async fn put(&self, _req: PutRequest) -> Result<MemoryId, SemanticError> {
            Ok(MemoryId::new("failing"))
        }
        async fn query(&self, _q: SemanticQuery) -> Result<Vec<ScoredEntry>, SemanticError> {
            Err(SemanticError::Backend("db exploded".into()))
        }
        async fn delete(&self, _id: &MemoryId) -> Result<(), SemanticError> {
            Ok(())
        }
        async fn evict(&self, _p: &EvictionPolicy) -> Result<usize, SemanticError> {
            Ok(0)
        }
        async fn stats(&self) -> Result<SemanticStats, SemanticError> {
            Err(SemanticError::Backend("nope".into()))
        }
    }

    /// Store that panics if `query` is called.
    struct PanicStore;

    #[async_trait]
    impl SemanticMemoryStore for PanicStore {
        async fn put(&self, _req: PutRequest) -> Result<MemoryId, SemanticError> {
            Ok(MemoryId::new("panic-store"))
        }
        async fn query(&self, _q: SemanticQuery) -> Result<Vec<ScoredEntry>, SemanticError> {
            panic!("query() must not be called when enrichment is disabled");
        }
        async fn delete(&self, _id: &MemoryId) -> Result<(), SemanticError> {
            Ok(())
        }
        async fn evict(&self, _p: &EvictionPolicy) -> Result<usize, SemanticError> {
            Ok(0)
        }
        async fn stats(&self) -> Result<SemanticStats, SemanticError> {
            Err(SemanticError::Backend("nope".into()))
        }
    }

    fn hit(id: &str, content: &str, score: f32) -> ScoredEntry {
        ScoredEntry {
            entry: SemanticEntry {
                id: MemoryId::new(id),
                agent_id: "agent-a".to_string(),
                content: content.to_string(),
                embedding: Some(vec![1.0, 0.0, 0.0, 0.0]),
                tier: SegmentKind::MemoryRecall(id.to_string()),
                tags: vec![],
                created_at: Utc::now(),
                last_accessed_at: None,
                promoted: false,
                scope: None,
            },
            score,
            index_score: score,
            vector_score: score,
            recency_score: score,
        }
    }

    fn enabled_config() -> ContextEnricherConfig {
        ContextEnricherConfig {
            enabled: true,
            top_k: 3,
            similarity_threshold: None,
            timeout_ms: 500,
            recall_char_budget: 256,
            hybrid: HybridRetrievalConfig::default(),
            hierarchical_scopes_enabled: false,
            scope_hierarchy: None,
        }
    }

    // ── Happy path ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn happy_path_promotes_top_three() {
        let embedding = Arc::new(FixedEmbedding::new(vec![1.0, 0.0, 0.0, 0.0]));
        let store = Arc::new(CannedStore {
            hits: vec![
                hit("m1", "rust async programming", 0.9),
                hit("m2", "python concurrency", 0.7),
                hit("m3", "rust ownership", 0.6),
                hit("m4", "javascript promises", 0.4),
                hit("m5", "go goroutines", 0.3),
            ],
        });
        let enricher = ContextEnricher::new(
            embedding.clone(),
            store,
            enabled_config(),
            "agent-a",
        );

        let result = enricher.enrich("how do I write rust async code?", 10_000).await;

        assert_eq!(result.segments.len(), MAX_RECALL_SEGMENTS);
        assert!(result.query_embedding.is_some(), "happy path must carry embedding");
        for seg in &result.segments {
            assert_eq!(seg.priority, MEMORY_RECALL_PRIORITY);
            assert!(matches!(seg.kind, SegmentKind::MemoryRecall(_)));
            assert!(seg.id.starts_with("recall:"));
        }
        assert_eq!(embedding.call_count(), 1);
    }

    // ── Disabled flag is a pure no-op ───────────────────────────────────────

    #[tokio::test]
    async fn disabled_flag_is_noop_and_never_calls_backends() {
        // Using panicking mocks proves no upstream call happens.
        let embedding: Arc<dyn EmbeddingService> = Arc::new(PanicEmbedding);
        let store: Arc<dyn SemanticMemoryStore> = Arc::new(PanicStore);
        let mut cfg = enabled_config();
        cfg.enabled = false;
        let enricher = ContextEnricher::new(embedding, store, cfg, "agent-a");

        let result = enricher.enrich("any query", 10_000).await;
        assert!(result.segments.is_empty());
        assert!(result.query_embedding.is_none());
    }

    // ── Store failure degrades silently ─────────────────────────────────────

    #[tokio::test]
    async fn store_failure_swallowed_returns_empty() {
        let embedding = Arc::new(FixedEmbedding::new(vec![1.0, 0.0, 0.0, 0.0]));
        let store = Arc::new(FailingStore);
        let enricher = ContextEnricher::new(
            embedding,
            store,
            enabled_config(),
            "agent-a",
        );

        let result = enricher.enrich("query", 10_000).await;
        assert!(result.segments.is_empty(), "store failure must yield empty segments");
        assert!(result.query_embedding.is_none(), "failure must not leak embedding");
    }

    // ── Embedding failure degrades silently ─────────────────────────────────

    #[tokio::test]
    async fn embedding_failure_swallowed_returns_empty() {
        let embedding = Arc::new(FailingEmbedding);
        let store = Arc::new(CannedStore { hits: vec![] });
        let enricher = ContextEnricher::new(
            embedding,
            store,
            enabled_config(),
            "agent-a",
        );

        let result = enricher.enrich("query", 10_000).await;
        assert!(result.segments.is_empty());
        assert!(result.query_embedding.is_none());
    }

    // ── Timeout degrades silently ───────────────────────────────────────────

    #[tokio::test]
    async fn timeout_returns_empty() {
        let embedding = Arc::new(SlowEmbedding { delay_ms: 200 });
        let store = Arc::new(CannedStore {
            hits: vec![hit("m1", "rust async", 0.9)],
        });
        let mut cfg = enabled_config();
        cfg.timeout_ms = 10; // much shorter than SlowEmbedding's 200ms sleep
        let enricher = ContextEnricher::new(embedding, store, cfg, "agent-a");

        let result = enricher.enrich("query", 10_000).await;
        assert!(result.segments.is_empty(), "timeout must yield empty segments");
        assert!(result.query_embedding.is_none());
    }

    // ── Budget=0 short-circuits without calling backends ────────────────────

    #[tokio::test]
    async fn budget_zero_skips_without_upstream_calls() {
        let embedding: Arc<dyn EmbeddingService> = Arc::new(PanicEmbedding);
        let store: Arc<dyn SemanticMemoryStore> = Arc::new(PanicStore);
        let enricher = ContextEnricher::new(
            embedding,
            store,
            enabled_config(),
            "agent-a",
        );

        let result = enricher.enrich("non-empty query", 0).await;
        assert!(result.segments.is_empty());
        assert!(result.query_embedding.is_none());
    }

    // ── Top-K truncation: store returns 10 hits, enricher promotes at most 3

    #[tokio::test]
    async fn topk_truncation_caps_at_three() {
        let embedding = Arc::new(FixedEmbedding::new(vec![1.0, 0.0, 0.0, 0.0]));
        let hits: Vec<_> = (0..10)
            .map(|i| hit(&format!("m{i}"), &format!("content {i}"), 0.9 - (i as f32) * 0.05))
            .collect();
        let store = Arc::new(CannedStore { hits });
        let enricher = ContextEnricher::new(
            embedding,
            store,
            enabled_config(),
            "agent-a",
        );

        let result = enricher.enrich("query", 10_000).await;
        assert!(
            result.segments.len() <= MAX_RECALL_SEGMENTS,
            "expected at most {MAX_RECALL_SEGMENTS} segments, got {}",
            result.segments.len()
        );
        assert_eq!(result.segments.len(), MAX_RECALL_SEGMENTS);
    }

    // ── Empty hit list returns empty segments with an embedding ─────────────

    #[tokio::test]
    async fn empty_store_hits_produces_empty_segments_but_keeps_embedding() {
        let embedding = Arc::new(FixedEmbedding::new(vec![1.0, 0.0, 0.0, 0.0]));
        let store = Arc::new(CannedStore { hits: vec![] });
        let enricher = ContextEnricher::new(
            embedding,
            store,
            enabled_config(),
            "agent-a",
        );

        let result = enricher.enrich("query", 10_000).await;
        assert!(result.segments.is_empty());
        assert!(
            result.query_embedding.is_some(),
            "empty-hit path must still surface the embedding for the hybrid scorer"
        );
    }

    // ── Budget shrinks segment content ──────────────────────────────────────

    #[tokio::test]
    async fn tight_budget_truncates_segment_content() {
        let embedding = Arc::new(FixedEmbedding::new(vec![1.0, 0.0, 0.0, 0.0]));
        let store = Arc::new(CannedStore {
            hits: vec![hit("m1", "a".repeat(1000).as_str(), 0.9)],
        });
        let mut cfg = enabled_config();
        cfg.recall_char_budget = 100;
        let enricher = ContextEnricher::new(embedding, store, cfg, "agent-a");

        let result = enricher.enrich("query", 50).await;
        assert_eq!(result.segments.len(), 1);
        assert!(result.segments[0].content.len() <= 50);
    }

    // ── truncate_to helpers ─────────────────────────────────────────────────

    #[test]
    fn truncate_to_respects_utf8_boundary() {
        // Multi-byte char followed by ASCII; cap sits mid-codepoint.
        let s = "ü-world"; // ü is 2 bytes (0xc3 0xbc), then '-' etc.
        let out = truncate_to(s, 1);
        // end walks back to 0 because byte 1 is mid-codepoint for ü.
        assert_eq!(out, "");
    }

    #[test]
    fn truncate_to_returns_input_when_shorter() {
        let out = truncate_to("hi", 50);
        assert_eq!(out, "hi");
    }

    // ── GH#140: hierarchical scope enrichment ──────────────────────────────

    use sera_testing::semantic_memory::InMemorySemanticStore;
    use sera_types::{Damping, Scope, ScopeHierarchy};

    fn scoped_entry(agent: &str, content: &str, scope: Scope) -> SemanticEntry {
        SemanticEntry {
            id: MemoryId::new(format!("{agent}-{content}")),
            agent_id: agent.to_string(),
            content: content.to_string(),
            embedding: Some(vec![1.0, 0.0, 0.0, 0.0]),
            tier: SegmentKind::MemoryRecall(content.to_string()),
            tags: vec![],
            created_at: Utc::now(),
            last_accessed_at: None,
            promoted: false,
            scope: Some(scope),
        }
    }

    #[tokio::test]
    async fn hierarchical_flag_on_merges_three_scope_levels() {
        let embedding = Arc::new(FixedEmbedding::new(vec![1.0, 0.0, 0.0, 0.0]));
        let store = Arc::new(InMemorySemanticStore::new());
        store
            .insert_entry(scoped_entry("agent-a", "agent-row", Scope::Agent("agent-a".into())))
            .await
            .unwrap();
        store
            .insert_entry(scoped_entry("agent-a", "circle-row", Scope::Circle("ring".into())))
            .await
            .unwrap();
        store
            .insert_entry(scoped_entry("agent-a", "org-row", Scope::Org("acme".into())))
            .await
            .unwrap();
        store
            .insert_entry(scoped_entry("agent-a", "global-row", Scope::Global))
            .await
            .unwrap();

        let mut cfg = enabled_config();
        cfg.hierarchical_scopes_enabled = true;
        cfg.scope_hierarchy = Some(ScopeHierarchy {
            agent: "agent-a".into(),
            circle: Some("ring".into()),
            org: Some("acme".into()),
            damping: Damping::default(),
        });
        // top_k=10 to ensure all 4 hits survive the hybrid rerank cap
        cfg.top_k = 10;
        let enricher = ContextEnricher::new(embedding, store, cfg, "agent-a");

        let result = enricher.enrich("recall please", 10_000).await;
        assert!(result.query_embedding.is_some());
        // All 3 segments should come from distinct scope levels (cap=3).
        assert_eq!(result.segments.len(), MAX_RECALL_SEGMENTS);
        let contents: Vec<String> = result.segments.iter().map(|s| s.content.clone()).collect();
        // Exactly one scope is omitted — the lowest-dampened one (global).
        // Order is not guaranteed post-hybrid rerank, but agent/circle/org
        // must all be present.
        assert!(contents.iter().any(|c| c == "agent-row"));
        assert!(contents.iter().any(|c| c == "circle-row"));
        assert!(contents.iter().any(|c| c == "org-row"));
    }

    #[tokio::test]
    async fn hierarchical_flag_off_returns_only_agent_hits() {
        let embedding = Arc::new(FixedEmbedding::new(vec![1.0, 0.0, 0.0, 0.0]));
        let store = Arc::new(InMemorySemanticStore::new());
        store
            .insert_entry(scoped_entry("agent-a", "agent-row", Scope::Agent("agent-a".into())))
            .await
            .unwrap();
        // Rows under non-agent scopes but NOT agent_id=agent-a → invisible
        // to the agent-only path.
        store
            .insert_entry(scoped_entry("agent-b", "other-agent", Scope::Agent("agent-b".into())))
            .await
            .unwrap();

        let mut cfg = enabled_config();
        cfg.hierarchical_scopes_enabled = false;
        cfg.scope_hierarchy = Some(ScopeHierarchy {
            agent: "agent-a".into(),
            circle: Some("ring".into()),
            org: Some("acme".into()),
            damping: Damping::default(),
        });
        let enricher = ContextEnricher::new(embedding, store, cfg, "agent-a");

        let result = enricher.enrich("recall please", 10_000).await;
        assert_eq!(result.segments.len(), 1);
        assert_eq!(result.segments[0].content, "agent-row");
    }

    #[tokio::test]
    async fn hierarchical_flag_on_without_hierarchy_falls_back_to_agent() {
        let embedding = Arc::new(FixedEmbedding::new(vec![1.0, 0.0, 0.0, 0.0]));
        let store = Arc::new(InMemorySemanticStore::new());
        store
            .insert_entry(scoped_entry("agent-a", "agent-row", Scope::Agent("agent-a".into())))
            .await
            .unwrap();
        store
            .insert_entry(scoped_entry("agent-a", "global-row", Scope::Global))
            .await
            .unwrap();

        let mut cfg = enabled_config();
        cfg.hierarchical_scopes_enabled = true;
        cfg.scope_hierarchy = None; // flag on but no hierarchy configured
        let enricher = ContextEnricher::new(embedding, store, cfg, "agent-a");

        let result = enricher.enrich("recall", 10_000).await;
        // Agent-only path: global-row leaks in because it still carries
        // agent_id=agent-a (test row metadata), but it's NOT the dampened
        // hierarchical merge — it's a straight agent_id filter.
        assert!(!result.segments.is_empty());
        let contents: Vec<String> = result.segments.iter().map(|s| s.content.clone()).collect();
        assert!(contents.iter().any(|c| c == "agent-row"));
    }
}
