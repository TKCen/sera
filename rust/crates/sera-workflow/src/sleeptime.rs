//! Sleeptime Memory Consolidation — background memory processing during agent idle time.
//! SPEC-memory §2b / sera-40o.

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sera_types::memory::{MemoryBackend, MemoryError, RecallStore};
use thiserror::Error;
use tracing::{debug, info, warn};

// ── Configuration ─────────────────────────────────────────────────────────────

/// Configuration for the sleeptime consolidation service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SleeptimeConfig {
    /// Whether sleeptime consolidation is active.
    pub enabled: bool,
    /// How long (in seconds) an agent must be idle before consolidation triggers.
    #[serde(default = "default_idle_threshold_secs")]
    pub idle_threshold_secs: u64,
    /// Maximum fraction of the daily token budget that consolidation may consume.
    #[serde(default = "default_daily_token_budget_pct")]
    pub daily_token_budget_pct: f64,
    /// Enable compression of old low-importance entries.
    pub compression_enabled: bool,
    /// Enable promotion of frequently-recalled short-term entries to long-term.
    pub promotion_enabled: bool,
    /// Enable gap-detection analysis.
    pub gap_detection_enabled: bool,
    /// Cosine-similarity threshold for cross-linking entries.
    #[serde(default = "default_cross_link_threshold")]
    pub cross_link_threshold: f64,
    /// Enable exponential-decay scoring of old entries.
    pub decay_enabled: bool,
    /// Half-life used for exponential decay (in days).
    #[serde(default = "default_decay_half_life_days")]
    pub decay_half_life_days: u32,
}

fn default_idle_threshold_secs() -> u64 {
    300
}

fn default_daily_token_budget_pct() -> f64 {
    0.10
}

fn default_cross_link_threshold() -> f64 {
    0.85
}

fn default_decay_half_life_days() -> u32 {
    30
}

impl Default for SleeptimeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_threshold_secs: default_idle_threshold_secs(),
            daily_token_budget_pct: default_daily_token_budget_pct(),
            compression_enabled: true,
            promotion_enabled: true,
            gap_detection_enabled: true,
            cross_link_threshold: default_cross_link_threshold(),
            decay_enabled: true,
            decay_half_life_days: default_decay_half_life_days(),
        }
    }
}

// ── Phase enum ────────────────────────────────────────────────────────────────

/// The consolidation phase that produced a [`ConsolidationResult`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsolidationPhase {
    /// Summarise old low-importance entries to reduce storage.
    Compression,
    /// Promote frequently-recalled short-term entries to long-term.
    Promotion,
    /// Identify coverage gaps in the agent's memory.
    GapDetection,
    /// Link semantically-similar entries via cross-reference tags.
    CrossLinking,
    /// Apply exponential decay to ageing entries; archive low-scorers.
    Decay,
}

// ── Result / Report ───────────────────────────────────────────────────────────

/// Output from a single consolidation phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationResult {
    pub phase: ConsolidationPhase,
    pub entries_processed: u32,
    pub entries_modified: u32,
    pub tokens_used: u64,
    pub duration: Duration,
    pub details: serde_json::Value,
}

/// Aggregated report covering all phases run in a single consolidation cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationReport {
    pub agent_id: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub phases: Vec<ConsolidationResult>,
    pub budget_remaining_pct: f64,
}

impl ConsolidationReport {
    /// Sum of tokens used across all completed phases.
    pub fn total_tokens_used(&self) -> u64 {
        self.phases.iter().map(|p| p.tokens_used).sum()
    }

    /// Returns `true` when at least one phase was cut short by the token budget.
    pub fn was_budget_limited(&self) -> bool {
        self.budget_remaining_pct <= 0.0
    }
}

// ── IdleDetector trait ────────────────────────────────────────────────────────

/// Determines whether an agent is currently idle.
#[async_trait]
pub trait IdleDetector: Send + Sync {
    /// Returns `true` if the agent has been idle long enough to trigger consolidation.
    async fn is_idle(&self, agent_id: &str) -> Result<bool, ConsolidationError>;

    /// Returns the timestamp of the agent's last activity, or `None` if unknown.
    async fn last_activity(
        &self,
        agent_id: &str,
    ) -> Result<Option<DateTime<Utc>>, ConsolidationError>;
}

// ── Error ─────────────────────────────────────────────────────────────────────

/// Errors that may occur during sleeptime consolidation.
#[derive(Debug, Error)]
pub enum ConsolidationError {
    /// The agent is not idle; consolidation was skipped.
    #[error("agent is not idle")]
    NotIdle,

    /// The daily token budget was exceeded.
    #[error("token budget exceeded: used {used_pct:.1}% of limit {limit_pct:.1}%")]
    BudgetExceeded { used_pct: f64, limit_pct: f64 },

    /// An underlying memory backend error.
    #[error("memory error: {0}")]
    MemoryError(#[from] MemoryError),

    /// Invalid or inconsistent configuration.
    #[error("configuration error: {0}")]
    ConfigError(String),

    /// The feature is not yet implemented (POST-MVS stub).
    #[error("not implemented (post-mvs stub): {feature}")]
    PostMvsStub { feature: &'static str },
}

// ── SleeptimeConsolidator ─────────────────────────────────────────────────────

/// Background service that consolidates agent memory during idle periods.
///
/// Each phase is a no-op stub (real LLM integration is POST-MVS).
/// The structure, types, and budget-gating logic are fully wired.
pub struct SleeptimeConsolidator {
    config: SleeptimeConfig,
    idle_detector: Box<dyn IdleDetector>,
}

impl SleeptimeConsolidator {
    /// Create a new consolidator with the given configuration and idle detector.
    pub fn new(config: SleeptimeConfig, idle_detector: Box<dyn IdleDetector>) -> Self {
        Self { config, idle_detector }
    }

    /// Run a full consolidation cycle for `agent_id`.
    ///
    /// Returns `Err(ConsolidationError::NotIdle)` if the agent is active.
    /// Returns `Err(ConsolidationError::BudgetExceeded)` if the token budget
    /// is consumed before all phases complete.
    pub async fn run_consolidation(
        &self,
        agent_id: &str,
        memory: &dyn MemoryBackend,
        recall_store: &RecallStore,
    ) -> Result<ConsolidationReport, ConsolidationError> {
        if !self.idle_detector.is_idle(agent_id).await? {
            return Err(ConsolidationError::NotIdle);
        }

        info!(agent_id, "sleeptime consolidation starting");

        let started_at = Utc::now();
        let mut report = ConsolidationReport {
            agent_id: agent_id.to_string(),
            started_at,
            completed_at: None,
            phases: Vec::new(),
            budget_remaining_pct: self.config.daily_token_budget_pct,
        };

        if self.config.compression_enabled {
            match self.run_compression(agent_id, memory).await {
                Ok(result) => {
                    self.deduct_tokens(&mut report, result.tokens_used);
                    report.phases.push(result);
                    if !self.check_budget(&report) {
                        report.completed_at = Some(Utc::now());
                        return Err(ConsolidationError::BudgetExceeded {
                            used_pct: self.config.daily_token_budget_pct - report.budget_remaining_pct,
                            limit_pct: self.config.daily_token_budget_pct,
                        });
                    }
                }
                Err(ConsolidationError::PostMvsStub { feature }) => {
                    warn!(feature, "compression phase skipped: post-mvs stub");
                }
                Err(e) => return Err(e),
            }
        }

        if self.config.promotion_enabled {
            match self.run_promotion(agent_id, memory, recall_store).await {
                Ok(result) => {
                    self.deduct_tokens(&mut report, result.tokens_used);
                    report.phases.push(result);
                    if !self.check_budget(&report) {
                        report.completed_at = Some(Utc::now());
                        return Err(ConsolidationError::BudgetExceeded {
                            used_pct: self.config.daily_token_budget_pct - report.budget_remaining_pct,
                            limit_pct: self.config.daily_token_budget_pct,
                        });
                    }
                }
                Err(ConsolidationError::PostMvsStub { feature }) => {
                    warn!(feature, "promotion phase skipped: post-mvs stub");
                }
                Err(e) => return Err(e),
            }
        }

        if self.config.gap_detection_enabled {
            match self.run_gap_detection(agent_id, memory).await {
                Ok(result) => {
                    self.deduct_tokens(&mut report, result.tokens_used);
                    report.phases.push(result);
                    if !self.check_budget(&report) {
                        report.completed_at = Some(Utc::now());
                        return Err(ConsolidationError::BudgetExceeded {
                            used_pct: self.config.daily_token_budget_pct - report.budget_remaining_pct,
                            limit_pct: self.config.daily_token_budget_pct,
                        });
                    }
                }
                Err(ConsolidationError::PostMvsStub { feature }) => {
                    warn!(feature, "gap detection phase skipped: post-mvs stub");
                }
                Err(e) => return Err(e),
            }
        }

        // Cross-linking always runs if the budget allows (no separate flag in config).
        {
            match self.run_cross_linking(agent_id, memory).await {
                Ok(result) => {
                    self.deduct_tokens(&mut report, result.tokens_used);
                    report.phases.push(result);
                    if !self.check_budget(&report) {
                        report.completed_at = Some(Utc::now());
                        return Err(ConsolidationError::BudgetExceeded {
                            used_pct: self.config.daily_token_budget_pct - report.budget_remaining_pct,
                            limit_pct: self.config.daily_token_budget_pct,
                        });
                    }
                }
                Err(ConsolidationError::PostMvsStub { feature }) => {
                    warn!(feature, "cross-linking phase skipped: post-mvs stub");
                }
                Err(e) => return Err(e),
            }
        }

        if self.config.decay_enabled {
            match self.run_decay(agent_id, memory).await {
                Ok(result) => {
                    self.deduct_tokens(&mut report, result.tokens_used);
                    report.phases.push(result);
                }
                Err(ConsolidationError::PostMvsStub { feature }) => {
                    warn!(feature, "decay phase skipped: post-mvs stub");
                }
                Err(e) => return Err(e),
            }
        }

        report.completed_at = Some(Utc::now());
        info!(
            agent_id,
            phases = report.phases.len(),
            total_tokens = report.total_tokens_used(),
            "sleeptime consolidation complete"
        );

        Ok(report)
    }

    /// Returns `false` if the budget has been exhausted.
    pub fn check_budget(&self, report: &ConsolidationReport) -> bool {
        report.budget_remaining_pct > 0.0
    }

    // ── Private phase methods ─────────────────────────────────────────────────

    /// Compression phase — summarise old low-importance entries.
    ///
    /// POST-MVS: real LLM summarisation. Currently a no-op stub.
    async fn run_compression(
        &self,
        agent_id: &str,
        _memory: &dyn MemoryBackend,
    ) -> Result<ConsolidationResult, ConsolidationError> {
        debug!(agent_id, "compression phase: scanning for compaction candidates");

        // POST-MVS: query memory for old/low-importance entries and call
        // `memory.compact()` with an appropriate CompactionScope.
        Err(ConsolidationError::PostMvsStub { feature: "sleeptime.compression" })
    }

    /// Promotion phase — move entries passing promotion gates to long-term memory.
    ///
    /// POST-MVS: reads RecallStore signals and calls `memory.write()` with
    /// updated tier. Currently a no-op stub.
    async fn run_promotion(
        &self,
        agent_id: &str,
        _memory: &dyn MemoryBackend,
        _recall_store: &RecallStore,
    ) -> Result<ConsolidationResult, ConsolidationError> {
        debug!(agent_id, "promotion phase: evaluating recall signals");

        // POST-MVS: for each eligible entry, fetch from memory, update tier to
        // LongTerm, and write back.
        Err(ConsolidationError::PostMvsStub { feature: "sleeptime.promotion" })
    }

    /// Gap-detection phase — analyse memory for coverage gaps.
    ///
    /// POST-MVS: LLM-driven analysis. Currently a no-op stub that returns
    /// an empty gap list.
    async fn run_gap_detection(
        &self,
        agent_id: &str,
        _memory: &dyn MemoryBackend,
    ) -> Result<ConsolidationResult, ConsolidationError> {
        debug!(agent_id, "gap detection phase: analysing coverage");

        // POST-MVS: cluster existing entries, identify topic areas with thin
        // coverage, return structured gap descriptions.
        Err(ConsolidationError::PostMvsStub { feature: "sleeptime.gap_detection" })
    }

    /// Cross-linking phase — tag pairs of entries whose similarity exceeds the threshold.
    ///
    /// POST-MVS: embedding-based cosine similarity. Currently a no-op stub.
    async fn run_cross_linking(
        &self,
        agent_id: &str,
        _memory: &dyn MemoryBackend,
    ) -> Result<ConsolidationResult, ConsolidationError> {
        debug!(
            agent_id,
            threshold = self.config.cross_link_threshold,
            "cross-linking phase: scanning for similar entries"
        );

        // POST-MVS: embed all entries, compute pairwise cosine similarity,
        // add cross-reference tags to entries above the threshold.
        Err(ConsolidationError::PostMvsStub { feature: "sleeptime.cross_linking" })
    }

    /// Decay phase — apply exponential decay and archive entries below threshold.
    ///
    /// POST-MVS: score-weighted decay. Currently a no-op stub.
    async fn run_decay(
        &self,
        agent_id: &str,
        _memory: &dyn MemoryBackend,
    ) -> Result<ConsolidationResult, ConsolidationError> {
        debug!(
            agent_id,
            half_life_days = self.config.decay_half_life_days,
            "decay phase: applying exponential decay"
        );

        // POST-MVS: for each entry, compute age-adjusted score using:
        //   score * 0.5^(age_days / half_life_days)
        // Entries falling below an archive threshold are marked for archival.
        Err(ConsolidationError::PostMvsStub { feature: "sleeptime.decay" })
    }

    /// Subtract `tokens` from the remaining budget in `report`.
    fn deduct_tokens(&self, _report: &mut ConsolidationReport, tokens: u64) {
        // TODO(post-mvs): real token accounting not yet implemented.
        // Keeping () return to avoid cascading call-site changes; warn instead.
        warn!(tokens, "deduct_tokens: post-mvs stub — token budget not deducted");
        let _ = tokens;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::memory::{
        CompactionResult, CompactionScope, MemoryContext, MemoryEntry, MemoryId,
        MemoryQuery, MemorySearchResult, MemoryStats,
    };

    // ── Minimal in-memory backend for tests ──────────────────────────────────

    struct NoopMemory;

    #[async_trait]
    impl MemoryBackend for NoopMemory {
        async fn write(
            &self,
            entry: MemoryEntry,
            _ctx: &MemoryContext,
        ) -> Result<MemoryId, MemoryError> {
            Ok(entry.id)
        }

        async fn search(
            &self,
            _query: &MemoryQuery,
            _ctx: &MemoryContext,
        ) -> Result<Vec<MemorySearchResult>, MemoryError> {
            Ok(vec![])
        }

        async fn get(&self, id: &MemoryId) -> Result<MemoryEntry, MemoryError> {
            Err(MemoryError::NotFound { id: id.0.clone() })
        }

        async fn delete(&self, _id: &MemoryId) -> Result<(), MemoryError> {
            Ok(())
        }

        async fn compact(
            &self,
            _scope: &CompactionScope,
        ) -> Result<CompactionResult, MemoryError> {
            Ok(CompactionResult {
                entries_before: 0,
                entries_after: 0,
                entries_removed: 0,
                entries_merged: 0,
            })
        }

        async fn stats(&self) -> MemoryStats {
            use std::collections::HashMap;
            MemoryStats {
                total_entries: 0,
                entries_by_tier: HashMap::new(),
                total_size_bytes: 0,
                index_status: sera_types::memory::IndexStatus::NotConfigured,
            }
        }
    }

    // ── Idle detector stubs ───────────────────────────────────────────────────

    struct AlwaysIdle;

    #[async_trait]
    impl IdleDetector for AlwaysIdle {
        async fn is_idle(&self, _agent_id: &str) -> Result<bool, ConsolidationError> {
            Ok(true)
        }

        async fn last_activity(
            &self,
            _agent_id: &str,
        ) -> Result<Option<DateTime<Utc>>, ConsolidationError> {
            Ok(Some(Utc::now() - chrono::Duration::seconds(600)))
        }
    }

    struct NeverIdle;

    #[async_trait]
    impl IdleDetector for NeverIdle {
        async fn is_idle(&self, _agent_id: &str) -> Result<bool, ConsolidationError> {
            Ok(false)
        }

        async fn last_activity(
            &self,
            _agent_id: &str,
        ) -> Result<Option<DateTime<Utc>>, ConsolidationError> {
            Ok(Some(Utc::now()))
        }
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn sleeptime_config_defaults() {
        let cfg = SleeptimeConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.idle_threshold_secs, 300);
        assert!((cfg.daily_token_budget_pct - 0.10).abs() < f64::EPSILON);
        assert!((cfg.cross_link_threshold - 0.85).abs() < f64::EPSILON);
        assert_eq!(cfg.decay_half_life_days, 30);
    }

    #[test]
    fn sleeptime_config_roundtrip() {
        let cfg = SleeptimeConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: SleeptimeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.idle_threshold_secs, cfg.idle_threshold_secs);
        assert_eq!(parsed.decay_half_life_days, cfg.decay_half_life_days);
    }

    #[test]
    fn consolidation_phase_serde() {
        let phases = [
            ConsolidationPhase::Compression,
            ConsolidationPhase::Promotion,
            ConsolidationPhase::GapDetection,
            ConsolidationPhase::CrossLinking,
            ConsolidationPhase::Decay,
        ];
        for phase in &phases {
            let json = serde_json::to_string(phase).unwrap();
            let parsed: ConsolidationPhase = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, phase);
        }
    }

    #[test]
    fn consolidation_report_total_tokens() {
        let report = ConsolidationReport {
            agent_id: "agent-1".to_string(),
            started_at: Utc::now(),
            completed_at: None,
            phases: vec![
                ConsolidationResult {
                    phase: ConsolidationPhase::Compression,
                    entries_processed: 10,
                    entries_modified: 3,
                    tokens_used: 50,
                    duration: Duration::from_millis(100),
                    details: serde_json::json!({}),
                },
                ConsolidationResult {
                    phase: ConsolidationPhase::Promotion,
                    entries_processed: 5,
                    entries_modified: 2,
                    tokens_used: 30,
                    duration: Duration::from_millis(50),
                    details: serde_json::json!({}),
                },
            ],
            budget_remaining_pct: 0.05,
        };

        assert_eq!(report.total_tokens_used(), 80);
        assert!(!report.was_budget_limited());
    }

    #[test]
    fn consolidation_report_budget_limited() {
        let report = ConsolidationReport {
            agent_id: "agent-1".to_string(),
            started_at: Utc::now(),
            completed_at: None,
            phases: vec![],
            budget_remaining_pct: 0.0,
        };
        assert!(report.was_budget_limited());
    }

    #[tokio::test]
    async fn run_consolidation_returns_not_idle() {
        let cfg = SleeptimeConfig::default();
        let consolidator = SleeptimeConsolidator::new(cfg, Box::new(NeverIdle));
        let memory = NoopMemory;
        let store = RecallStore::new();

        let result = consolidator.run_consolidation("agent-1", &memory, &store).await;
        assert!(matches!(result, Err(ConsolidationError::NotIdle)));
    }

    #[tokio::test]
    async fn run_consolidation_all_phases_idle() {
        let cfg = SleeptimeConfig::default();
        let consolidator = SleeptimeConsolidator::new(cfg, Box::new(AlwaysIdle));
        let memory = NoopMemory;
        let store = RecallStore::new();

        let report = consolidator
            .run_consolidation("agent-1", &memory, &store)
            .await
            .expect("consolidation should succeed");

        assert_eq!(report.agent_id, "agent-1");
        assert!(report.completed_at.is_some());
        // All 5 phases are POST-MVS stubs — they are skipped (warn-logged) so
        // no phase results appear in the report.
        assert_eq!(report.phases.len(), 0);
        assert_eq!(report.total_tokens_used(), 0);
    }

    #[tokio::test]
    async fn run_consolidation_disabled_phases_skipped() {
        let cfg = SleeptimeConfig {
            compression_enabled: false,
            promotion_enabled: false,
            gap_detection_enabled: false,
            decay_enabled: false,
            ..SleeptimeConfig::default()
        };
        let consolidator = SleeptimeConsolidator::new(cfg, Box::new(AlwaysIdle));
        let memory = NoopMemory;
        let store = RecallStore::new();

        let report = consolidator
            .run_consolidation("agent-1", &memory, &store)
            .await
            .expect("consolidation should succeed");

        // Cross-linking is the only unconditional phase, but it is also a
        // POST-MVS stub — so the report has no phase results either.
        assert_eq!(report.phases.len(), 0);
    }

    #[tokio::test]
    async fn run_promotion_evaluates_recall_signals() {
        use sera_types::memory::{MemoryId, RecallSignal};

        let mut store = RecallStore::new();
        for i in 0u64..3 {
            store.record(RecallSignal {
                memory_id: MemoryId::new("mem-promote"),
                query_text: format!("query-{i}"),
                query_hash: i,
                score: 0.9,
                timestamp: "2026-04-09T10:00:00Z".to_string(),
            });
        }
        store.record(RecallSignal {
            memory_id: MemoryId::new("mem-skip"),
            query_text: "query-low".to_string(),
            query_hash: 99,
            score: 0.3,
            timestamp: "2026-04-09T10:00:00Z".to_string(),
        });

        let cfg = SleeptimeConfig::default();
        let consolidator = SleeptimeConsolidator::new(cfg, Box::new(AlwaysIdle));
        let memory = NoopMemory;

        // Promotion phase is a POST-MVS stub — run_consolidation succeeds but
        // the promotion phase is warn-logged and absent from the report.
        let result = consolidator.run_promotion("agent-1", &memory, &store).await;
        assert!(
            matches!(result, Err(ConsolidationError::PostMvsStub { feature: "sleeptime.promotion" })),
            "expected PostMvsStub for sleeptime.promotion, got: {result:?}"
        );
    }

    #[test]
    fn check_budget_positive_remaining() {
        let cfg = SleeptimeConfig::default();
        let consolidator = SleeptimeConsolidator::new(cfg, Box::new(AlwaysIdle));
        let report = ConsolidationReport {
            agent_id: "a".to_string(),
            started_at: Utc::now(),
            completed_at: None,
            phases: vec![],
            budget_remaining_pct: 0.05,
        };
        assert!(consolidator.check_budget(&report));
    }

    #[test]
    fn check_budget_zero_remaining() {
        let cfg = SleeptimeConfig::default();
        let consolidator = SleeptimeConsolidator::new(cfg, Box::new(AlwaysIdle));
        let report = ConsolidationReport {
            agent_id: "a".to_string(),
            started_at: Utc::now(),
            completed_at: None,
            phases: vec![],
            budget_remaining_pct: 0.0,
        };
        assert!(!consolidator.check_budget(&report));
    }

    #[test]
    fn consolidation_error_display() {
        let e = ConsolidationError::NotIdle;
        assert_eq!(e.to_string(), "agent is not idle");

        let e = ConsolidationError::BudgetExceeded { used_pct: 12.5, limit_pct: 10.0 };
        assert!(e.to_string().contains("budget exceeded"));

        let e = ConsolidationError::ConfigError("bad value".to_string());
        assert!(e.to_string().contains("bad value"));
    }

    // ── PostMvsStub tests ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn compression_returns_post_mvs_stub() {
        let cfg = SleeptimeConfig::default();
        let consolidator = SleeptimeConsolidator::new(cfg, Box::new(AlwaysIdle));
        let memory = NoopMemory;
        let result = consolidator.run_compression("agent-x", &memory).await;
        assert!(
            matches!(result, Err(ConsolidationError::PostMvsStub { feature: "sleeptime.compression" })),
            "expected PostMvsStub for sleeptime.compression, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn promotion_returns_post_mvs_stub() {
        let cfg = SleeptimeConfig::default();
        let consolidator = SleeptimeConsolidator::new(cfg, Box::new(AlwaysIdle));
        let memory = NoopMemory;
        let store = RecallStore::new();
        let result = consolidator.run_promotion("agent-x", &memory, &store).await;
        assert!(
            matches!(result, Err(ConsolidationError::PostMvsStub { feature: "sleeptime.promotion" })),
            "expected PostMvsStub for sleeptime.promotion, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn gap_detection_returns_post_mvs_stub() {
        let cfg = SleeptimeConfig::default();
        let consolidator = SleeptimeConsolidator::new(cfg, Box::new(AlwaysIdle));
        let memory = NoopMemory;
        let result = consolidator.run_gap_detection("agent-x", &memory).await;
        assert!(
            matches!(result, Err(ConsolidationError::PostMvsStub { feature: "sleeptime.gap_detection" })),
            "expected PostMvsStub for sleeptime.gap_detection, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn cross_linking_returns_post_mvs_stub() {
        let cfg = SleeptimeConfig::default();
        let consolidator = SleeptimeConsolidator::new(cfg, Box::new(AlwaysIdle));
        let memory = NoopMemory;
        let result = consolidator.run_cross_linking("agent-x", &memory).await;
        assert!(
            matches!(result, Err(ConsolidationError::PostMvsStub { feature: "sleeptime.cross_linking" })),
            "expected PostMvsStub for sleeptime.cross_linking, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn decay_returns_post_mvs_stub() {
        let cfg = SleeptimeConfig::default();
        let consolidator = SleeptimeConsolidator::new(cfg, Box::new(AlwaysIdle));
        let memory = NoopMemory;
        let result = consolidator.run_decay("agent-x", &memory).await;
        assert!(
            matches!(result, Err(ConsolidationError::PostMvsStub { feature: "sleeptime.decay" })),
            "expected PostMvsStub for sleeptime.decay, got: {result:?}"
        );
    }

    #[test]
    fn post_mvs_stub_error_display() {
        let e = ConsolidationError::PostMvsStub { feature: "sleeptime.compression" };
        assert!(e.to_string().contains("post-mvs stub"));
        assert!(e.to_string().contains("sleeptime.compression"));
    }
}
