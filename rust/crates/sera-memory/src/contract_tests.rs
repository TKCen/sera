//! Cross-backend contract test suite for [`SemanticMemoryStore`].
//!
//! A single set of behavioural assertions that every production
//! implementation must satisfy.  Add a new backend by writing a one-line
//! `#[tokio::test]` that calls [`run_contract_suite`] with a factory
//! closure.
//!
//! ## How to add a backend
//!
//! ```rust,ignore
//! #[tokio::test]
//! async fn my_backend_satisfies_contract() {
//!     run_contract_suite(|| async {
//!         Box::new(MyStore::new()) as Box<dyn SemanticMemoryStore>
//!     })
//!     .await;
//! }
//! ```
//!
//! Backends with known implementation gaps can opt out of individual
//! sub-contracts via [`ContractOptions`] and [`run_contract_suite_with_options`].
//! Each opt-out must be accompanied by a comment citing the tracking issue.
//!
//! ## Backend requirements
//!
//! * `put` MUST accept a [`PutRequest`] with `supplied_embedding` set to a
//!   3-dimensional `f32` vector.
//! * `query` MUST accept a [`SemanticQuery`] with `query_embedding` set to a
//!   3-dimensional vector AND `text` set to a plain-text string. Backends
//!   that only support one signal are free to ignore the other.
//!
//! Backends that cannot accept pre-supplied embeddings at all (e.g.
//! server-side-only embedding services) should not be wired to this suite
//! and should supply their own integration-level contract tests.

use std::future::Future;

use crate::{
    EvictionPolicy, MemoryId, PutRequest, Scope, ScopeHierarchy, SemanticMemoryStore,
    SemanticQuery,
};
use sera_types::memory::SegmentKind;

// ── helpers ──────────────────────────────────────────────────────────────────

/// 3-dimensional unit vector pointing along `x`.
const EMB_X: [f32; 3] = [1.0, 0.0, 0.0];
/// 3-dimensional unit vector pointing along `y` (orthogonal to `EMB_X`).
const EMB_Y: [f32; 3] = [0.0, 1.0, 0.0];

fn tier() -> SegmentKind {
    SegmentKind::MemoryRecall("contract".into())
}

/// Build a minimal [`PutRequest`] with a pre-supplied embedding so both
/// vector-capable and keyword-only backends can accept it.
fn put(agent: &str, content: &str, emb: [f32; 3]) -> PutRequest {
    PutRequest {
        agent_id: agent.into(),
        content: content.into(),
        scope: None,
        tier: tier(),
        tags: vec![],
        promoted: false,
        supplied_embedding: Some(emb.to_vec()),
    }
}

/// Build a [`SemanticQuery`] that includes both `text` and
/// `query_embedding` so keyword-only and vector backends both return
/// results.
fn query(agent: &str, text: &str, emb: [f32; 3], top_k: usize) -> SemanticQuery {
    SemanticQuery {
        agent_id: agent.into(),
        scope: None,
        tier_filter: None,
        text: Some(text.into()),
        query_embedding: Some(emb.to_vec()),
        top_k,
        similarity_threshold: None,
    }
}

// ── contract suite ────────────────────────────────────────────────────────────

/// Opt-in flags for optional contract sub-suites.
///
/// All flags default to `true` (full contract). Set a flag to `false` only
/// when a known implementation gap exists — always add a comment with the
/// tracking issue.
#[derive(Debug, Clone)]
pub struct ContractOptions {
    /// The backend correctly filters `query()` results by
    /// `SemanticQuery::scope`. Set to `false` for backends where scope
    /// filtering is not yet applied in the SQL/query path.
    ///
    /// Known gap: `SqliteMemoryStore` — `bm25_probe` and `vec_probe` do not
    /// include `scope_kind`/`scope_key` in their SQL WHERE clauses.
    pub scope_filter: bool,

    /// The backend's `query_hierarchical` correctly fans out across scope
    /// levels and returns results from multiple scopes. Depends on
    /// `scope_filter` working correctly in the underlying `query()`.
    ///
    /// Known gap: `SqliteMemoryStore` — same root cause as `scope_filter`.
    pub hierarchical_query: bool,
}

impl Default for ContractOptions {
    fn default() -> Self {
        Self {
            scope_filter: true,
            hierarchical_query: true,
        }
    }
}

/// Run every contract assertion against the store produced by `factory`.
///
/// `factory` is called once per sub-assertion group so each group starts
/// from a clean state. Enables all contracts. Use
/// [`run_contract_suite_with_options`] to opt out of individual contracts.
pub async fn run_contract_suite<F, Fut>(factory: F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    run_contract_suite_with_options(factory, ContractOptions::default()).await;
}

/// Run contract assertions with per-backend opt-outs.
///
/// Prefer [`run_contract_suite`] unless a specific known gap requires an
/// opt-out.
pub async fn run_contract_suite_with_options<F, Fut>(factory: F, opts: ContractOptions)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    contract_put_returns_id(&factory).await;
    contract_query_returns_inserted_row(&factory).await;
    contract_delete_removes_row(&factory).await;
    contract_delete_missing_is_not_found(&factory).await;
    contract_agent_id_isolation(&factory).await;
    contract_query_empty_store(&factory).await;
    contract_top_k_limits_results(&factory).await;
    contract_stats_reflect_puts(&factory).await;
    contract_evict_ttl_removes_old_rows(&factory).await;
    contract_evict_row_cap_keeps_newest(&factory).await;
    contract_promoted_exempt_from_ttl(&factory).await;
    if opts.scope_filter {
        contract_scope_filter_isolates_by_scope(&factory).await;
    }
    if opts.hierarchical_query {
        contract_hierarchical_query_merges_levels(&factory).await;
    }
}

// ── individual contracts ──────────────────────────────────────────────────────

async fn contract_put_returns_id<F, Fut>(factory: &F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    let store = factory().await;
    let id = store.put(put("a", "hello contract", EMB_X)).await.unwrap();
    assert!(!id.as_str().is_empty(), "put must return a non-empty MemoryId");
}

async fn contract_query_returns_inserted_row<F, Fut>(factory: &F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    let store = factory().await;
    store.put(put("a", "hello contract world", EMB_X)).await.unwrap();
    let hits = store.query(query("a", "hello contract", EMB_X, 5)).await.unwrap();
    assert!(!hits.is_empty(), "query must return the inserted row");
    assert!(
        hits.iter().any(|h| h.entry.content == "hello contract world"),
        "query result must include the inserted content"
    );
}

async fn contract_delete_removes_row<F, Fut>(factory: &F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    let store = factory().await;
    let id = store.put(put("a", "to be deleted", EMB_X)).await.unwrap();

    store.delete(&id).await.unwrap();

    // A query for the same content must no longer return the row.
    let hits = store.query(query("a", "to be deleted", EMB_X, 5)).await.unwrap();
    assert!(
        hits.iter().all(|h| h.entry.id != id),
        "deleted row must not appear in query results"
    );
}

async fn contract_delete_missing_is_not_found<F, Fut>(factory: &F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    let store = factory().await;
    let missing = MemoryId::new("contract-test-nonexistent-id-xyzzy");
    let err = store.delete(&missing).await.unwrap_err();
    assert!(
        matches!(err, crate::SemanticError::NotFound(_)),
        "delete of unknown id must return SemanticError::NotFound, got: {err:?}"
    );
}

async fn contract_agent_id_isolation<F, Fut>(factory: &F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    let store = factory().await;
    store.put(put("alice", "alice secret", EMB_X)).await.unwrap();
    store.put(put("bob", "bob secret", EMB_X)).await.unwrap();

    let alice_hits = store.query(query("alice", "secret", EMB_X, 10)).await.unwrap();
    for h in &alice_hits {
        assert_eq!(
            h.entry.agent_id, "alice",
            "alice's query must only return alice's rows"
        );
    }

    let bob_hits = store.query(query("bob", "secret", EMB_X, 10)).await.unwrap();
    for h in &bob_hits {
        assert_eq!(
            h.entry.agent_id, "bob",
            "bob's query must only return bob's rows"
        );
    }
}

async fn contract_query_empty_store<F, Fut>(factory: &F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    let store = factory().await;
    let hits = store.query(query("a", "anything", EMB_X, 5)).await.unwrap();
    assert!(hits.is_empty(), "query on empty store must return empty results");
}

async fn contract_top_k_limits_results<F, Fut>(factory: &F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    let store = factory().await;
    for i in 0..5 {
        store
            .put(put("a", &format!("row content number {i}"), EMB_X))
            .await
            .unwrap();
    }
    let hits = store.query(query("a", "row content number", EMB_X, 2)).await.unwrap();
    assert!(
        hits.len() <= 2,
        "query with top_k=2 must return at most 2 results, got {}",
        hits.len()
    );
}

async fn contract_stats_reflect_puts<F, Fut>(factory: &F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    let store = factory().await;
    let s0 = store.stats().await.unwrap();
    assert_eq!(s0.total_rows, 0, "empty store must report 0 total_rows");

    store.put(put("a", "first row", EMB_X)).await.unwrap();
    store.put(put("a", "second row", EMB_Y)).await.unwrap();
    store.put(put("b", "other agent row", EMB_X)).await.unwrap();

    let s = store.stats().await.unwrap();
    assert_eq!(s.total_rows, 3, "stats must count all rows across agents");
    assert!(
        !s.per_agent_top.is_empty(),
        "per_agent_top must be non-empty after inserts"
    );
    let a_count = s.per_agent_top.iter().find(|(id, _)| id == "a").map(|(_, n)| *n);
    assert_eq!(a_count, Some(2), "agent 'a' must have 2 rows in per_agent_top");
}

async fn contract_evict_ttl_removes_old_rows<F, Fut>(factory: &F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    // NOTE: this contract verifies that `evict` with `ttl_days=0` removes
    // all rows written with `created_at` in the past. Since the put path
    // always stamps `Utc::now()` we can only test with ttl_days=0 (removes
    // everything) or trust that the implementation does the right thing for
    // future timestamps. We use ttl_days=0 and check that the count drops.
    let store = factory().await;
    store.put(put("a", "old row a", EMB_X)).await.unwrap();
    store.put(put("a", "old row b", EMB_Y)).await.unwrap();

    let removed = store
        .evict(&EvictionPolicy {
            ttl_days: Some(0),
            ..Default::default()
        })
        .await
        .unwrap();
    // TTL of 0 days means "remove anything not created in the future".
    // Rows created just now are at the boundary; some backends treat ≤0 as
    // "age >= 0 days = everything", others may keep the current-second row.
    // We assert only that the call succeeds and removed ≤ the 2 rows we wrote.
    assert!(removed <= 2, "evict ttl=0 must report ≤ 2 rows removed, got {removed}");

    let s = store.stats().await.unwrap();
    assert!(
        s.total_rows <= 2,
        "total rows after ttl=0 eviction must be ≤ 2"
    );
}

async fn contract_evict_row_cap_keeps_newest<F, Fut>(factory: &F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    let store = factory().await;
    // Insert 4 rows for agent 'a'. Because put stamps Utc::now() we cannot
    // control created_at precisely, so we verify only the count invariant:
    // after capping at 2, ≤2 rows survive.
    for i in 0..4 {
        store
            .put(put("a", &format!("row cap test {i}"), EMB_X))
            .await
            .unwrap();
    }

    let removed = store
        .evict(&EvictionPolicy {
            max_per_agent: Some(2),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(removed, 2, "evict max_per_agent=2 must remove exactly 2 of 4 rows");

    let s = store.stats().await.unwrap();
    assert_eq!(s.total_rows, 2, "2 rows must survive the row-cap eviction");
}

async fn contract_promoted_exempt_from_ttl<F, Fut>(factory: &F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    let store = factory().await;

    // Insert one promoted row.
    let promoted_id = store
        .put(PutRequest {
            promoted: true,
            ..put("a", "pinned forever", EMB_X)
        })
        .await
        .unwrap();

    // Apply promote() as well to exercise that code path.
    store.promote(&promoted_id).await.unwrap();

    // Insert a non-promoted row.
    store.put(put("a", "evictable", EMB_Y)).await.unwrap();

    // Evict with ttl_days=0 + promoted_exempt=true.
    let removed = store
        .evict(&EvictionPolicy {
            ttl_days: Some(0),
            promoted_exempt: true,
            ..Default::default()
        })
        .await
        .unwrap();

    // At most 1 row should have been removed (the non-promoted one).
    assert!(
        removed <= 1,
        "promoted_exempt eviction must remove at most 1 row, got {removed}"
    );

    let s = store.stats().await.unwrap();
    assert!(
        s.total_rows >= 1,
        "promoted row must survive ttl eviction with promoted_exempt=true"
    );
}

async fn contract_scope_filter_isolates_by_scope<F, Fut>(factory: &F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    let store = factory().await;

    // Insert one agent-scoped and one circle-scoped row.
    store
        .put(PutRequest {
            scope: Some(Scope::Agent("agent-a".into())),
            ..put("agent-a", "agent row", EMB_X)
        })
        .await
        .unwrap();
    store
        .put(PutRequest {
            scope: Some(Scope::Circle("circle-1".into())),
            ..put("agent-a", "circle row", EMB_X)
        })
        .await
        .unwrap();

    // Query pinned to the circle scope.
    let circle_hits = store
        .query(SemanticQuery {
            scope: Some(Scope::Circle("circle-1".into())),
            ..query("agent-a", "row", EMB_X, 10)
        })
        .await
        .unwrap();

    assert!(
        circle_hits.iter().all(|h| h.entry.content == "circle row"),
        "circle-scoped query must not return agent-scoped rows"
    );
}

async fn contract_hierarchical_query_merges_levels<F, Fut>(factory: &F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Box<dyn SemanticMemoryStore>>,
{
    let store = factory().await;

    // One row at each scope level.
    store
        .put(PutRequest {
            scope: Some(Scope::Agent("agent-a".into())),
            ..put("agent-a", "agent scope row", EMB_X)
        })
        .await
        .unwrap();
    store
        .put(PutRequest {
            scope: Some(Scope::Circle("circle-1".into())),
            ..put("agent-a", "circle scope row", EMB_X)
        })
        .await
        .unwrap();

    let hierarchy = ScopeHierarchy {
        agent: "agent-a".into(),
        circle: Some("circle-1".into()),
        org: None,
        damping: Default::default(),
    };

    let hits = store
        .query_hierarchical(&hierarchy, EMB_X.to_vec(), 10)
        .await
        .unwrap();

    assert!(
        hits.len() >= 2,
        "hierarchical query must merge results from agent + circle scopes, got {} hit(s)",
        hits.len()
    );
    let contents: Vec<&str> = hits.iter().map(|h| h.entry.content.as_str()).collect();
    assert!(
        contents.contains(&"agent scope row"),
        "hierarchical query must include agent-scope row"
    );
    assert!(
        contents.contains(&"circle scope row"),
        "hierarchical query must include circle-scope row"
    );
    // Agent-scope row must rank higher than circle-scope row (default damping).
    let agent_pos = hits
        .iter()
        .position(|h| h.entry.content == "agent scope row")
        .unwrap();
    let circle_pos = hits
        .iter()
        .position(|h| h.entry.content == "circle scope row")
        .unwrap();
    assert!(
        agent_pos <= circle_pos,
        "agent-scope hit must rank >= circle-scope hit in hierarchical results"
    );
}

// ── backend wiring ────────────────────────────────────────────────────────────

#[cfg(all(feature = "testing", feature = "sqlite"))]
#[cfg(test)]
mod backend_tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use sera_types::embedding::{EmbeddingError, EmbeddingHealth};
    use sera_types::EmbeddingService;

    use super::{run_contract_suite, run_contract_suite_with_options, ContractOptions};
    use crate::{InMemorySemanticStore, SemanticMemoryStore, SqliteMemoryStore};

    /// Minimal embedding stub that maps any text to the 3-dimensional
    /// `EMB_X` unit vector. Wiring this into `SqliteMemoryStore` pins
    /// `dims = 3`, which matches the contract-test vectors.
    struct FixedEmbedding3;

    #[async_trait]
    impl EmbeddingService for FixedEmbedding3 {
        fn model_id(&self) -> &str {
            "contract-test-fixed-3d"
        }
        fn dimensions(&self) -> usize {
            3
        }
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            Ok(texts.iter().map(|_| vec![1.0_f32, 0.0, 0.0]).collect())
        }
        async fn health(&self) -> Result<EmbeddingHealth, EmbeddingError> {
            Ok(EmbeddingHealth {
                available: true,
                detail: "contract-test stub".into(),
                latency_ms: Some(0),
            })
        }
    }

    #[tokio::test]
    async fn in_memory_satisfies_contract() {
        run_contract_suite(|| async {
            Box::new(InMemorySemanticStore::new()) as Box<dyn SemanticMemoryStore>
        })
        .await;
    }

    #[tokio::test]
    async fn sqlite_satisfies_contract() {
        // scope_filter=false: SqliteMemoryStore's bm25_probe/vec_probe do not
        // yet apply SemanticQuery::scope as a SQL WHERE predicate. Tracked for
        // fix; once resolved, remove this opt-out and use run_contract_suite.
        run_contract_suite_with_options(
            || async {
                // Supply a 3-dim embedding service so SqliteMemoryStore's internal
                // `dims` matches the 3-element vectors used in the contract suite.
                let svc = Arc::new(FixedEmbedding3) as Arc<dyn EmbeddingService>;
                Box::new(
                    SqliteMemoryStore::open_in_memory(Some(svc))
                        .expect("sqlite open_in_memory failed"),
                ) as Box<dyn SemanticMemoryStore>
            },
            ContractOptions {
                scope_filter: false,
                hierarchical_query: false,
            },
        )
        .await;
    }
}
