//! In-memory [`SemanticMemoryStore`] for tests and pgvector-free dev
//! deployments.
//!
//! Backed by an `Arc<Mutex<HashMap<MemoryId, SemanticEntry>>>`; queries do
//! a linear cosine-similarity scan over the map. Not meant for production
//! — for that path see `sera-db::pgvector_store::PgVectorStore`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sera_types::{
    EvictionPolicy, MemoryId, ScoredEntry, Scope, SemanticEntry, SemanticError,
    SemanticMemoryStore, SemanticQuery, SemanticStats, memory::SegmentKind,
};
use uuid::Uuid;

/// In-process, [`SemanticMemoryStore`]-conforming fake.
#[derive(Clone, Debug)]
pub struct InMemorySemanticStore {
    inner: Arc<Mutex<State>>,
    dimensions: usize,
}

#[derive(Debug, Default)]
struct State {
    rows: HashMap<MemoryId, SemanticEntry>,
}

impl InMemorySemanticStore {
    /// Create a store that accepts any vector dimensionality (the first
    /// [`put`] pins the dimension for the lifetime of the instance).
    pub fn new() -> Self {
        Self::with_dimensions(0)
    }

    /// Create a store with a fixed dimensionality. A value of `0` means
    /// "infer from first write".
    pub fn with_dimensions(dimensions: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(State::default())),
            dimensions,
        }
    }

    /// Configured embedding dimensionality. `0` means "not yet fixed".
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Borrow a snapshot of the current row count (for tests).
    pub fn len(&self) -> usize {
        self.inner.lock().expect("poisoned").rows.len()
    }

    /// `true` iff the store holds no rows.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn dims(&self) -> usize {
        self.dimensions
    }
}

impl Default for InMemorySemanticStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Cosine similarity in `[-1, 1]`; returns `0.0` for zero-magnitude
/// vectors instead of `NaN`.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
}

fn recency_norm(created_at: DateTime<Utc>, now: DateTime<Utc>) -> f32 {
    const HALF_LIFE_DAYS: f32 = 14.0;
    let age_days = (now - created_at).num_seconds().max(0) as f32 / 86_400.0;
    (1.0 - age_days / HALF_LIFE_DAYS).clamp(0.0, 1.0)
}

#[async_trait]
impl SemanticMemoryStore for InMemorySemanticStore {
    async fn put(&self, mut entry: SemanticEntry) -> Result<MemoryId, SemanticError> {
        if self.dims() != 0 && entry.embedding.len() != self.dims() {
            return Err(SemanticError::DimensionMismatch {
                expected: self.dims(),
                got: entry.embedding.len(),
            });
        }

        if entry.id.as_str().is_empty() {
            entry.id = MemoryId::new(Uuid::new_v4().to_string());
        }

        let id = entry.id.clone();
        let mut guard = self.inner.lock().expect("poisoned");
        guard.rows.insert(id.clone(), entry);
        Ok(id)
    }

    async fn query(&self, query: SemanticQuery) -> Result<Vec<ScoredEntry>, SemanticError> {
        let probe = query.query_embedding.ok_or_else(|| {
            SemanticError::Backend(
                "InMemorySemanticStore::query requires query.query_embedding".into(),
            )
        })?;
        let guard = self.inner.lock().expect("poisoned");
        let now = Utc::now();

        let matches_tier = |entry: &SemanticEntry| -> bool {
            query
                .tier_filter
                .as_ref()
                .map(|t| tier_eq(&entry.tier, t))
                .unwrap_or(true)
        };

        // GH#140: when the caller pins a `Scope`, filter on the row's
        // effective scope (defaulting to `Scope::Agent(agent_id)` for
        // back-compat rows that were written without `scope`).
        let matches_scope = |entry: &SemanticEntry| -> bool {
            match &query.scope {
                None => entry.agent_id == query.agent_id,
                Some(target) => {
                    let effective = entry
                        .scope
                        .clone()
                        .unwrap_or_else(|| Scope::Agent(entry.agent_id.clone()));
                    &effective == target
                }
            }
        };

        let mut scored: Vec<ScoredEntry> = guard
            .rows
            .values()
            .filter(|e| matches_scope(e) && matches_tier(e))
            .map(|entry| {
                let vs = cosine(&entry.embedding, &probe);
                let rs = recency_norm(entry.created_at, now);
                ScoredEntry {
                    entry: entry.clone(),
                    score: vs,
                    index_score: 0.0,
                    vector_score: vs,
                    recency_score: rs,
                }
            })
            .collect();

        if let Some(threshold) = query.similarity_threshold {
            scored.retain(|s| s.score >= threshold);
        }

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.entry.created_at.cmp(&a.entry.created_at))
        });

        let top_k = query.top_k.max(1);
        scored.truncate(top_k);
        Ok(scored)
    }

    async fn delete(&self, id: &MemoryId) -> Result<(), SemanticError> {
        let mut guard = self.inner.lock().expect("poisoned");
        if guard.rows.remove(id).is_none() {
            return Err(SemanticError::NotFound(id.clone()));
        }
        Ok(())
    }

    async fn evict(&self, policy: &EvictionPolicy) -> Result<usize, SemanticError> {
        let mut guard = self.inner.lock().expect("poisoned");
        let mut removed = 0usize;

        if let Some(ttl) = policy.ttl_days {
            let cutoff = Utc::now() - chrono::Duration::days(ttl as i64);
            let before = guard.rows.len();
            guard.rows.retain(|_, e| {
                if e.created_at >= cutoff {
                    return true;
                }
                if policy.promoted_exempt && e.promoted {
                    return true;
                }
                false
            });
            removed += before - guard.rows.len();
        }

        if let Some(cap) = policy.max_per_agent {
            // Group indices by agent, newest first, keep first `cap`.
            let mut by_agent: HashMap<String, Vec<MemoryId>> = HashMap::new();
            let mut order_map: HashMap<MemoryId, DateTime<Utc>> = HashMap::new();
            for (id, e) in guard.rows.iter() {
                order_map.insert(id.clone(), e.created_at);
                by_agent
                    .entry(e.agent_id.clone())
                    .or_default()
                    .push(id.clone());
            }
            let mut to_remove: Vec<MemoryId> = Vec::new();
            for (_agent, mut ids) in by_agent {
                ids.sort_by(|a, b| {
                    order_map
                        .get(b)
                        .copied()
                        .unwrap_or_else(Utc::now)
                        .cmp(&order_map.get(a).copied().unwrap_or_else(Utc::now))
                });
                for id in ids.into_iter().skip(cap) {
                    if policy.promoted_exempt
                        && let Some(e) = guard.rows.get(&id)
                        && e.promoted
                    {
                        continue;
                    }
                    to_remove.push(id);
                }
            }
            for id in to_remove {
                if guard.rows.remove(&id).is_some() {
                    removed += 1;
                }
            }
        }

        Ok(removed)
    }

    async fn promote(&self, id: &MemoryId) -> Result<(), SemanticError> {
        let mut guard = self.inner.lock().expect("poisoned");
        match guard.rows.get_mut(id) {
            Some(row) => {
                row.promoted = true;
                Ok(())
            }
            None => Err(SemanticError::NotFound(id.clone())),
        }
    }

    async fn touch(&self, id: &MemoryId) -> Result<(), SemanticError> {
        let mut guard = self.inner.lock().expect("poisoned");
        if let Some(row) = guard.rows.get_mut(id) {
            row.last_accessed_at = Some(Utc::now());
        }
        Ok(())
    }

    async fn stats(&self) -> Result<SemanticStats, SemanticError> {
        let guard = self.inner.lock().expect("poisoned");
        let total_rows = guard.rows.len();
        let mut per_agent: HashMap<String, usize> = HashMap::new();
        let mut oldest = None;
        let mut newest = None;
        for e in guard.rows.values() {
            *per_agent.entry(e.agent_id.clone()).or_insert(0) += 1;
            oldest = Some(match oldest {
                None => e.created_at,
                Some(o) if e.created_at < o => e.created_at,
                Some(o) => o,
            });
            newest = Some(match newest {
                None => e.created_at,
                Some(n) if e.created_at > n => e.created_at,
                Some(n) => n,
            });
        }
        let mut per_agent_top: Vec<(String, usize)> = per_agent.into_iter().collect();
        per_agent_top.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        per_agent_top.truncate(16);

        let epoch = DateTime::<Utc>::from_timestamp(0, 0).unwrap_or_else(Utc::now);
        Ok(SemanticStats {
            total_rows,
            per_agent_top,
            oldest: oldest.unwrap_or(epoch),
            newest: newest.unwrap_or(epoch),
        })
    }
}

fn tier_eq(a: &SegmentKind, b: &SegmentKind) -> bool {
    use SegmentKind::*;
    match (a, b) {
        (Soul, Soul) => true,
        (SystemPrompt, SystemPrompt) => true,
        (Persona, Persona) => true,
        (Skill(x), Skill(y)) => x == y,
        (MemoryRecall(x), MemoryRecall(y)) => x == y,
        (Custom(x), Custom(y)) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_entry(agent: &str, content: &str, emb: Vec<f32>) -> SemanticEntry {
        SemanticEntry {
            id: MemoryId::new(""),
            agent_id: agent.into(),
            content: content.into(),
            embedding: emb,
            tier: SegmentKind::MemoryRecall("r".into()),
            tags: vec![],
            created_at: Utc::now(),
            last_accessed_at: None,
            promoted: false,
            scope: None,
        }
    }

    #[tokio::test]
    async fn put_query_delete_roundtrip() {
        let store = InMemorySemanticStore::new();
        let id = store
            .put(mk_entry("a", "hello world", vec![1.0, 0.0, 0.0]))
            .await
            .unwrap();
        assert_eq!(store.len(), 1);

        let q = SemanticQuery {
            agent_id: "a".into(),
            tier_filter: None,
            text: None,
            query_embedding: Some(vec![1.0, 0.0, 0.0]),
            top_k: 5,
            similarity_threshold: None,
            scope: None,
        };
        let hits = store.query(q).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.content, "hello world");
        assert!((hits[0].vector_score - 1.0).abs() < 1e-5);

        store.delete(&id).await.unwrap();
        assert!(store.is_empty());
    }

    #[tokio::test]
    async fn delete_nonexistent_is_not_found() {
        let store = InMemorySemanticStore::new();
        let err = store
            .delete(&MemoryId::new(Uuid::new_v4().to_string()))
            .await
            .unwrap_err();
        assert!(matches!(err, SemanticError::NotFound(_)));
    }

    #[tokio::test]
    async fn agent_id_isolation() {
        let store = InMemorySemanticStore::new();
        store
            .put(mk_entry("alice", "secret a", vec![1.0, 0.0]))
            .await
            .unwrap();
        store
            .put(mk_entry("bob", "secret b", vec![1.0, 0.0]))
            .await
            .unwrap();

        let q = SemanticQuery {
            agent_id: "alice".into(),
            tier_filter: None,
            text: None,
            query_embedding: Some(vec![1.0, 0.0]),
            top_k: 5,
            similarity_threshold: None,
            scope: None,
        };
        let hits = store.query(q).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.content, "secret a");
        assert_eq!(hits[0].entry.agent_id, "alice");
    }

    #[tokio::test]
    async fn similarity_threshold_filters() {
        let store = InMemorySemanticStore::new();
        store
            .put(mk_entry("a", "same", vec![1.0, 0.0]))
            .await
            .unwrap();
        store
            .put(mk_entry("a", "orthogonal", vec![0.0, 1.0]))
            .await
            .unwrap();

        let q = SemanticQuery {
            agent_id: "a".into(),
            tier_filter: None,
            text: None,
            query_embedding: Some(vec![1.0, 0.0]),
            top_k: 10,
            similarity_threshold: Some(0.5),
            scope: None,
        };
        let hits = store.query(q).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.content, "same");
    }

    #[tokio::test]
    async fn dimension_mismatch_rejected_when_fixed() {
        let store = InMemorySemanticStore::with_dimensions(3);
        let err = store
            .put(mk_entry("a", "bad", vec![1.0, 2.0]))
            .await
            .unwrap_err();
        assert!(matches!(err, SemanticError::DimensionMismatch { expected: 3, got: 2 }));
    }

    #[tokio::test]
    async fn ttl_eviction_removes_old_rows() {
        let store = InMemorySemanticStore::new();
        let mut old = mk_entry("a", "old", vec![1.0]);
        old.created_at = Utc::now() - chrono::Duration::days(10);
        let fresh = mk_entry("a", "fresh", vec![1.0]);
        store.put(old).await.unwrap();
        store.put(fresh).await.unwrap();

        let removed = store
            .evict(&EvictionPolicy {
                ttl_days: Some(5),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.len(), 1);
    }

    #[tokio::test]
    async fn row_cap_eviction_keeps_newest() {
        let store = InMemorySemanticStore::new();
        for i in 0..5 {
            let mut e = mk_entry("a", &format!("row-{i}"), vec![1.0]);
            e.created_at = Utc::now() - chrono::Duration::seconds((5 - i) as i64);
            store.put(e).await.unwrap();
        }

        let removed = store
            .evict(&EvictionPolicy {
                max_per_agent: Some(2),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(removed, 3);
        assert_eq!(store.len(), 2);
    }

    #[tokio::test]
    async fn promoted_exempt_from_ttl() {
        let store = InMemorySemanticStore::new();
        let mut old = mk_entry("a", "pinned", vec![1.0]);
        old.created_at = Utc::now() - chrono::Duration::days(30);
        old.promoted = true;
        store.put(old).await.unwrap();

        let removed = store
            .evict(&EvictionPolicy {
                ttl_days: Some(5),
                promoted_exempt: true,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(removed, 0);
        assert_eq!(store.len(), 1);
    }

    #[tokio::test]
    async fn stats_reports_totals_and_per_agent() {
        let store = InMemorySemanticStore::new();
        store
            .put(mk_entry("alice", "1", vec![1.0]))
            .await
            .unwrap();
        store
            .put(mk_entry("alice", "2", vec![1.0]))
            .await
            .unwrap();
        store
            .put(mk_entry("bob", "1", vec![1.0]))
            .await
            .unwrap();

        let s = store.stats().await.unwrap();
        assert_eq!(s.total_rows, 3);
        assert_eq!(s.per_agent_top.len(), 2);
        assert_eq!(s.per_agent_top[0], ("alice".into(), 2));
    }

    #[tokio::test]
    async fn tier_filter_narrows_results() {
        let store = InMemorySemanticStore::new();
        let mut e1 = mk_entry("a", "recall-a", vec![1.0, 0.0]);
        e1.tier = SegmentKind::MemoryRecall("x".into());
        let mut e2 = mk_entry("a", "skill-a", vec![1.0, 0.0]);
        e2.tier = SegmentKind::Skill("code".into());
        store.put(e1).await.unwrap();
        store.put(e2).await.unwrap();

        let q = SemanticQuery {
            agent_id: "a".into(),
            tier_filter: Some(SegmentKind::Skill("code".into())),
            text: None,
            query_embedding: Some(vec![1.0, 0.0]),
            top_k: 10,
            similarity_threshold: None,
            scope: None,
        };
        let hits = store.query(q).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.content, "skill-a");
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine(&a, &b)).abs() < 1e-6);
    }

    #[test]
    fn cosine_identical_is_one() {
        let a = vec![1.0, 2.0, 3.0];
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-6);
    }

    // ── GH#140: hierarchical scope tests ──────────────────────────────────

    use sera_types::{Damping, Scope, ScopeHierarchy};

    fn mk_scoped(
        agent: &str,
        content: &str,
        emb: Vec<f32>,
        scope: Option<Scope>,
    ) -> SemanticEntry {
        SemanticEntry {
            id: MemoryId::new(""),
            agent_id: agent.into(),
            content: content.into(),
            embedding: emb,
            tier: SegmentKind::MemoryRecall("r".into()),
            tags: vec![],
            created_at: Utc::now(),
            last_accessed_at: None,
            promoted: false,
            scope,
        }
    }

    #[tokio::test]
    async fn scope_filter_matches_single_scope_exactly() {
        let store = InMemorySemanticStore::new();
        store
            .put(mk_scoped(
                "a",
                "agent-row",
                vec![1.0, 0.0],
                Some(Scope::Agent("a".into())),
            ))
            .await
            .unwrap();
        store
            .put(mk_scoped(
                "a",
                "circle-row",
                vec![1.0, 0.0],
                Some(Scope::Circle("ring".into())),
            ))
            .await
            .unwrap();

        let q = SemanticQuery {
            agent_id: "a".into(),
            scope: Some(Scope::Circle("ring".into())),
            tier_filter: None,
            text: None,
            query_embedding: Some(vec![1.0, 0.0]),
            top_k: 10,
            similarity_threshold: None,
        };
        let hits = store.query(q).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.content, "circle-row");
    }

    #[tokio::test]
    async fn scope_filter_treats_missing_scope_as_agent() {
        // Pre-migration row: no scope set. Expect it to be visible as
        // Scope::Agent(agent_id).
        let store = InMemorySemanticStore::new();
        store
            .put(mk_scoped("alice", "legacy", vec![1.0, 0.0], None))
            .await
            .unwrap();

        let q = SemanticQuery {
            agent_id: "alice".into(),
            scope: Some(Scope::Agent("alice".into())),
            tier_filter: None,
            text: None,
            query_embedding: Some(vec![1.0, 0.0]),
            top_k: 10,
            similarity_threshold: None,
        };
        let hits = store.query(q).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.content, "legacy");
    }

    #[tokio::test]
    async fn hierarchical_query_merges_across_levels() {
        let store = InMemorySemanticStore::new();
        store
            .put(mk_scoped(
                "a",
                "agent-hit",
                vec![1.0, 0.0],
                Some(Scope::Agent("a".into())),
            ))
            .await
            .unwrap();
        store
            .put(mk_scoped(
                "a",
                "circle-hit",
                vec![1.0, 0.0],
                Some(Scope::Circle("ring".into())),
            ))
            .await
            .unwrap();
        store
            .put(mk_scoped(
                "a",
                "org-hit",
                vec![1.0, 0.0],
                Some(Scope::Org("acme".into())),
            ))
            .await
            .unwrap();
        store
            .put(mk_scoped(
                "a",
                "global-hit",
                vec![1.0, 0.0],
                Some(Scope::Global),
            ))
            .await
            .unwrap();

        let hierarchy = ScopeHierarchy {
            agent: "a".into(),
            circle: Some("ring".into()),
            org: Some("acme".into()),
            damping: Damping::default(),
        };

        let hits = store
            .query_hierarchical(&hierarchy, vec![1.0, 0.0], 10)
            .await
            .unwrap();

        // All four scopes contribute one row.
        assert_eq!(hits.len(), 4);
        let contents: Vec<&str> = hits.iter().map(|h| h.entry.content.as_str()).collect();
        assert!(contents.contains(&"agent-hit"));
        assert!(contents.contains(&"circle-hit"));
        assert!(contents.contains(&"org-hit"));
        assert!(contents.contains(&"global-hit"));
    }

    #[tokio::test]
    async fn hierarchical_damping_orders_agent_above_global() {
        let store = InMemorySemanticStore::new();
        // All hits have identical raw cosine similarity (1.0) — the only
        // tiebreaker is the per-level damping factor.
        store
            .put(mk_scoped(
                "a",
                "agent-row",
                vec![1.0, 0.0],
                Some(Scope::Agent("a".into())),
            ))
            .await
            .unwrap();
        store
            .put(mk_scoped(
                "a",
                "circle-row",
                vec![1.0, 0.0],
                Some(Scope::Circle("ring".into())),
            ))
            .await
            .unwrap();
        store
            .put(mk_scoped(
                "a",
                "org-row",
                vec![1.0, 0.0],
                Some(Scope::Org("acme".into())),
            ))
            .await
            .unwrap();
        store
            .put(mk_scoped(
                "a",
                "global-row",
                vec![1.0, 0.0],
                Some(Scope::Global),
            ))
            .await
            .unwrap();

        let hierarchy = ScopeHierarchy {
            agent: "a".into(),
            circle: Some("ring".into()),
            org: Some("acme".into()),
            damping: Damping::default(),
        };

        let hits = store
            .query_hierarchical(&hierarchy, vec![1.0, 0.0], 10)
            .await
            .unwrap();

        // Expect agent > circle > org > global by dampened score.
        assert_eq!(hits.len(), 4);
        assert_eq!(hits[0].entry.content, "agent-row");
        assert_eq!(hits[1].entry.content, "circle-row");
        assert_eq!(hits[2].entry.content, "org-row");
        assert_eq!(hits[3].entry.content, "global-row");
        assert!(hits[0].dampened_score > hits[1].dampened_score);
        assert!(hits[1].dampened_score > hits[2].dampened_score);
        assert!(hits[2].dampened_score > hits[3].dampened_score);
    }

    #[tokio::test]
    async fn hierarchical_tiebreak_by_recency() {
        let store = InMemorySemanticStore::new();
        // Two entries in the same scope with identical vector score —
        // the newer one must win the sort tiebreak.
        let mut older = mk_scoped(
            "a",
            "older",
            vec![1.0, 0.0],
            Some(Scope::Agent("a".into())),
        );
        older.created_at = Utc::now() - chrono::Duration::seconds(120);
        let newer = mk_scoped(
            "a",
            "newer",
            vec![1.0, 0.0],
            Some(Scope::Agent("a".into())),
        );
        store.put(older).await.unwrap();
        store.put(newer).await.unwrap();

        let hierarchy = ScopeHierarchy::agent_only("a");
        let hits = store
            .query_hierarchical(&hierarchy, vec![1.0, 0.0], 10)
            .await
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].entry.content, "newer");
    }

    #[tokio::test]
    async fn agent_only_hierarchy_still_checks_global() {
        // `ScopeHierarchy::agent_only` always appends `Scope::Global`.
        // Rows in Circle / Org scope should be invisible.
        let store = InMemorySemanticStore::new();
        store
            .put(mk_scoped(
                "a",
                "agent-row",
                vec![1.0, 0.0],
                Some(Scope::Agent("a".into())),
            ))
            .await
            .unwrap();
        store
            .put(mk_scoped(
                "a",
                "circle-row",
                vec![1.0, 0.0],
                Some(Scope::Circle("ring".into())),
            ))
            .await
            .unwrap();
        store
            .put(mk_scoped(
                "a",
                "global-row",
                vec![1.0, 0.0],
                Some(Scope::Global),
            ))
            .await
            .unwrap();

        let hierarchy = ScopeHierarchy::agent_only("a");
        let hits = store
            .query_hierarchical(&hierarchy, vec![1.0, 0.0], 10)
            .await
            .unwrap();

        let contents: Vec<&str> = hits.iter().map(|h| h.entry.content.as_str()).collect();
        assert!(contents.contains(&"agent-row"));
        assert!(contents.contains(&"global-row"));
        assert!(
            !contents.contains(&"circle-row"),
            "circle row must not leak into agent-only hierarchy"
        );
    }

    #[tokio::test]
    async fn non_hierarchical_query_preserves_backcompat() {
        // Flag-off regression — without `scope`, the query must hit every
        // row for the agent regardless of scope_kind.
        let store = InMemorySemanticStore::new();
        store
            .put(mk_scoped(
                "a",
                "agent-row",
                vec![1.0, 0.0],
                Some(Scope::Agent("a".into())),
            ))
            .await
            .unwrap();
        store
            .put(mk_scoped(
                "a",
                "circle-row",
                vec![1.0, 0.0],
                Some(Scope::Circle("ring".into())),
            ))
            .await
            .unwrap();
        store
            .put(mk_scoped(
                "a",
                "legacy-row",
                vec![1.0, 0.0],
                None,
            ))
            .await
            .unwrap();

        let q = SemanticQuery {
            agent_id: "a".into(),
            scope: None,
            tier_filter: None,
            text: None,
            query_embedding: Some(vec![1.0, 0.0]),
            top_k: 10,
            similarity_threshold: None,
        };
        // Expect 2: agent-row and legacy-row (both map to agent_id=a).
        // circle-row belongs to a circle, not agent a.
        let hits = store.query(q).await.unwrap();
        let contents: Vec<&str> = hits.iter().map(|h| h.entry.content.as_str()).collect();
        assert!(contents.contains(&"agent-row"));
        assert!(contents.contains(&"legacy-row"));
        // The circle row belongs to `a` by agent_id but has Scope::Circle;
        // pre-scope behaviour keyed on agent_id only, so it stays visible
        // under the back-compat path.
        assert!(contents.contains(&"circle-row"));
    }
}
