//! Prompt refinement loop — weekly inner-life schedule.
//!
//! Analyzes scored interaction traces to identify weak dimensions and
//! generates evidence-based prompt change proposals via a conservative
//! one-change-per-cycle policy.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::prompt_versioning::{PromptSection, PromptVersionError};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the prompt refinement loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefinementConfig {
    /// Minimum scored interactions required before refinement runs.
    pub min_interactions: usize,
    /// Number of lowest-scoring traces to review.
    pub low_sample_size: usize,
    /// Number of highest-scoring traces to review.
    pub high_sample_size: usize,
    /// Token budget per refinement run.
    pub token_budget: usize,
    /// Maximum changes per cycle.
    pub max_changes_per_cycle: usize,
}

impl Default for RefinementConfig {
    fn default() -> Self {
        Self {
            min_interactions: 10,
            low_sample_size: 5,
            high_sample_size: 5,
            token_budget: 5000,
            max_changes_per_cycle: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Input / output types
// ---------------------------------------------------------------------------

/// A scored interaction trace used as input to refinement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredTrace {
    pub interaction_id: String,
    pub agent_id: String,
    pub overall_score: f64,
    pub dimension_scores: HashMap<String, f64>,
    pub trace_summary: String,
}

/// A proposed prompt change from the refinement analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptChange {
    pub section: PromptSection,
    pub current_content: String,
    pub proposed_content: String,
    pub rationale: String,
    pub weak_dimension: String,
    pub expected_improvement: String,
}

/// Result of a refinement cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefinementResult {
    pub agent_id: String,
    pub traces_analyzed: usize,
    pub weakest_dimension: String,
    pub proposed_change: Option<PromptChange>,
    pub applied: bool,
    pub cycle_timestamp: chrono::DateTime<chrono::Utc>,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during a refinement cycle.
#[derive(Debug, thiserror::Error)]
pub enum RefinementError {
    #[error("insufficient data: need {needed} interactions, have {have}")]
    InsufficientData { needed: usize, have: usize },
    #[error("analysis failed: {0}")]
    AnalysisFailed(String),
    #[error("prompt version error: {0}")]
    VersionError(#[from] PromptVersionError),
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Runs a prompt refinement cycle for a given agent.
#[async_trait]
pub trait RefinementAnalyzer: Send + Sync {
    /// Run a refinement cycle for the given agent.
    ///
    /// Returns `None` if there are fewer than `min_interactions` scored traces.
    async fn analyze(
        &self,
        agent_id: &str,
        traces: &[ScoredTrace],
    ) -> Result<Option<RefinementResult>, RefinementError>;
}

// ---------------------------------------------------------------------------
// In-memory implementation
// ---------------------------------------------------------------------------

/// Simple in-memory refinement analyzer. Does not call an LLM; the proposed
/// content is a placeholder that a real integration would replace.
pub struct InMemoryRefinementAnalyzer {
    config: RefinementConfig,
}

impl InMemoryRefinementAnalyzer {
    /// Create a new analyzer with the given configuration.
    pub fn new(config: RefinementConfig) -> Self {
        Self { config }
    }

    /// Identify the weakest dimension by averaging scores across the provided
    /// traces. Returns the dimension name with the lowest average, or
    /// `"unknown"` if no dimension data is present.
    fn weakest_dimension(traces: &[&ScoredTrace]) -> String {
        let mut totals: HashMap<&str, (f64, usize)> = HashMap::new();
        for trace in traces {
            for (dim, &score) in &trace.dimension_scores {
                let entry = totals.entry(dim.as_str()).or_insert((0.0, 0));
                entry.0 += score;
                entry.1 += 1;
            }
        }
        totals
            .into_iter()
            .map(|(dim, (sum, count))| (dim, sum / count as f64))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(dim, _)| dim.to_owned())
            .unwrap_or_else(|| "unknown".to_owned())
    }

    /// Compute the average overall score for a slice of traces.
    fn avg_score(traces: &[&ScoredTrace]) -> f64 {
        if traces.is_empty() {
            return 0.0;
        }
        traces.iter().map(|t| t.overall_score).sum::<f64>() / traces.len() as f64
    }
}

#[async_trait]
impl RefinementAnalyzer for InMemoryRefinementAnalyzer {
    async fn analyze(
        &self,
        agent_id: &str,
        traces: &[ScoredTrace],
    ) -> Result<Option<RefinementResult>, RefinementError> {
        let needed = self.config.min_interactions;
        if traces.len() < needed {
            return Ok(None);
        }

        // Sort by overall_score ascending (lowest first).
        let mut sorted: Vec<&ScoredTrace> = traces.iter().collect();
        sorted.sort_by(|a, b| {
            a.overall_score
                .partial_cmp(&b.overall_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let low_n = self.config.low_sample_size.min(sorted.len());
        let high_n = self.config.high_sample_size.min(sorted.len());

        let low_group: Vec<&ScoredTrace> = sorted[..low_n].to_vec();
        let high_group: Vec<&ScoredTrace> = sorted[sorted.len() - high_n..].to_vec();

        let weak_dim = Self::weakest_dimension(&low_group);
        let low_avg = Self::avg_score(&low_group);
        let high_avg = Self::avg_score(&high_group);
        let score_delta = high_avg - low_avg;

        // Respect max_changes_per_cycle — currently always 1.
        let proposed_change = if self.config.max_changes_per_cycle >= 1 {
            Some(PromptChange {
                section: PromptSection::Principles,
                current_content: String::new(),
                proposed_content: format!(
                    "[PLACEHOLDER] Strengthen guidance for dimension '{weak_dim}'. \
                     Real content to be generated by LLM integration."
                ),
                rationale: format!(
                    "Dimension '{weak_dim}' scored {low_avg:.2} on average across the \
                     {low_n} lowest-scoring traces versus {high_avg:.2} on the \
                     {high_n} highest-scoring traces (delta: {score_delta:.2}). \
                     Improving this dimension is expected to raise overall quality."
                ),
                weak_dimension: weak_dim.clone(),
                expected_improvement: format!(
                    "Reduce gap between low and high groups on '{weak_dim}' \
                     (current delta {score_delta:.2})."
                ),
            })
        } else {
            None
        };

        Ok(Some(RefinementResult {
            agent_id: agent_id.to_owned(),
            traces_analyzed: traces.len(),
            weakest_dimension: weak_dim,
            proposed_change,
            applied: false, // Review mode — explicit activation required.
            cycle_timestamp: chrono::Utc::now(),
        }))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_trace(id: &str, overall: f64, dims: &[(&str, f64)]) -> ScoredTrace {
        ScoredTrace {
            interaction_id: id.to_owned(),
            agent_id: "agent-test".to_owned(),
            overall_score: overall,
            dimension_scores: dims.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            trace_summary: format!("trace {id}"),
        }
    }

    fn default_analyzer() -> InMemoryRefinementAnalyzer {
        InMemoryRefinementAnalyzer::new(RefinementConfig::default())
    }

    #[tokio::test]
    async fn insufficient_traces_returns_none() {
        let analyzer = default_analyzer();
        let traces: Vec<ScoredTrace> = (0..9)
            .map(|i| make_trace(&i.to_string(), i as f64 * 0.1, &[("clarity", 0.5)]))
            .collect();
        let result = analyzer.analyze("agent-1", &traces).await.unwrap();
        assert!(result.is_none(), "should return None with fewer than 10 traces");
    }

    #[tokio::test]
    async fn exactly_min_interactions_produces_result() {
        let analyzer = default_analyzer();
        let traces: Vec<ScoredTrace> = (0..10)
            .map(|i| make_trace(&i.to_string(), i as f64 * 0.1, &[("clarity", i as f64 * 0.1)]))
            .collect();
        let result = analyzer.analyze("agent-1", &traces).await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn weakest_dimension_identified_correctly() {
        let analyzer = default_analyzer();

        // 10 traces: low-scorers have low "conciseness", high "tone"
        let mut traces = Vec::new();
        for i in 0..5 {
            traces.push(make_trace(
                &format!("low-{i}"),
                0.2 + i as f64 * 0.01,
                &[("conciseness", 0.1), ("tone", 0.9)],
            ));
        }
        for i in 0..5 {
            traces.push(make_trace(
                &format!("high-{i}"),
                0.8 + i as f64 * 0.01,
                &[("conciseness", 0.8), ("tone", 0.9)],
            ));
        }

        let result = analyzer.analyze("agent-1", &traces).await.unwrap().unwrap();
        assert_eq!(result.weakest_dimension, "conciseness");
    }

    #[tokio::test]
    async fn result_has_applied_false() {
        let analyzer = default_analyzer();
        let traces: Vec<ScoredTrace> = (0..10)
            .map(|i| make_trace(&i.to_string(), i as f64 * 0.1, &[("clarity", 0.5)]))
            .collect();
        let result = analyzer.analyze("agent-1", &traces).await.unwrap().unwrap();
        assert!(!result.applied, "activation is Review mode — applied must be false");
    }

    #[tokio::test]
    async fn max_changes_per_cycle_one() {
        let analyzer = default_analyzer();
        let traces: Vec<ScoredTrace> = (0..10)
            .map(|i| make_trace(&i.to_string(), i as f64 * 0.1, &[("clarity", 0.5)]))
            .collect();
        let result = analyzer.analyze("agent-1", &traces).await.unwrap().unwrap();
        // At most one proposed change.
        let change_count = if result.proposed_change.is_some() { 1usize } else { 0 };
        assert!(change_count <= 1);
    }

    #[tokio::test]
    async fn config_defaults_are_correct() {
        let cfg = RefinementConfig::default();
        assert_eq!(cfg.min_interactions, 10);
        assert_eq!(cfg.low_sample_size, 5);
        assert_eq!(cfg.high_sample_size, 5);
        assert_eq!(cfg.token_budget, 5000);
        assert_eq!(cfg.max_changes_per_cycle, 1);
    }

    #[tokio::test]
    async fn zero_max_changes_produces_no_change() {
        let analyzer = InMemoryRefinementAnalyzer::new(RefinementConfig {
            max_changes_per_cycle: 0,
            ..RefinementConfig::default()
        });
        let traces: Vec<ScoredTrace> = (0..10)
            .map(|i| make_trace(&i.to_string(), i as f64 * 0.1, &[("clarity", 0.5)]))
            .collect();
        let result = analyzer.analyze("agent-1", &traces).await.unwrap().unwrap();
        assert!(result.proposed_change.is_none());
    }
}
