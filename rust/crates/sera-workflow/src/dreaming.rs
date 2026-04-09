use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Top-level configuration for the dreaming workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamingConfig {
    /// Whether dreaming is active.
    pub enabled: bool,
    /// Cron expression that controls when dreaming runs.
    pub schedule: String,
    /// Per-phase configuration.
    pub phases: DreamingPhases,
}

/// Configuration for all three dreaming phases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamingPhases {
    pub light: LightSleepConfig,
    pub rem: RemSleepConfig,
    pub deep: DeepSleepConfig,
}

/// Light-sleep phase — recent memory scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightSleepConfig {
    /// How many days back to scan for recent memories.
    pub lookback_days: u32,
    /// Maximum number of candidates to surface.
    pub limit: u32,
}

/// REM-sleep phase — pattern detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemSleepConfig {
    /// How many days back to scan for patterns.
    pub lookback_days: u32,
    /// Minimum pattern-strength score to consider.
    pub min_pattern_strength: f64,
}

/// Deep-sleep phase — memory consolidation and promotion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSleepConfig {
    /// Minimum composite score required to promote a candidate.
    pub min_score: f64,
    /// Minimum number of times a memory must have been recalled.
    pub min_recall_count: u32,
    /// Minimum number of unique queries that surfaced this memory.
    pub min_unique_queries: u32,
    /// Maximum age in days; older memories are skipped.
    pub max_age_days: u32,
    /// Maximum number of candidates to promote per run.
    pub limit: u32,
}

/// Scoring weights used during dream candidate evaluation.
///
/// The default values sum to exactly `1.0` per SPEC-workflow-engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamingWeights {
    pub relevance: f64,
    pub frequency: f64,
    pub query_diversity: f64,
    pub recency: f64,
    pub consolidation: f64,
    pub conceptual_richness: f64,
}

impl Default for DreamingWeights {
    fn default() -> Self {
        Self {
            relevance: 0.30,
            frequency: 0.24,
            query_diversity: 0.15,
            recency: 0.15,
            consolidation: 0.10,
            conceptual_richness: 0.06,
        }
    }
}

/// A memory candidate being evaluated for promotion during deep sleep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamCandidate {
    /// The key that identifies the memory.
    pub memory_key: String,
    /// Individual dimension scores (keyed by dimension name).
    pub scores: HashMap<String, f64>,
    /// Weighted composite score — populated by [`DreamCandidate::compute_score`].
    pub total_score: f64,
    /// How many times this memory has been recalled.
    pub recall_count: u32,
    /// How many distinct queries surfaced this memory.
    pub unique_queries: u32,
}

impl DreamCandidate {
    /// Compute the weighted composite score from the individual dimension scores
    /// and store it in [`DreamCandidate::total_score`].
    pub fn compute_score(&mut self, weights: &DreamingWeights) {
        let get = |key: &str| self.scores.get(key).copied().unwrap_or(0.0);

        self.total_score = get("relevance") * weights.relevance
            + get("frequency") * weights.frequency
            + get("query_diversity") * weights.query_diversity
            + get("recency") * weights.recency
            + get("consolidation") * weights.consolidation
            + get("conceptual_richness") * weights.conceptual_richness;
    }

    /// Returns `true` when this candidate passes all deep-sleep gate conditions.
    pub fn passes_gates(&self, config: &DeepSleepConfig) -> bool {
        self.total_score >= config.min_score
            && self.recall_count >= config.min_recall_count
            && self.unique_queries >= config.min_unique_queries
    }
}
