//! Interaction quality scoring for SERA agents.
//!
//! Scores completed interactions on 5 dimensions using three modes:
//! - **SelfScore**: agent scores its own interaction from a trace summary
//! - **Evaluator**: separate evaluator agent scores the interaction
//! - **Operator**: human operator provides manual score via API
//!
//! 10% random evaluator sampling is used in steady-state for drift detection.
//! Trivial interactions (<2 turns) are skipped.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The 5 scoring dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoringDimension {
    Helpfulness,
    Accuracy,
    Efficiency,
    Communication,
    MemoryUse,
}

impl ScoringDimension {
    /// All five dimensions in a canonical order.
    pub const ALL: [ScoringDimension; 5] = [
        ScoringDimension::Helpfulness,
        ScoringDimension::Accuracy,
        ScoringDimension::Efficiency,
        ScoringDimension::Communication,
        ScoringDimension::MemoryUse,
    ];
}

/// How the score was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoringMode {
    /// Agent scores its own interaction (2000-5000 token budget from trace summary).
    SelfScore,
    /// Separate evaluator agent scores the interaction.
    Evaluator,
    /// Human operator provides manual score via API.
    Operator,
}

/// A single dimension score (0.0–1.0).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionScore {
    pub dimension: ScoringDimension,
    /// Score in range [0.0, 1.0].
    pub score: f64,
    pub rationale: String,
}

impl DimensionScore {
    /// Validate that the score is within [0.0, 1.0].
    pub fn validate(&self) -> Result<(), ScoringError> {
        if !(0.0..=1.0).contains(&self.score) {
            return Err(ScoringError::InvalidScore { value: self.score });
        }
        Ok(())
    }
}

/// Complete interaction quality score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionScore {
    pub id: String,
    pub agent_id: String,
    pub interaction_id: String,
    pub mode: ScoringMode,
    pub dimensions: Vec<DimensionScore>,
    /// Equal-weighted average of all dimension scores.
    pub overall_score: f64,
    pub turn_count: u32,
    pub scored_at: DateTime<Utc>,
}

impl InteractionScore {
    /// Compute equal-weighted average from dimension scores.
    fn compute_overall(dimensions: &[DimensionScore]) -> f64 {
        if dimensions.is_empty() {
            return 0.0;
        }
        let sum: f64 = dimensions.iter().map(|d| d.score).sum();
        sum / dimensions.len() as f64
    }
}

/// A request to score an interaction.
#[derive(Debug, Clone)]
pub struct ScoringRequest {
    pub agent_id: String,
    pub interaction_id: String,
    /// Conversation trace used as input to the scorer.
    pub trace_summary: String,
    pub turn_count: u32,
    pub mode: ScoringMode,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ScoringError {
    #[error("scoring failed: {0}")]
    ScoringFailed(String),
    #[error("invalid score value: {value} (must be 0.0-1.0)")]
    InvalidScore { value: f64 },
    #[error("parse error: {0}")]
    ParseError(String),
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait InteractionScorer: Send + Sync {
    /// Score an interaction. Returns `None` if the interaction is trivial (<2 turns).
    async fn score(
        &self,
        request: &ScoringRequest,
    ) -> Result<Option<InteractionScore>, ScoringError>;

    /// Return true if this interaction should be sent to an evaluator agent
    /// (~10% random sampling for drift detection).
    fn should_evaluate(&self, agent_id: &str) -> bool;
}

// ---------------------------------------------------------------------------
// InMemoryInteractionScorer
// ---------------------------------------------------------------------------

/// Parsed scoring response from the LLM.
#[derive(Debug, Deserialize)]
struct LlmScoringResponse {
    scores: Vec<LlmDimensionScore>,
}

#[derive(Debug, Deserialize)]
struct LlmDimensionScore {
    dimension: ScoringDimension,
    score: f64,
    rationale: String,
}

/// In-memory implementation of [`InteractionScorer`].
///
/// Scores are stored in a `Vec` behind a `RwLock`. For `SelfScore` and
/// `Evaluator` modes the scorer builds a prompt and parses the JSON response.
/// For `Operator` mode, the trace_summary is expected to contain a
/// JSON-serialized `Vec<DimensionScore>` with pre-computed scores.
pub struct InMemoryInteractionScorer {
    scores: RwLock<Vec<InteractionScore>>,
}

impl InMemoryInteractionScorer {
    /// Create a new empty scorer.
    pub fn new() -> Self {
        Self {
            scores: RwLock::new(Vec::new()),
        }
    }

    /// Build the system prompt used for self-scoring.
    fn self_score_prompt(trace: &str) -> String {
        format!(
            "You are evaluating the quality of an AI agent interaction. \
             Score the following interaction trace on exactly 5 dimensions. \
             Each score must be a float between 0.0 and 1.0. \
             Respond with valid JSON only — no markdown, no explanation outside the JSON.\n\n\
             Format:\n\
             {{\"scores\": [\
             {{\"dimension\": \"helpfulness\", \"score\": <float>, \"rationale\": \"<string>\"}}, \
             {{\"dimension\": \"accuracy\", \"score\": <float>, \"rationale\": \"<string>\"}}, \
             {{\"dimension\": \"efficiency\", \"score\": <float>, \"rationale\": \"<string>\"}}, \
             {{\"dimension\": \"communication\", \"score\": <float>, \"rationale\": \"<string>\"}}, \
             {{\"dimension\": \"memory_use\", \"score\": <float>, \"rationale\": \"<string>\"}}\
             ]}}\n\n\
             Trace:\n{trace}"
        )
    }

    /// Build the system prompt used for external evaluator scoring.
    fn evaluator_prompt(trace: &str) -> String {
        format!(
            "You are an independent evaluator assessing an AI agent's interaction quality. \
             Evaluate the trace objectively as an external observer. \
             Score the interaction on exactly 5 dimensions. \
             Each score must be a float between 0.0 and 1.0. \
             Respond with valid JSON only — no markdown, no explanation outside the JSON.\n\n\
             Format:\n\
             {{\"scores\": [\
             {{\"dimension\": \"helpfulness\", \"score\": <float>, \"rationale\": \"<string>\"}}, \
             {{\"dimension\": \"accuracy\", \"score\": <float>, \"rationale\": \"<string>\"}}, \
             {{\"dimension\": \"efficiency\", \"score\": <float>, \"rationale\": \"<string>\"}}, \
             {{\"dimension\": \"communication\", \"score\": <float>, \"rationale\": \"<string>\"}}, \
             {{\"dimension\": \"memory_use\", \"score\": <float>, \"rationale\": \"<string>\"}}\
             ]}}\n\n\
             Trace:\n{trace}"
        )
    }

    /// Parse an LLM JSON response into dimension scores.
    fn parse_llm_response(json: &str) -> Result<Vec<DimensionScore>, ScoringError> {
        let parsed: LlmScoringResponse = serde_json::from_str(json)
            .map_err(|e| ScoringError::ParseError(e.to_string()))?;

        let mut dims = Vec::with_capacity(parsed.scores.len());
        for s in parsed.scores {
            let ds = DimensionScore {
                dimension: s.dimension,
                score: s.score,
                rationale: s.rationale,
            };
            ds.validate()?;
            dims.push(ds);
        }
        Ok(dims)
    }

    /// Score using a prompt-based approach (SelfScore or Evaluator).
    ///
    /// In production this would call an LLM via sera-models. Here we accept
    /// the `trace_summary` as the raw JSON response so the module is testable
    /// without a live model. When the trace looks like JSON we parse it
    /// directly; otherwise we return an error indicating no model is wired up.
    async fn score_from_prompt(
        &self,
        _prompt: &str,
        trace_summary: &str,
    ) -> Result<Vec<DimensionScore>, ScoringError> {
        // Attempt to parse the trace_summary as a pre-supplied JSON response.
        // This is the test/offline path. A real implementation would call an
        // LLM with `_prompt` and parse the model's response.
        Self::parse_llm_response(trace_summary)
    }

    /// Return all scores for the given agent, newest first, limited to `limit`.
    pub async fn get_scores(&self, agent_id: &str, limit: usize) -> Vec<InteractionScore> {
        let guard = self.scores.read().await;
        guard
            .iter()
            .rev()
            .filter(|s| s.agent_id == agent_id)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Return the per-dimension average scores for an agent over the last
    /// `window_days` days.
    pub async fn get_average_scores(
        &self,
        agent_id: &str,
        window_days: i64,
    ) -> std::collections::HashMap<ScoringDimension, f64> {
        let cutoff = Utc::now() - chrono::Duration::days(window_days);
        let guard = self.scores.read().await;
        let relevant: Vec<&InteractionScore> = guard
            .iter()
            .filter(|s| s.agent_id == agent_id && s.scored_at >= cutoff)
            .collect();

        let mut totals: std::collections::HashMap<ScoringDimension, (f64, u32)> =
            std::collections::HashMap::new();
        for score in relevant {
            for dim in &score.dimensions {
                let entry = totals.entry(dim.dimension).or_insert((0.0, 0));
                entry.0 += dim.score;
                entry.1 += 1;
            }
        }
        totals
            .into_iter()
            .map(|(k, (sum, count))| (k, sum / count as f64))
            .collect()
    }

    /// Return the average score for a single dimension over the last
    /// `window_days` days.
    pub async fn get_dimension_average(
        &self,
        agent_id: &str,
        dimension: ScoringDimension,
        window_days: i64,
    ) -> Option<f64> {
        let cutoff = Utc::now() - chrono::Duration::days(window_days);
        let guard = self.scores.read().await;
        let mut sum = 0.0f64;
        let mut count = 0u32;
        for score in guard.iter().filter(|s| s.agent_id == agent_id && s.scored_at >= cutoff) {
            for dim in &score.dimensions {
                if dim.dimension == dimension {
                    sum += dim.score;
                    count += 1;
                }
            }
        }
        if count == 0 {
            None
        } else {
            Some(sum / count as f64)
        }
    }
}

impl Default for InMemoryInteractionScorer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl InteractionScorer for InMemoryInteractionScorer {
    async fn score(
        &self,
        request: &ScoringRequest,
    ) -> Result<Option<InteractionScore>, ScoringError> {
        // Skip trivial interactions.
        if request.turn_count < 2 {
            return Ok(None);
        }

        let dimensions = match request.mode {
            ScoringMode::SelfScore => {
                let prompt = Self::self_score_prompt(&request.trace_summary);
                self.score_from_prompt(&prompt, &request.trace_summary).await?
            }
            ScoringMode::Evaluator => {
                let prompt = Self::evaluator_prompt(&request.trace_summary);
                self.score_from_prompt(&prompt, &request.trace_summary).await?
            }
            ScoringMode::Operator => {
                // Operator mode: trace_summary contains a JSON array of DimensionScore.
                let dims: Vec<DimensionScore> =
                    serde_json::from_str(&request.trace_summary)
                        .map_err(|e| ScoringError::ParseError(e.to_string()))?;
                for d in &dims {
                    d.validate()?;
                }
                dims
            }
        };

        let overall_score = InteractionScore::compute_overall(&dimensions);
        let score = InteractionScore {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: request.agent_id.clone(),
            interaction_id: request.interaction_id.clone(),
            mode: request.mode,
            overall_score,
            dimensions,
            turn_count: request.turn_count,
            scored_at: Utc::now(),
        };

        self.scores.write().await.push(score.clone());
        Ok(Some(score))
    }

    fn should_evaluate(&self, _agent_id: &str) -> bool {
        rand::random::<f64>() < 0.1
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_scorer() -> InMemoryInteractionScorer {
        InMemoryInteractionScorer::new()
    }

    fn llm_json_response() -> String {
        serde_json::json!({
            "scores": [
                {"dimension": "helpfulness",   "score": 0.9, "rationale": "very helpful"},
                {"dimension": "accuracy",      "score": 0.8, "rationale": "mostly accurate"},
                {"dimension": "efficiency",    "score": 0.7, "rationale": "reasonable"},
                {"dimension": "communication", "score": 0.85,"rationale": "clear"},
                {"dimension": "memory_use",    "score": 0.6, "rationale": "adequate"}
            ]
        })
        .to_string()
    }

    fn operator_json() -> String {
        serde_json::json!([
            {"dimension": "helpfulness",   "score": 0.9, "rationale": "great"},
            {"dimension": "accuracy",      "score": 0.8, "rationale": "good"},
            {"dimension": "efficiency",    "score": 0.7, "rationale": "ok"},
            {"dimension": "communication", "score": 0.85,"rationale": "clear"},
            {"dimension": "memory_use",    "score": 0.6, "rationale": "fair"}
        ])
        .to_string()
    }

    #[tokio::test]
    async fn trivial_interaction_returns_none() {
        let scorer = make_scorer();
        let req = ScoringRequest {
            agent_id: "agent-1".into(),
            interaction_id: "int-1".into(),
            trace_summary: llm_json_response(),
            turn_count: 1,
            mode: ScoringMode::SelfScore,
        };
        let result = scorer.score(&req).await.unwrap();
        assert!(result.is_none(), "should return None for <2 turns");
    }

    #[tokio::test]
    async fn zero_turns_returns_none() {
        let scorer = make_scorer();
        let req = ScoringRequest {
            agent_id: "agent-1".into(),
            interaction_id: "int-2".into(),
            trace_summary: llm_json_response(),
            turn_count: 0,
            mode: ScoringMode::SelfScore,
        };
        assert!(scorer.score(&req).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn valid_self_score_produces_five_dimensions() {
        let scorer = make_scorer();
        let req = ScoringRequest {
            agent_id: "agent-1".into(),
            interaction_id: "int-3".into(),
            trace_summary: llm_json_response(),
            turn_count: 4,
            mode: ScoringMode::SelfScore,
        };
        let score = scorer.score(&req).await.unwrap().expect("should produce a score");
        assert_eq!(score.dimensions.len(), 5);
        assert_eq!(score.agent_id, "agent-1");
        assert_eq!(score.turn_count, 4);
        assert_eq!(score.mode, ScoringMode::SelfScore);
    }

    #[tokio::test]
    async fn overall_score_is_average_of_dimensions() {
        let scorer = make_scorer();
        let req = ScoringRequest {
            agent_id: "agent-1".into(),
            interaction_id: "int-4".into(),
            trace_summary: llm_json_response(),
            turn_count: 3,
            mode: ScoringMode::SelfScore,
        };
        let score = scorer.score(&req).await.unwrap().unwrap();
        let expected: f64 = score.dimensions.iter().map(|d| d.score).sum::<f64>()
            / score.dimensions.len() as f64;
        assert!(
            (score.overall_score - expected).abs() < f64::EPSILON,
            "overall_score should equal the mean of dimension scores"
        );
    }

    #[tokio::test]
    async fn evaluator_mode_produces_score() {
        let scorer = make_scorer();
        let req = ScoringRequest {
            agent_id: "agent-2".into(),
            interaction_id: "int-5".into(),
            trace_summary: llm_json_response(),
            turn_count: 5,
            mode: ScoringMode::Evaluator,
        };
        let score = scorer.score(&req).await.unwrap().unwrap();
        assert_eq!(score.mode, ScoringMode::Evaluator);
        assert_eq!(score.dimensions.len(), 5);
    }

    #[tokio::test]
    async fn operator_mode_accepts_precomputed_scores() {
        let scorer = make_scorer();
        let req = ScoringRequest {
            agent_id: "agent-3".into(),
            interaction_id: "int-6".into(),
            trace_summary: operator_json(),
            turn_count: 2,
            mode: ScoringMode::Operator,
        };
        let score = scorer.score(&req).await.unwrap().unwrap();
        assert_eq!(score.mode, ScoringMode::Operator);
        assert_eq!(score.dimensions.len(), 5);
    }

    #[tokio::test]
    async fn invalid_score_value_is_rejected() {
        let bad_json = serde_json::json!({
            "scores": [
                {"dimension": "helpfulness",   "score": 1.5, "rationale": "over range"},
                {"dimension": "accuracy",      "score": 0.8, "rationale": "ok"},
                {"dimension": "efficiency",    "score": 0.7, "rationale": "ok"},
                {"dimension": "communication", "score": 0.85,"rationale": "ok"},
                {"dimension": "memory_use",    "score": 0.6, "rationale": "ok"}
            ]
        })
        .to_string();

        let scorer = make_scorer();
        let req = ScoringRequest {
            agent_id: "agent-1".into(),
            interaction_id: "int-7".into(),
            trace_summary: bad_json,
            turn_count: 3,
            mode: ScoringMode::SelfScore,
        };
        let err = scorer.score(&req).await.unwrap_err();
        assert!(matches!(err, ScoringError::InvalidScore { .. }));
    }

    #[tokio::test]
    async fn negative_score_is_rejected() {
        let bad_json = serde_json::json!({
            "scores": [
                {"dimension": "helpfulness",   "score": -0.1, "rationale": "negative"},
                {"dimension": "accuracy",      "score": 0.8,  "rationale": "ok"},
                {"dimension": "efficiency",    "score": 0.7,  "rationale": "ok"},
                {"dimension": "communication", "score": 0.85, "rationale": "ok"},
                {"dimension": "memory_use",    "score": 0.6,  "rationale": "ok"}
            ]
        })
        .to_string();

        let scorer = make_scorer();
        let req = ScoringRequest {
            agent_id: "agent-1".into(),
            interaction_id: "int-8".into(),
            trace_summary: bad_json,
            turn_count: 3,
            mode: ScoringMode::SelfScore,
        };
        assert!(matches!(
            scorer.score(&req).await.unwrap_err(),
            ScoringError::InvalidScore { .. }
        ));
    }

    #[test]
    fn should_evaluate_is_statistically_ten_percent() {
        let scorer = make_scorer();
        let trials = 2000;
        let hits: usize = (0..trials)
            .filter(|_| scorer.should_evaluate("agent-1"))
            .count();
        // Expect roughly 200/2000. Allow ±100 for statistical noise.
        assert!(
            hits > 50 && hits < 350,
            "should_evaluate hit rate {hits}/{trials} outside expected range"
        );
    }

    #[tokio::test]
    async fn get_scores_returns_correct_results_with_limit() {
        let scorer = make_scorer();
        // Insert 5 scores for agent-a and 2 for agent-b.
        for i in 0..5u32 {
            let req = ScoringRequest {
                agent_id: "agent-a".into(),
                interaction_id: format!("int-a-{i}"),
                trace_summary: llm_json_response(),
                turn_count: 3,
                mode: ScoringMode::SelfScore,
            };
            scorer.score(&req).await.unwrap();
        }
        for i in 0..2u32 {
            let req = ScoringRequest {
                agent_id: "agent-b".into(),
                interaction_id: format!("int-b-{i}"),
                trace_summary: llm_json_response(),
                turn_count: 3,
                mode: ScoringMode::SelfScore,
            };
            scorer.score(&req).await.unwrap();
        }

        // get_scores with limit=3 should return only 3 scores for agent-a.
        let scores_a = scorer.get_scores("agent-a", 3).await;
        assert_eq!(scores_a.len(), 3);
        assert!(scores_a.iter().all(|s| s.agent_id == "agent-a"));

        // agent-b should return both scores.
        let scores_b = scorer.get_scores("agent-b", 10).await;
        assert_eq!(scores_b.len(), 2);
        assert!(scores_b.iter().all(|s| s.agent_id == "agent-b"));
    }

    #[tokio::test]
    async fn get_dimension_average_returns_correct_value() {
        let scorer = make_scorer();
        // Score helpfulness=0.9 twice for agent-x, then helpfulness=0.5 once.
        let json_high = serde_json::json!({
            "scores": [
                {"dimension": "helpfulness",   "score": 0.9, "rationale": "high"},
                {"dimension": "accuracy",      "score": 0.8, "rationale": "ok"},
                {"dimension": "efficiency",    "score": 0.7, "rationale": "ok"},
                {"dimension": "communication", "score": 0.85,"rationale": "ok"},
                {"dimension": "memory_use",    "score": 0.6, "rationale": "ok"}
            ]
        })
        .to_string();
        let json_low = serde_json::json!({
            "scores": [
                {"dimension": "helpfulness",   "score": 0.5, "rationale": "low"},
                {"dimension": "accuracy",      "score": 0.8, "rationale": "ok"},
                {"dimension": "efficiency",    "score": 0.7, "rationale": "ok"},
                {"dimension": "communication", "score": 0.85,"rationale": "ok"},
                {"dimension": "memory_use",    "score": 0.6, "rationale": "ok"}
            ]
        })
        .to_string();

        for trace in [&json_high, &json_high, &json_low] {
            scorer
                .score(&ScoringRequest {
                    agent_id: "agent-x".into(),
                    interaction_id: uuid::Uuid::new_v4().to_string(),
                    trace_summary: trace.clone(),
                    turn_count: 3,
                    mode: ScoringMode::SelfScore,
                })
                .await
                .unwrap();
        }

        let avg = scorer
            .get_dimension_average("agent-x", ScoringDimension::Helpfulness, 7)
            .await
            .expect("should have data");

        let expected = (0.9 + 0.9 + 0.5) / 3.0;
        assert!(
            (avg - expected).abs() < 1e-9,
            "expected {expected}, got {avg}"
        );
    }
}
