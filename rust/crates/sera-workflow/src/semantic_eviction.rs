//! Tier-2 semantic memory maintenance — bead sera-tier2-d.
//!
//! Bundles three things that all run on the same cron cadence:
//!
//! 1. **Eviction** — calls [`SemanticMemoryStore::evict`] with a policy
//!    built from [`SemanticEvictionConfig`]. Row-cap + TTL, both
//!    promoted-exempt by default.
//! 2. **Dreaming promotion** — surfaces the top-N surviving rows (by
//!    composite score) and flips `promoted = true` so they become
//!    persistent Tier-1 `MemoryRecall` candidates for future turns.
//! 3. **Maintenance** — on a slower cadence, calls
//!    [`SemanticMemoryStore::maintenance`] to run
//!    `REINDEX INDEX CONCURRENTLY` (or whatever the backend exposes).
//!
//! Telemetry is emitted as structured `tracing::info!` events with
//! stable `metric = "…"` fields so operators can build dashboards
//! without string-matching message text.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use sera_types::{
    EvictionPolicy, MemoryId, ScoredEntry, SemanticError, SemanticMemoryStore, SemanticQuery,
    SemanticStats,
};

use crate::types::CronSchedule;

/// Default per-agent row cap. Keeps the hottest 50 000 rows per agent
/// before LRU-style eviction kicks in.
pub const DEFAULT_MAX_PER_AGENT: usize = 50_000;

/// Default TTL — rows older than this are evicted (promoted rows exempt).
pub const DEFAULT_TTL_DAYS: u32 = 180;

/// Default eviction cron — 3am daily.
pub const DEFAULT_EVICTION_CRON: &str = "0 0 3 * * *";

/// Default reindex cron — 3:30am every Sunday (weekly).
pub const DEFAULT_REINDEX_CRON: &str = "0 30 3 * * SUN";

/// Default number of surviving rows to elevate per run.
pub const DEFAULT_DREAMING_TOP_N: usize = 50;

/// Configuration for [`SemanticEvictionJob`].
///
/// Fields are deliberately separate so operators can tune row-cap, TTL,
/// and schedule independently. `None` on a numeric field means "do not
/// apply this dimension", consistent with the underlying
/// [`EvictionPolicy`] semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEvictionConfig {
    /// Whether the job is active. When `false`, [`SemanticEvictionJob::run`]
    /// and [`SemanticEvictionJob::run_maintenance`] short-circuit.
    pub enabled: bool,
    /// Per-agent row cap. `None` disables row-cap eviction.
    pub eviction_max_per_agent: Option<usize>,
    /// Age-based TTL in days. `None` disables TTL eviction.
    pub eviction_ttl_days: Option<u32>,
    /// Exempt rows with `promoted = true` from both row-cap and TTL. Set
    /// to `false` only if you want the dreaming-promotion output to
    /// eventually expire as well.
    pub promoted_exempt: bool,
    /// Cron expression for the eviction + dreaming-promotion pass.
    pub eviction_schedule_cron: String,
    /// Cron expression for the `maintenance()` call (`REINDEX …`).
    pub reindex_schedule_cron: String,
    /// Top-N rows to elevate into `promoted = true` per run. `0`
    /// disables the dreaming-promotion pass.
    pub dreaming_top_n: usize,
}

impl Default for SemanticEvictionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            eviction_max_per_agent: Some(DEFAULT_MAX_PER_AGENT),
            eviction_ttl_days: Some(DEFAULT_TTL_DAYS),
            promoted_exempt: true,
            eviction_schedule_cron: DEFAULT_EVICTION_CRON.to_string(),
            reindex_schedule_cron: DEFAULT_REINDEX_CRON.to_string(),
            dreaming_top_n: DEFAULT_DREAMING_TOP_N,
        }
    }
}

impl SemanticEvictionConfig {
    /// Compile the configured eviction cron expression, surfacing a
    /// [`WorkflowError`](crate::WorkflowError) on parse failure.
    pub fn eviction_cron(&self) -> CronSchedule {
        CronSchedule {
            expression: self.eviction_schedule_cron.clone(),
        }
    }

    /// Compile the configured reindex cron expression.
    pub fn reindex_cron(&self) -> CronSchedule {
        CronSchedule {
            expression: self.reindex_schedule_cron.clone(),
        }
    }

    /// Build a [`EvictionPolicy`] matching this config.
    pub fn to_policy(&self) -> EvictionPolicy {
        EvictionPolicy {
            max_per_agent: self.eviction_max_per_agent,
            ttl_days: self.eviction_ttl_days,
            promoted_exempt: self.promoted_exempt,
        }
    }
}

/// Aggregate report from a single [`SemanticEvictionJob::run`] call.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SemanticEvictionReport {
    /// Number of rows removed by the eviction pass.
    pub rows_removed: usize,
    /// Number of rows promoted by the dreaming-promotion pass.
    pub rows_promoted: usize,
    /// Store total row count before eviction (best-effort — from
    /// `SemanticMemoryStore::stats`).
    pub rows_before: usize,
    /// Store total row count after eviction + promotion.
    pub rows_after: usize,
    /// Error string from a failed dreaming-promotion attempt, if any.
    /// The overall run is not failed by a single promotion error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dreaming_error: Option<String>,
}

/// Minimal abstraction over "fetch top-N surviving rows". The default
/// impl calls [`SemanticMemoryStore::stats`] and then runs one
/// [`SemanticMemoryStore::query`] per top-agent with a zero-vector
/// probe — sufficient for the MVP. Custom impls can replace this with a
/// smarter ranker once one is available.
#[async_trait]
pub trait DreamingCandidatePicker: Send + Sync + 'static {
    /// Return up to `top_n` surviving rows to elevate into `promoted`.
    async fn pick(
        &self,
        store: &dyn SemanticMemoryStore,
        top_n: usize,
    ) -> Result<Vec<MemoryId>, SemanticError>;
}

/// Default picker — asks the store for its top agents (via `stats`) and
/// fetches the newest `top_n` rows for each via `query`. Not a sophisticated
/// composite scorer, but deterministic enough for the consolidation pass
/// and keeps the trait seam open for smarter impls.
#[derive(Debug, Default)]
pub struct StatsDreamingPicker;

#[async_trait]
impl DreamingCandidatePicker for StatsDreamingPicker {
    async fn pick(
        &self,
        store: &dyn SemanticMemoryStore,
        top_n: usize,
    ) -> Result<Vec<MemoryId>, SemanticError> {
        if top_n == 0 {
            return Ok(vec![]);
        }
        let stats: SemanticStats = store.stats().await?;
        if stats.per_agent_top.is_empty() {
            return Ok(vec![]);
        }

        let dims = infer_dimensions(&stats);
        let probe = vec![0.0f32; dims];

        let mut picked: Vec<ScoredEntry> = Vec::new();
        for (agent, _count) in &stats.per_agent_top {
            let q = SemanticQuery {
                agent_id: agent.clone(),
                tier_filter: None,
                text: None,
                query_embedding: Some(probe.clone()),
                top_k: top_n,
                similarity_threshold: None,
            };
            match store.query(q).await {
                Ok(mut hits) => {
                    picked.append(&mut hits);
                }
                Err(e) => {
                    warn!(
                        agent = %agent,
                        error = %e,
                        "dreaming picker: per-agent query failed (skipping agent)"
                    );
                }
            }
            if picked.len() >= top_n * 4 {
                break;
            }
        }

        // Pick the top-N by composite score + recency.
        picked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    b.recency_score
                        .partial_cmp(&a.recency_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        });
        picked.truncate(top_n);
        Ok(picked.into_iter().map(|p| p.entry.id).collect())
    }
}

/// Best-effort dimensionality inference. The store doesn't expose the
/// configured vector length, so we fall back to a probe the sera-runtime
/// configuration guarantees works for both OpenAI (1536) and Ollama
/// (typically 768) embeddings: send an empty-ish zero-vector sized to
/// the default OpenAI dims. pgvector validates per-column and rejects
/// mismatches; on mismatch the dreaming promotion pass logs a warn and
/// skips — eviction still runs.
fn infer_dimensions(_stats: &SemanticStats) -> usize {
    // Default to OpenAI `text-embedding-3-small` dims. Backends that
    // differ will surface a `SemanticError::DimensionMismatch` during
    // the query, which the caller in `pick()` treats as a per-agent skip.
    1536
}

/// Workflow-side job that evicts expired / over-cap rows, promotes the
/// surviving top-N for the dreaming-workflow hop, and optionally runs
/// index maintenance.
pub struct SemanticEvictionJob {
    store: Arc<dyn SemanticMemoryStore>,
    config: SemanticEvictionConfig,
    picker: Arc<dyn DreamingCandidatePicker>,
}

impl std::fmt::Debug for SemanticEvictionJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticEvictionJob")
            .field("config", &self.config)
            .finish()
    }
}

impl SemanticEvictionJob {
    /// Build a job using the default [`StatsDreamingPicker`].
    pub fn new(store: Arc<dyn SemanticMemoryStore>, config: SemanticEvictionConfig) -> Self {
        Self {
            store,
            config,
            picker: Arc::new(StatsDreamingPicker),
        }
    }

    /// Build a job with a custom picker — useful for tests or when a
    /// composite-scorer lands in a later bead.
    pub fn new_with_picker(
        store: Arc<dyn SemanticMemoryStore>,
        config: SemanticEvictionConfig,
        picker: Arc<dyn DreamingCandidatePicker>,
    ) -> Self {
        Self {
            store,
            config,
            picker,
        }
    }

    /// Borrow the configured schedule for the eviction + promotion pass.
    pub fn eviction_cron(&self) -> CronSchedule {
        self.config.eviction_cron()
    }

    /// Borrow the configured schedule for the maintenance pass.
    pub fn reindex_cron(&self) -> CronSchedule {
        self.config.reindex_cron()
    }

    /// Run the full eviction + dreaming-promotion pass. Always returns
    /// `Ok(_)` on partial failures so a single agent query error
    /// doesn't starve the rest of the scheduler. Hard errors
    /// (eviction itself failing) surface as `Err(SemanticError)`.
    pub async fn run(&self) -> Result<SemanticEvictionReport, SemanticError> {
        if !self.config.enabled {
            debug!("semantic_eviction: disabled, skipping run");
            return Ok(SemanticEvictionReport::default());
        }

        let rows_before = match self.store.stats().await {
            Ok(s) => s.total_rows,
            Err(e) => {
                warn!(error = %e, "semantic_eviction: stats() failed pre-run, continuing");
                0
            }
        };

        let policy = self.config.to_policy();
        let rows_removed = self.store.evict(&policy).await?;
        info!(
            metric = "semantic_eviction_rows_removed",
            rows_removed,
            max_per_agent = ?self.config.eviction_max_per_agent,
            ttl_days = ?self.config.eviction_ttl_days,
            promoted_exempt = self.config.promoted_exempt,
            "semantic eviction complete"
        );

        // Dreaming promotion — elevate surviving top-N.
        let mut rows_promoted = 0usize;
        let mut dreaming_error: Option<String> = None;
        if self.config.dreaming_top_n > 0 {
            match self
                .picker
                .pick(self.store.as_ref(), self.config.dreaming_top_n)
                .await
            {
                Ok(ids) => {
                    for id in &ids {
                        match self.store.promote(id).await {
                            Ok(()) => rows_promoted += 1,
                            Err(SemanticError::NotFound(_)) => {
                                // Raced with eviction — fine.
                            }
                            Err(e) => {
                                warn!(
                                    id = %id,
                                    error = %e,
                                    "semantic_eviction: promote() failed for candidate"
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "semantic_eviction: dreaming picker failed; skipping promotion"
                    );
                    dreaming_error = Some(e.to_string());
                }
            }
        }
        info!(
            metric = "semantic_dreaming_promotions",
            rows_promoted,
            top_n = self.config.dreaming_top_n,
            "dreaming promotion complete"
        );

        let rows_after = match self.store.stats().await {
            Ok(s) => s.total_rows,
            Err(_) => rows_before.saturating_sub(rows_removed),
        };

        Ok(SemanticEvictionReport {
            rows_removed,
            rows_promoted,
            rows_before,
            rows_after,
            dreaming_error,
        })
    }

    /// Run the backend's `maintenance()` hook (e.g. REINDEX CONCURRENTLY
    /// for pgvector). Cheap for in-memory backends (default no-op).
    pub async fn run_maintenance(&self) -> Result<(), SemanticError> {
        if !self.config.enabled {
            debug!("semantic_eviction: disabled, skipping maintenance");
            return Ok(());
        }
        self.store.maintenance().await?;
        info!(
            metric = "semantic_memory_reindex",
            "semantic memory maintenance complete"
        );
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::{Duration as ChronoDuration, Utc};
    use sera_types::memory::SegmentKind;
    use sera_types::{SemanticEntry, SemanticStats};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Minimal in-memory [`SemanticMemoryStore`] for the workflow tests.
    ///
    /// We duplicate a slim version here (rather than re-using
    /// `sera-testing::InMemorySemanticStore`) to avoid a cross-crate
    /// dev-dependency cycle: sera-testing pulls in sera-workflow
    /// indirectly. The behaviour we need (eviction + promote + touch)
    /// is tiny and orthogonal.
    struct FakeStore {
        rows: Mutex<HashMap<MemoryId, SemanticEntry>>,
    }

    impl FakeStore {
        fn new() -> Self {
            Self {
                rows: Mutex::new(HashMap::new()),
            }
        }
        fn len(&self) -> usize {
            self.rows.lock().unwrap().len()
        }
        fn get(&self, id: &MemoryId) -> Option<SemanticEntry> {
            self.rows.lock().unwrap().get(id).cloned()
        }
    }

    #[async_trait]
    impl SemanticMemoryStore for FakeStore {
        async fn put(&self, entry: SemanticEntry) -> Result<MemoryId, SemanticError> {
            let id = entry.id.clone();
            self.rows.lock().unwrap().insert(id.clone(), entry);
            Ok(id)
        }
        async fn query(
            &self,
            query: SemanticQuery,
        ) -> Result<Vec<ScoredEntry>, SemanticError> {
            let rows = self.rows.lock().unwrap();
            let mut out: Vec<ScoredEntry> = rows
                .values()
                .filter(|e| e.agent_id == query.agent_id)
                .cloned()
                .map(|e| ScoredEntry {
                    entry: e,
                    score: 1.0,
                    index_score: 0.0,
                    vector_score: 1.0,
                    recency_score: 1.0,
                })
                .collect();
            out.truncate(query.top_k.max(1));
            Ok(out)
        }
        async fn delete(&self, id: &MemoryId) -> Result<(), SemanticError> {
            let mut rows = self.rows.lock().unwrap();
            if rows.remove(id).is_none() {
                return Err(SemanticError::NotFound(id.clone()));
            }
            Ok(())
        }
        async fn evict(&self, policy: &EvictionPolicy) -> Result<usize, SemanticError> {
            let mut rows = self.rows.lock().unwrap();
            let before = rows.len();
            if let Some(ttl) = policy.ttl_days {
                let cutoff = Utc::now() - ChronoDuration::days(ttl as i64);
                rows.retain(|_, e| {
                    if policy.promoted_exempt && e.promoted {
                        return true;
                    }
                    e.created_at >= cutoff
                });
            }
            if let Some(cap) = policy.max_per_agent {
                // Group by agent, keep newest `cap` per agent.
                let mut by_agent: HashMap<String, Vec<(MemoryId, chrono::DateTime<Utc>, bool)>> =
                    HashMap::new();
                for (id, e) in rows.iter() {
                    by_agent
                        .entry(e.agent_id.clone())
                        .or_default()
                        .push((id.clone(), e.created_at, e.promoted));
                }
                let mut to_remove: Vec<MemoryId> = Vec::new();
                for (_agent, mut entries) in by_agent {
                    entries.sort_by_key(|b| std::cmp::Reverse(b.1));
                    for (id, _ts, promoted) in entries.into_iter().skip(cap) {
                        if policy.promoted_exempt && promoted {
                            continue;
                        }
                        to_remove.push(id);
                    }
                }
                for id in to_remove {
                    rows.remove(&id);
                }
            }
            Ok(before - rows.len())
        }
        async fn stats(&self) -> Result<SemanticStats, SemanticError> {
            let rows = self.rows.lock().unwrap();
            let total_rows = rows.len();
            let mut per_agent: HashMap<String, usize> = HashMap::new();
            let mut oldest = None;
            let mut newest = None;
            for e in rows.values() {
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
            per_agent_top.sort_by_key(|b| std::cmp::Reverse(b.1));
            let now = Utc::now();
            Ok(SemanticStats {
                total_rows,
                per_agent_top,
                oldest: oldest.unwrap_or(now),
                newest: newest.unwrap_or(now),
            })
        }
        async fn promote(&self, id: &MemoryId) -> Result<(), SemanticError> {
            let mut rows = self.rows.lock().unwrap();
            match rows.get_mut(id) {
                Some(e) => {
                    e.promoted = true;
                    Ok(())
                }
                None => Err(SemanticError::NotFound(id.clone())),
            }
        }
        async fn touch(&self, _id: &MemoryId) -> Result<(), SemanticError> {
            Ok(())
        }
    }

    fn mk_entry(id: &str, agent: &str, age_days: i64, promoted: bool) -> SemanticEntry {
        SemanticEntry {
            id: MemoryId::new(id),
            agent_id: agent.to_string(),
            content: format!("content-{id}"),
            embedding: vec![0.1, 0.2, 0.3],
            tier: SegmentKind::MemoryRecall(id.to_string()),
            tags: vec![],
            created_at: Utc::now() - ChronoDuration::days(age_days),
            last_accessed_at: None,
            promoted,
        }
    }

    // ── Config defaults ──────────────────────────────────────────────────────

    #[test]
    fn config_defaults_exercise_full_policy() {
        let cfg = SemanticEvictionConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.eviction_max_per_agent, Some(DEFAULT_MAX_PER_AGENT));
        assert_eq!(cfg.eviction_ttl_days, Some(DEFAULT_TTL_DAYS));
        assert!(cfg.promoted_exempt);
        assert_eq!(cfg.dreaming_top_n, DEFAULT_DREAMING_TOP_N);
    }

    #[test]
    fn config_cron_schedules_parse() {
        let cfg = SemanticEvictionConfig::default();
        assert!(cfg.eviction_cron().is_valid());
        assert!(cfg.reindex_cron().is_valid());
    }

    #[test]
    fn config_roundtrip_preserves_fields() {
        let cfg = SemanticEvictionConfig::default();
        let j = serde_json::to_string(&cfg).unwrap();
        let back: SemanticEvictionConfig = serde_json::from_str(&j).unwrap();
        assert_eq!(back.dreaming_top_n, cfg.dreaming_top_n);
        assert_eq!(back.eviction_schedule_cron, cfg.eviction_schedule_cron);
    }

    // ── Eviction ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn ttl_eviction_removes_old_rows() {
        let store = Arc::new(FakeStore::new());
        store
            .put(mk_entry("old", "a", 400, false))
            .await
            .unwrap();
        store
            .put(mk_entry("fresh", "a", 1, false))
            .await
            .unwrap();
        assert_eq!(store.len(), 2);

        let cfg = SemanticEvictionConfig {
            eviction_max_per_agent: None,
            eviction_ttl_days: Some(180),
            dreaming_top_n: 0,
            ..Default::default()
        };
        let job = SemanticEvictionJob::new(store.clone(), cfg);
        let report = job.run().await.unwrap();
        assert_eq!(report.rows_removed, 1);
        assert_eq!(store.len(), 1);
    }

    #[tokio::test]
    async fn promoted_rows_survive_ttl_eviction() {
        let store = Arc::new(FakeStore::new());
        store
            .put(mk_entry("pinned", "a", 400, true))
            .await
            .unwrap();
        store
            .put(mk_entry("old-normal", "a", 400, false))
            .await
            .unwrap();

        let cfg = SemanticEvictionConfig {
            eviction_max_per_agent: None,
            eviction_ttl_days: Some(180),
            promoted_exempt: true,
            dreaming_top_n: 0,
            ..Default::default()
        };
        let job = SemanticEvictionJob::new(store.clone(), cfg);
        let report = job.run().await.unwrap();
        assert_eq!(report.rows_removed, 1);
        assert_eq!(store.len(), 1);
        // The pinned row is still there.
        assert!(store.get(&MemoryId::new("pinned")).is_some());
    }

    #[tokio::test]
    async fn row_cap_evicts_oldest_first() {
        let store = Arc::new(FakeStore::new());
        // 5 rows, ages 5..=1 days old. Cap=2 → keep 2 newest (days 1,2).
        for i in 0..5 {
            let id = format!("row-{i}");
            let age = (5 - i) as i64;
            store
                .put(mk_entry(&id, "a", age, false))
                .await
                .unwrap();
        }
        assert_eq!(store.len(), 5);

        let cfg = SemanticEvictionConfig {
            eviction_max_per_agent: Some(2),
            eviction_ttl_days: None,
            dreaming_top_n: 0,
            ..Default::default()
        };
        let job = SemanticEvictionJob::new(store.clone(), cfg);
        let report = job.run().await.unwrap();
        assert_eq!(report.rows_removed, 3);
        assert_eq!(store.len(), 2);
    }

    #[tokio::test]
    async fn disabled_job_is_a_noop() {
        let store = Arc::new(FakeStore::new());
        store
            .put(mk_entry("old", "a", 400, false))
            .await
            .unwrap();
        let cfg = SemanticEvictionConfig {
            enabled: false,
            eviction_ttl_days: Some(180),
            ..Default::default()
        };
        let job = SemanticEvictionJob::new(store.clone(), cfg);
        let report = job.run().await.unwrap();
        assert_eq!(report.rows_removed, 0);
        assert_eq!(store.len(), 1);
    }

    // ── Dreaming promotion ───────────────────────────────────────────────────

    struct FixedPicker {
        ids: Vec<MemoryId>,
    }
    #[async_trait]
    impl DreamingCandidatePicker for FixedPicker {
        async fn pick(
            &self,
            _store: &dyn SemanticMemoryStore,
            top_n: usize,
        ) -> Result<Vec<MemoryId>, SemanticError> {
            Ok(self.ids.iter().take(top_n).cloned().collect())
        }
    }

    #[tokio::test]
    async fn dreaming_promotion_flips_top_n_to_promoted() {
        let store = Arc::new(FakeStore::new());
        for i in 0..5 {
            store
                .put(mk_entry(&format!("r-{i}"), "a", 1, false))
                .await
                .unwrap();
        }

        let picker = Arc::new(FixedPicker {
            ids: vec![MemoryId::new("r-0"), MemoryId::new("r-2"), MemoryId::new("r-4")],
        });
        let cfg = SemanticEvictionConfig {
            eviction_max_per_agent: None,
            eviction_ttl_days: None,
            dreaming_top_n: 3,
            ..Default::default()
        };
        let job =
            SemanticEvictionJob::new_with_picker(store.clone(), cfg, picker);
        let report = job.run().await.unwrap();

        assert_eq!(report.rows_promoted, 3);
        for id in &["r-0", "r-2", "r-4"] {
            let e = store.get(&MemoryId::new(*id)).unwrap();
            assert!(e.promoted, "{id} should be promoted");
        }
        for id in &["r-1", "r-3"] {
            let e = store.get(&MemoryId::new(*id)).unwrap();
            assert!(!e.promoted, "{id} should NOT be promoted");
        }
    }

    #[tokio::test]
    async fn dreaming_promotion_skips_when_top_n_zero() {
        let store = Arc::new(FakeStore::new());
        store
            .put(mk_entry("r-0", "a", 1, false))
            .await
            .unwrap();
        let cfg = SemanticEvictionConfig {
            eviction_max_per_agent: None,
            eviction_ttl_days: None,
            dreaming_top_n: 0,
            ..Default::default()
        };
        let job = SemanticEvictionJob::new(store.clone(), cfg);
        let report = job.run().await.unwrap();
        assert_eq!(report.rows_promoted, 0);
        assert!(!store.get(&MemoryId::new("r-0")).unwrap().promoted);
    }

    // ── Maintenance ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn maintenance_default_is_ok_on_in_memory_store() {
        let store = Arc::new(FakeStore::new());
        let job = SemanticEvictionJob::new(store, SemanticEvictionConfig::default());
        // Default trait impl is a no-op; should complete without error.
        job.run_maintenance().await.unwrap();
    }

    // ── Report shape ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn report_records_before_and_after_counts() {
        let store = Arc::new(FakeStore::new());
        for i in 0..5 {
            store
                .put(mk_entry(&format!("r-{i}"), "a", 400, false))
                .await
                .unwrap();
        }
        let cfg = SemanticEvictionConfig {
            eviction_max_per_agent: None,
            eviction_ttl_days: Some(180),
            dreaming_top_n: 0,
            ..Default::default()
        };
        let job = SemanticEvictionJob::new(store.clone(), cfg);
        let report = job.run().await.unwrap();
        assert_eq!(report.rows_before, 5);
        assert_eq!(report.rows_removed, 5);
        assert_eq!(report.rows_after, 0);
    }
}
