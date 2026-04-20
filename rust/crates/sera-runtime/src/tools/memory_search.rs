//! `memory_search` — Tier-2 semantic-memory recall tool (bead sera-tier2-d).
//!
//! Gives the LLM explicit access to Tier-2 semantic memory in addition to
//! the auto-promotion the `ContextEnricher` already performs. The tool
//! embeds a query string, runs a scoped `SemanticMemoryStore::query`,
//! updates `last_accessed_at` for each hit, and returns a batched
//! `MemorySearchResult` payload.
//!
//! ## Dedupe vs. enricher
//!
//! The enricher promotes up to 3 Tier-2 rows per turn as
//! `SegmentKind::MemoryRecall` segments. When the LLM also calls
//! `memory_search` in the same turn, some hits may already be rendered in
//! the turn's `MemoryBlock`. The dedupe seam is this tool's result
//! rendering: hits whose `MemoryId` appears in
//! `ToolContext.active_recall_ids` are replaced with a stub entry carrying
//! `already_in_context: true` instead of being dropped silently, so the
//! model can still correlate its query against what it already sees.
//!
//! ## Risk level
//!
//! `RiskLevel::Read`. The only write is `last_accessed_at`, which is a
//! pure access-tracking signal and not a material state change.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sera_types::memory::{MemoryId as TierOneMemoryId, MemorySearchResult, MemoryTier, SegmentKind};
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema,
};
use sera_types::EmbeddingService;
use sera_memory::{ScoredEntry, SemanticMemoryStore, SemanticQuery};

/// Default `top_k` when the caller omits the field.
const DEFAULT_TOP_K: usize = 5;

/// Hard upper bound on `top_k` to protect the backend.
const MAX_TOP_K: usize = 25;

/// Registered tool name — the LLM uses this verbatim.
pub const TOOL_NAME: &str = "memory_search";

/// Batched payload returned by the tool. Mirrors
/// [`sera_types::memory::MemorySearchResult`] at the entry level so
/// downstream code can consume either shape without translation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchToolResult {
    /// One entry per hit (post-dedupe against `active_recall_ids`).
    pub results: Vec<MemorySearchToolEntry>,
    /// Total hit count returned by the store before dedupe rendering.
    pub total: usize,
    /// Wall-clock search time in milliseconds.
    pub search_time_ms: u64,
}

/// One hit rendered by the tool. Carries the canonical
/// [`MemorySearchResult`] plus tier-2 metadata (tags) and an
/// `already_in_context` flag that lets the LLM skip redundant quoting
/// when the enricher has already surfaced the row this turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchToolEntry {
    #[serde(flatten)]
    pub hit: MemorySearchResult,
    /// Opaque tier-2 tags from [`sera_types::SemanticEntry::tags`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// `true` iff the entry's `MemoryId` already appears in
    /// `ToolContext.active_recall_ids` — meaning the enricher already
    /// promoted this row into the turn's `MemoryBlock`. When `true` the
    /// `hit.content` is emptied so the LLM doesn't quote the same text
    /// twice; the id is retained for correlation.
    #[serde(default, skip_serializing_if = "is_false")]
    pub already_in_context: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Spec-aligned `Tool` implementation for semantic recall.
///
/// Holds two `Arc`s (embedding service + store) so the runtime can share
/// these with the enricher without wiring a second provider.
pub struct MemorySearchTool {
    embedding: Arc<dyn EmbeddingService>,
    store: Arc<dyn SemanticMemoryStore>,
}

impl MemorySearchTool {
    /// Construct a new `MemorySearchTool` over the given embedding service
    /// and store. Both `Arc`s are shared with whatever other components
    /// (e.g. the `ContextEnricher`) wire the same backends.
    pub fn new(
        embedding: Arc<dyn EmbeddingService>,
        store: Arc<dyn SemanticMemoryStore>,
    ) -> Self {
        Self { embedding, store }
    }
}

impl std::fmt::Debug for MemorySearchTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemorySearchTool")
            .field("embedding_model", &self.embedding.model_id())
            .field("embedding_dimensions", &self.embedding.dimensions())
            .finish()
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: TOOL_NAME.to_string(),
            description:
                "Recall up to top_k long-term semantic-memory entries for this agent via \
                 embedding similarity. Returns a batched result; entries already surfaced \
                 in context are flagged with already_in_context=true."
                    .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["memory".to_string(), "tier-2".to_string()],
        }
    }

    fn schema(&self) -> ToolSchema {
        use std::collections::HashMap;
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "query".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Natural-language text to embed and match against stored memories.".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "top_k".to_string(),
            ParameterSchema {
                schema_type: "integer".to_string(),
                description: Some(format!(
                    "Maximum number of hits to return. Default {DEFAULT_TOP_K}, capped at {MAX_TOP_K}."
                )),
                enum_values: None,
                default: Some(serde_json::json!(DEFAULT_TOP_K)),
            },
        );
        properties.insert(
            "tier_filter".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "Optional SegmentKind matcher. Currently accepts \"memory_recall\" or \
                     \"skill:<id>\" syntactic forms; unknown values are ignored."
                        .to_string(),
                ),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["query".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let args = &input.arguments;

        let query_text = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing 'query' field".to_string()))?;

        if query_text.trim().is_empty() {
            return Err(ToolError::InvalidInput("'query' must not be empty".to_string()));
        }

        let top_k = args
            .get("top_k")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_TOP_K)
            .clamp(1, MAX_TOP_K);

        let tier_filter = args
            .get("tier_filter")
            .and_then(|v| v.as_str())
            .and_then(parse_tier_filter);

        let agent_id = extract_agent_id(&ctx).ok_or_else(|| {
            ToolError::InvalidInput(
                "memory_search requires an agent-scoped principal (agent:<id>)".to_string(),
            )
        })?;

        let start = Instant::now();

        // 1. Embed the query. Propagate embedding failures as execution
        //    errors — memory_search is explicit (LLM-requested) so a
        //    silent-empty response would be misleading.
        let mut vectors = self
            .embedding
            .embed(&[query_text.to_string()])
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("embedding failed: {e}")))?;

        let query_embedding = vectors.pop().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "embedding service returned no vectors for a single-input batch".to_string(),
            )
        })?;

        // 2. Query the store.
        let query = SemanticQuery {
            agent_id,
            tier_filter: tier_filter.clone(),
            text: Some(query_text.to_string()),
            query_embedding: Some(query_embedding),
            top_k,
            similarity_threshold: None,
            scope: None,
        };
        let hits: Vec<ScoredEntry> = self
            .store
            .query(query)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("store query failed: {e}")))?;

        // 3. Touch last_accessed_at on every hit (best-effort — NotFound
        //    tolerated per `SemanticMemoryStore::touch` contract).
        for hit in &hits {
            let _ = self.store.touch(&hit.entry.id).await;
        }

        // 4. Dedupe render against active_recall_ids. Entries already in
        //    context keep their id + score but drop content.
        let total = hits.len();
        let active: std::collections::HashSet<&str> =
            ctx.active_recall_ids.iter().map(|s| s.as_str()).collect();
        let results: Vec<MemorySearchToolEntry> = hits
            .into_iter()
            .map(|h| render_entry(h, &active))
            .collect();

        let elapsed = start.elapsed().as_millis() as u64;
        let result = MemorySearchToolResult {
            results,
            total,
            search_time_ms: elapsed,
        };

        let payload = serde_json::to_string(&result).map_err(|e| {
            ToolError::ExecutionFailed(format!("serialize memory_search result: {e}"))
        })?;
        Ok(ToolOutput::success(payload))
    }
}

/// Extract the bare agent id from a `PrincipalRef`. The runtime formats
/// the principal id as `agent:<id>` — see
/// `default_runtime::execute_turn` for the canonical construction site.
fn extract_agent_id(ctx: &ToolContext) -> Option<String> {
    let raw = ctx.principal.id.0.as_str();
    raw.strip_prefix("agent:").map(|s| s.to_string())
}

/// Translate a free-form `tier_filter` string into a `SegmentKind`.
///
/// Accepts a handful of shapes so the LLM can filter without knowing the
/// exact enum variant names:
///
/// - `"memory_recall"` / `"memoryrecall"` → any `MemoryRecall` segment
///   (the store matches on variant + inner id; we synthesise a wildcard
///   via `MemoryRecall("*")` which the `tier_eq` helper in the in-memory
///   store will NOT match — in practice most callers omit this field, so
///   we only narrow when the match is exact).
/// - `"skill:<id>"` → `Skill(<id>)`
/// - `"persona"` / `"soul"` / `"system_prompt"` → matching variants
/// - anything else → `None` (no filter applied)
fn parse_tier_filter(raw: &str) -> Option<SegmentKind> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "soul" => Some(SegmentKind::Soul),
        "system_prompt" | "systemprompt" => Some(SegmentKind::SystemPrompt),
        "persona" => Some(SegmentKind::Persona),
        _ => {
            if let Some(id) = trimmed.strip_prefix("skill:") {
                return Some(SegmentKind::Skill(id.to_string()));
            }
            if let Some(id) = trimmed.strip_prefix("memory_recall:") {
                return Some(SegmentKind::MemoryRecall(id.to_string()));
            }
            if let Some(id) = trimmed.strip_prefix("custom:") {
                return Some(SegmentKind::Custom(id.to_string()));
            }
            None
        }
    }
}

/// Map a `ScoredEntry` to a `MemorySearchToolEntry`, flagging hits that
/// are already promoted into the current turn's `MemoryBlock`.
fn render_entry(
    scored: ScoredEntry,
    active: &std::collections::HashSet<&str>,
) -> MemorySearchToolEntry {
    let already = active.contains(scored.entry.id.as_str());
    let tier = semantic_tier_to_legacy(&scored.entry.tier);
    let id_str = scored.entry.id.as_str().to_string();
    let tags = scored.entry.tags.clone();
    let hit = MemorySearchResult {
        id: TierOneMemoryId::new(id_str),
        content: if already {
            String::new()
        } else {
            scored.entry.content
        },
        score: scored.score as f64,
        tier,
        source: None,
    };
    MemorySearchToolEntry {
        hit,
        tags,
        already_in_context: already,
    }
}

/// Collapse a `SegmentKind` down to the legacy three-variant
/// [`MemoryTier`] enum for the `MemorySearchResult` shape.
fn semantic_tier_to_legacy(kind: &SegmentKind) -> MemoryTier {
    match kind {
        SegmentKind::MemoryRecall(_) => MemoryTier::LongTerm,
        SegmentKind::Skill(_) => MemoryTier::Shared,
        _ => MemoryTier::LongTerm,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use sera_types::{EmbeddingError, EmbeddingHealth};
    use sera_memory::{EvictionPolicy, MemoryId, PutRequest, SemanticEntry, SemanticError, SemanticStats};
    use sera_types::principal::{PrincipalId, PrincipalKind, PrincipalRef};
    use sera_types::tool::{
        AuditHandle, CredentialBag, DefaultAuthzProviderStub, SessionRef, ToolPolicy,
        ToolProfile,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    // ── Mocks ────────────────────────────────────────────────────────────────

    struct FixedEmbedding {
        vector: Vec<f32>,
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
            Err(EmbeddingError::Provider("nope".into()))
        }
        async fn health(&self) -> Result<EmbeddingHealth, EmbeddingError> {
            Err(EmbeddingError::Provider("nope".into()))
        }
    }

    /// Canned store that returns a fixed hit list and records
    /// `touch()` / `query()` calls for assertion.
    struct CannedStore {
        hits: Vec<ScoredEntry>,
        observed_top_k: AtomicUsize,
        observed_tier_filter: Mutex<Option<SegmentKind>>,
        touched: Mutex<Vec<String>>,
    }

    impl CannedStore {
        fn new(hits: Vec<ScoredEntry>) -> Self {
            Self {
                hits,
                observed_top_k: AtomicUsize::new(0),
                observed_tier_filter: Mutex::new(None),
                touched: Mutex::new(Vec::new()),
            }
        }
        fn touches(&self) -> Vec<String> {
            self.touched.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl SemanticMemoryStore for CannedStore {
        async fn put(&self, _req: PutRequest) -> Result<MemoryId, SemanticError> {
            Ok(MemoryId::new("canned"))
        }
        async fn query(&self, q: SemanticQuery) -> Result<Vec<ScoredEntry>, SemanticError> {
            self.observed_top_k.store(q.top_k, Ordering::SeqCst);
            *self.observed_tier_filter.lock().unwrap() = q.tier_filter.clone();
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
        async fn touch(&self, id: &MemoryId) -> Result<(), SemanticError> {
            self.touched.lock().unwrap().push(id.as_str().to_string());
            Ok(())
        }
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    fn hit(id: &str, content: &str, score: f32) -> ScoredEntry {
        ScoredEntry {
            entry: SemanticEntry {
                id: MemoryId::new(id),
                agent_id: "agent-a".to_string(),
                content: content.to_string(),
                embedding: Some(vec![1.0, 0.0, 0.0, 0.0]),
                tier: SegmentKind::MemoryRecall(id.to_string()),
                tags: vec!["tag-a".into()],
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

    fn agent_ctx(agent_id: &str, active: Vec<String>) -> ToolContext {
        ToolContext {
            session: SessionRef::new("test-session"),
            principal: PrincipalRef {
                id: PrincipalId::new(format!("agent:{agent_id}")),
                kind: PrincipalKind::Agent,
            },
            credentials: CredentialBag::new(),
            policy: ToolPolicy::from_profile(ToolProfile::Full),
            audit_handle: AuditHandle {
                trace_id: "t".into(),
                span_id: "s".into(),
            },
            authz: Arc::new(DefaultAuthzProviderStub),
            active_recall_ids: active,
        }
    }

    fn mk_input(args: serde_json::Value) -> ToolInput {
        ToolInput {
            name: TOOL_NAME.to_string(),
            arguments: args,
            call_id: "call-1".to_string(),
        }
    }

    // ── Metadata ─────────────────────────────────────────────────────────────

    #[test]
    fn metadata_reports_read_risk_and_correct_name() {
        let tool = MemorySearchTool::new(
            Arc::new(FixedEmbedding { vector: vec![1.0, 0.0, 0.0, 0.0] }),
            Arc::new(CannedStore::new(vec![])),
        );
        let meta = tool.metadata();
        assert_eq!(meta.name, TOOL_NAME);
        assert_eq!(meta.risk_level, RiskLevel::Read);
        assert_eq!(meta.execution_target, ExecutionTarget::InProcess);
    }

    #[test]
    fn schema_declares_query_required() {
        let tool = MemorySearchTool::new(
            Arc::new(FixedEmbedding { vector: vec![1.0, 0.0, 0.0, 0.0] }),
            Arc::new(CannedStore::new(vec![])),
        );
        let schema = tool.schema();
        assert_eq!(schema.parameters.required, vec!["query".to_string()]);
        assert!(schema.parameters.properties.contains_key("query"));
        assert!(schema.parameters.properties.contains_key("top_k"));
        assert!(schema.parameters.properties.contains_key("tier_filter"));
    }

    // ── Happy path ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn top_k_honored_and_touch_called_per_hit() {
        let store = Arc::new(CannedStore::new(vec![
            hit("m1", "one", 0.9),
            hit("m2", "two", 0.8),
            hit("m3", "three", 0.7),
        ]));
        let tool = MemorySearchTool::new(
            Arc::new(FixedEmbedding { vector: vec![1.0, 0.0, 0.0, 0.0] }),
            store.clone(),
        );

        let out = tool
            .execute(
                mk_input(serde_json::json!({"query": "anything", "top_k": 7})),
                agent_ctx("agent-a", vec![]),
            )
            .await
            .unwrap();

        let parsed: MemorySearchToolResult = serde_json::from_str(&out.content).unwrap();
        assert_eq!(parsed.total, 3);
        assert_eq!(parsed.results.len(), 3);
        assert_eq!(store.observed_top_k.load(Ordering::SeqCst), 7);
        assert_eq!(store.touches(), vec!["m1", "m2", "m3"]);
    }

    #[tokio::test]
    async fn top_k_defaults_and_clamps() {
        let store = Arc::new(CannedStore::new(vec![]));
        let tool = MemorySearchTool::new(
            Arc::new(FixedEmbedding { vector: vec![1.0, 0.0, 0.0, 0.0] }),
            store.clone(),
        );

        // Omitted → default 5
        tool.execute(
            mk_input(serde_json::json!({"query": "x"})),
            agent_ctx("a", vec![]),
        )
        .await
        .unwrap();
        assert_eq!(store.observed_top_k.load(Ordering::SeqCst), DEFAULT_TOP_K);

        // Excessive → clamped to MAX_TOP_K
        tool.execute(
            mk_input(serde_json::json!({"query": "x", "top_k": 9999})),
            agent_ctx("a", vec![]),
        )
        .await
        .unwrap();
        assert_eq!(store.observed_top_k.load(Ordering::SeqCst), MAX_TOP_K);
    }

    #[tokio::test]
    async fn tier_filter_passed_through() {
        let store = Arc::new(CannedStore::new(vec![]));
        let tool = MemorySearchTool::new(
            Arc::new(FixedEmbedding { vector: vec![1.0, 0.0, 0.0, 0.0] }),
            store.clone(),
        );
        tool.execute(
            mk_input(serde_json::json!({"query": "x", "tier_filter": "skill:code"})),
            agent_ctx("a", vec![]),
        )
        .await
        .unwrap();
        let filter = store.observed_tier_filter.lock().unwrap().clone();
        assert!(matches!(filter, Some(SegmentKind::Skill(ref s)) if s == "code"));
    }

    // ── Dedupe ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn dedupe_flags_already_promoted_entries() {
        let store = Arc::new(CannedStore::new(vec![
            hit("m1", "one content", 0.9),
            hit("m2", "two content", 0.8),
        ]));
        let tool = MemorySearchTool::new(
            Arc::new(FixedEmbedding { vector: vec![1.0, 0.0, 0.0, 0.0] }),
            store,
        );

        // Simulate the enricher having already promoted m1 into context.
        let ctx = agent_ctx("a", vec!["m1".to_string()]);
        let out = tool
            .execute(mk_input(serde_json::json!({"query": "q"})), ctx)
            .await
            .unwrap();
        let parsed: MemorySearchToolResult = serde_json::from_str(&out.content).unwrap();

        assert_eq!(parsed.results.len(), 2);
        let m1 = parsed.results.iter().find(|e| e.hit.id.0 == "m1").unwrap();
        let m2 = parsed.results.iter().find(|e| e.hit.id.0 == "m2").unwrap();
        assert!(m1.already_in_context);
        assert!(m1.hit.content.is_empty());
        assert!(!m2.already_in_context);
        assert_eq!(m2.hit.content, "two content");
    }

    /// End-to-end dedupe test: enricher promotes rows → memory_search hits
    /// them again in the same turn → final rendering shows one segment per
    /// unique MemoryId. Simulated by constructing the turn's
    /// `active_recall_ids` from the enricher's segment ids.
    #[tokio::test]
    async fn dedupe_single_segment_per_memory_id_in_same_turn() {
        // Enricher promoted m1 + m3.
        let promoted_by_enricher = vec!["m1".to_string(), "m3".to_string()];

        // memory_search returns m1, m2, m3 — all three present in store.
        let store = Arc::new(CannedStore::new(vec![
            hit("m1", "enricher-already-has-me", 0.9),
            hit("m2", "only-in-search", 0.8),
            hit("m3", "enricher-already-has-me-too", 0.7),
        ]));
        let tool = MemorySearchTool::new(
            Arc::new(FixedEmbedding { vector: vec![1.0, 0.0, 0.0, 0.0] }),
            store,
        );

        let ctx = agent_ctx("a", promoted_by_enricher.clone());
        let out = tool
            .execute(mk_input(serde_json::json!({"query": "q"})), ctx)
            .await
            .unwrap();
        let parsed: MemorySearchToolResult = serde_json::from_str(&out.content).unwrap();

        // Each MemoryId renders exactly once — either as enricher segment
        // (modelled via active_recall_ids) OR as a fresh memory_search
        // hit with full content; never both.
        use std::collections::HashSet;
        let mut seen: HashSet<String> = HashSet::new();
        for entry in &parsed.results {
            assert!(
                seen.insert(entry.hit.id.0.clone()),
                "MemoryId {} rendered twice in one turn",
                entry.hit.id.0
            );
            if promoted_by_enricher.contains(&entry.hit.id.0) {
                assert!(entry.already_in_context);
                assert!(entry.hit.content.is_empty());
            } else {
                assert!(!entry.already_in_context);
                assert!(!entry.hit.content.is_empty());
            }
        }
        assert_eq!(parsed.results.len(), 3);
    }

    // ── Failure modes ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn missing_query_is_invalid_input() {
        let tool = MemorySearchTool::new(
            Arc::new(FixedEmbedding { vector: vec![1.0, 0.0, 0.0, 0.0] }),
            Arc::new(CannedStore::new(vec![])),
        );
        let err = tool
            .execute(
                mk_input(serde_json::json!({})),
                agent_ctx("a", vec![]),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn empty_query_is_invalid_input() {
        let tool = MemorySearchTool::new(
            Arc::new(FixedEmbedding { vector: vec![1.0, 0.0, 0.0, 0.0] }),
            Arc::new(CannedStore::new(vec![])),
        );
        let err = tool
            .execute(
                mk_input(serde_json::json!({"query": "   "})),
                agent_ctx("a", vec![]),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn non_agent_principal_rejected() {
        let tool = MemorySearchTool::new(
            Arc::new(FixedEmbedding { vector: vec![1.0, 0.0, 0.0, 0.0] }),
            Arc::new(CannedStore::new(vec![])),
        );
        // Default ToolContext uses a "system" principal (not agent:*).
        let err = tool
            .execute(
                mk_input(serde_json::json!({"query": "q"})),
                ToolContext::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn embedding_failure_surfaces_as_execution_failed() {
        let tool = MemorySearchTool::new(
            Arc::new(FailingEmbedding),
            Arc::new(CannedStore::new(vec![])),
        );
        let err = tool
            .execute(
                mk_input(serde_json::json!({"query": "q"})),
                agent_ctx("a", vec![]),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed(_)));
    }

    // ── Small unit ───────────────────────────────────────────────────────────

    #[test]
    fn parse_tier_filter_accepts_known_shapes() {
        assert!(matches!(parse_tier_filter("soul"), Some(SegmentKind::Soul)));
        assert!(matches!(
            parse_tier_filter("skill:code"),
            Some(SegmentKind::Skill(ref s)) if s == "code"
        ));
        assert!(parse_tier_filter("").is_none());
        assert!(parse_tier_filter("unknown-variant").is_none());
    }

    #[test]
    fn extract_agent_id_strips_prefix() {
        let ctx = agent_ctx("sera-7bc3", vec![]);
        assert_eq!(extract_agent_id(&ctx).as_deref(), Some("sera-7bc3"));
    }
}
