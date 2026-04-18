//! Hybrid retrieval scoring — bead `sera-t5k`.
//!
//! Combines three signals into a unified relevance score for a single
//! [`Candidate`]:
//!
//! 1. **Full-text index score** — in-memory BM25 over a candidate slice
//!    (K1 = 1.5, b = 0.75 by default).
//! 2. **Vector similarity** — cosine similarity between query and
//!    candidate embeddings. Falls back to `0.0` when either vector is
//!    zero-valued (e.g. the gateway embedding stub).
//! 3. **Recency decay** — exponential decay with a configurable half-life.
//!
//! The three signals are min-max normalised across the candidate set
//! (index and recency individually, vector is already bounded to
//! `[-1, 1]` and is clamped to `[0, 1]` before weighting) and combined
//! via a weighted sum controlled by [`HybridRetrievalConfig`].
//!
//! The module is intentionally self-contained: it does not depend on
//! the live memory backend or embedding service. Integration with
//! [`crate::context_engine::ContextPipeline`] is tracked separately —
//! see `rust/docs/plan/HYBRID-RETRIEVAL.md`.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

/// Weights and parameters for hybrid scoring.
///
/// Weights are expected to sum to approximately `1.0` — the scorer does
/// not renormalise, so callers producing configuration from user input
/// should validate with [`HybridRetrievalConfig::validate`].
#[derive(Debug, Clone)]
pub struct HybridRetrievalConfig {
    /// Weight applied to the normalised BM25 index score.
    pub index_weight: f64,
    /// Weight applied to the cosine similarity score (clamped to `[0, 1]`).
    pub vector_weight: f64,
    /// Weight applied to the normalised recency score.
    pub recency_weight: f64,
    /// Half-life for recency decay, in seconds. Must be > 0.
    pub recency_half_life_secs: f64,
    /// BM25 K1 parameter. Defaults to 1.5.
    pub bm25_k1: f64,
    /// BM25 b parameter. Defaults to 0.75.
    pub bm25_b: f64,
    /// Master feature flag — when `false`, callers should short-circuit
    /// to the existing single-pass retrieval.
    pub hybrid_retrieval: bool,
}

impl Default for HybridRetrievalConfig {
    fn default() -> Self {
        Self {
            index_weight: 0.4,
            vector_weight: 0.4,
            recency_weight: 0.2,
            recency_half_life_secs: 24.0 * 60.0 * 60.0,
            bm25_k1: 1.5,
            bm25_b: 0.75,
            hybrid_retrieval: true,
        }
    }
}

impl HybridRetrievalConfig {
    /// Return `Err` if weights are negative or do not sum to `1.0 ± 1e-6`,
    /// or if the half-life is not strictly positive.
    pub fn validate(&self) -> Result<(), String> {
        if self.index_weight < 0.0 || self.vector_weight < 0.0 || self.recency_weight < 0.0 {
            return Err("hybrid weights must be non-negative".to_string());
        }
        let sum = self.index_weight + self.vector_weight + self.recency_weight;
        if (sum - 1.0).abs() > 1e-6 {
            return Err(format!("hybrid weights must sum to 1.0, got {sum}"));
        }
        if self.recency_half_life_secs <= 0.0 {
            return Err("recency_half_life_secs must be > 0".to_string());
        }
        Ok(())
    }
}

/// A single retrieval candidate with the fields needed for hybrid scoring.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// Stable identifier (used only for tie-breaking / dedupe, not scoring).
    pub id: String,
    /// Tokenised content used by the BM25 scorer. Tokens are assumed
    /// already lower-cased; the scorer does no further normalisation.
    pub tokens: Vec<String>,
    /// Embedding vector for cosine similarity. A zero vector signals
    /// "embedding unavailable" and contributes `0.0` to the vector score.
    pub embedding: Vec<f32>,
    /// When the candidate was written. Used by the recency decay.
    pub created_at: DateTime<Utc>,
}

/// A candidate paired with its computed hybrid score.
#[derive(Debug, Clone)]
pub struct ScoredCandidate<'a> {
    pub candidate: &'a Candidate,
    pub index_score: f64,
    pub vector_score: f64,
    pub recency_score: f64,
    pub combined_score: f64,
}

/// Hybrid retrieval scorer.
///
/// Construction is cheap; callers should rebuild a scorer whenever the
/// candidate slice changes because BM25 statistics (doc lengths, IDF,
/// avgdl) are pre-computed over the provided slice.
pub struct HybridScorer<'a> {
    config: HybridRetrievalConfig,
    now: DateTime<Utc>,
    query_tokens: Vec<String>,
    query_embedding: Vec<f32>,
    candidates: &'a [Candidate],
    // BM25 statistics.
    avgdl: f64,
    doc_freq: HashMap<String, u32>,
    // Min/max tracking for normalisation.
    bm25_raw: Vec<f64>,
    recency_raw: Vec<f64>,
}

impl<'a> HybridScorer<'a> {
    /// Build a scorer against `candidates` using `config`.
    ///
    /// `now` is injected for determinism in tests; production callers
    /// should pass `Utc::now()`.
    pub fn new(
        config: HybridRetrievalConfig,
        query_tokens: Vec<String>,
        query_embedding: Vec<f32>,
        candidates: &'a [Candidate],
        now: DateTime<Utc>,
    ) -> Self {
        let n = candidates.len();
        let avgdl = if n == 0 {
            0.0
        } else {
            let sum: usize = candidates.iter().map(|c| c.tokens.len()).sum();
            sum as f64 / n as f64
        };

        let mut doc_freq: HashMap<String, u32> = HashMap::new();
        for c in candidates {
            let unique: std::collections::HashSet<&String> = c.tokens.iter().collect();
            for t in unique {
                *doc_freq.entry(t.clone()).or_insert(0) += 1;
            }
        }

        let mut scorer = Self {
            config,
            now,
            query_tokens,
            query_embedding,
            candidates,
            avgdl,
            doc_freq,
            bm25_raw: Vec::with_capacity(n),
            recency_raw: Vec::with_capacity(n),
        };

        // Pre-compute raw BM25 + recency so we can min-max normalise.
        for c in candidates {
            scorer.bm25_raw.push(scorer.bm25(c));
            scorer.recency_raw.push(scorer.recency(c));
        }

        scorer
    }

    /// Raw BM25 score for a single candidate against the current query.
    /// K1, b and N (= candidate count) come from config / the slice.
    fn bm25(&self, c: &Candidate) -> f64 {
        if self.candidates.is_empty() || c.tokens.is_empty() {
            return 0.0;
        }
        let n = self.candidates.len() as f64;
        let dl = c.tokens.len() as f64;
        let k1 = self.config.bm25_k1;
        let b = self.config.bm25_b;

        let mut tf_counts: HashMap<&str, u32> = HashMap::new();
        for tok in &c.tokens {
            *tf_counts.entry(tok.as_str()).or_insert(0) += 1;
        }

        let mut score = 0.0;
        for q in &self.query_tokens {
            let tf = *tf_counts.get(q.as_str()).unwrap_or(&0) as f64;
            if tf == 0.0 {
                continue;
            }
            let df = *self.doc_freq.get(q).unwrap_or(&0) as f64;
            // Robertson–Sparck Jones IDF with the standard additive
            // smoothing that keeps the value non-negative.
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            let norm = 1.0 - b + b * (dl / self.avgdl.max(1e-9));
            let contrib = idf * (tf * (k1 + 1.0)) / (tf + k1 * norm);
            score += contrib;
        }
        score
    }

    /// Cosine similarity clamped to `[0, 1]`. Degrades to `0.0` if either
    /// vector is empty or zero-magnitude (e.g. embedding stub path).
    fn cosine(&self, c: &Candidate) -> f64 {
        if self.query_embedding.is_empty() || c.embedding.is_empty() {
            return 0.0;
        }
        if self.query_embedding.len() != c.embedding.len() {
            return 0.0;
        }
        let mut dot = 0.0f64;
        let mut qa = 0.0f64;
        let mut cb = 0.0f64;
        for (q, v) in self.query_embedding.iter().zip(c.embedding.iter()) {
            let qf = *q as f64;
            let vf = *v as f64;
            dot += qf * vf;
            qa += qf * qf;
            cb += vf * vf;
        }
        if qa == 0.0 || cb == 0.0 {
            return 0.0;
        }
        let sim = dot / (qa.sqrt() * cb.sqrt());
        // Clamp to [0, 1]; negative cosine is not useful as a ranking signal
        // and would otherwise drag combined_score below the index/recency floor.
        sim.clamp(0.0, 1.0)
    }

    /// Exponential recency decay: `score = 2^(-age / half_life)`.
    fn recency(&self, c: &Candidate) -> f64 {
        let age_secs = (self.now - c.created_at).num_seconds() as f64;
        let age_secs = age_secs.max(0.0);
        let half_life = self.config.recency_half_life_secs.max(1e-9);
        (-age_secs / half_life * std::f64::consts::LN_2).exp()
    }

    /// Compute the hybrid score for a single candidate.
    ///
    /// Must be called with a candidate that belongs to the slice passed
    /// into [`Self::new`] — otherwise the normalisation will be skewed.
    pub fn score_hybrid(&self, candidate: &Candidate) -> f64 {
        self.score_components(candidate).combined_score
    }

    fn score_components<'b>(&self, candidate: &'b Candidate) -> ScoredCandidate<'b> {
        let raw_bm25 = self.bm25(candidate);
        let raw_recency = self.recency(candidate);
        let vec_score = self.cosine(candidate);

        let norm_index = normalise(raw_bm25, &self.bm25_raw);
        let norm_recency = normalise(raw_recency, &self.recency_raw);

        let combined = self.config.index_weight * norm_index
            + self.config.vector_weight * vec_score
            + self.config.recency_weight * norm_recency;

        ScoredCandidate {
            candidate,
            index_score: norm_index,
            vector_score: vec_score,
            recency_score: norm_recency,
            combined_score: combined,
        }
    }

    /// Score and rank every candidate, returning descending-score order.
    pub fn rank(&self) -> Vec<ScoredCandidate<'_>> {
        let mut scored: Vec<ScoredCandidate<'_>> =
            self.candidates.iter().map(|c| self.score_components(c)).collect();
        // Stable sort so equal scores preserve original order.
        scored.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored
    }
}

/// Min-max normalise `value` against the observed range in `all`.
/// Returns `0.0` if the range is empty or flat.
fn normalise(value: f64, all: &[f64]) -> f64 {
    if all.is_empty() {
        return 0.0;
    }
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for v in all {
        if *v < min {
            min = *v;
        }
        if *v > max {
            max = *v;
        }
    }
    let span = max - min;
    if span <= f64::EPSILON {
        return 0.0;
    }
    ((value - min) / span).clamp(0.0, 1.0)
}

/// Convenience: tokenise a string on whitespace and lower-case each token.
/// Callers with richer tokenisation needs should construct
/// `Candidate::tokens` themselves.
pub fn tokenise(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn cand(id: &str, text: &str, emb: Vec<f32>, created_at: DateTime<Utc>) -> Candidate {
        Candidate {
            id: id.to_string(),
            tokens: tokenise(text),
            embedding: emb,
            created_at,
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-04-17T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn default_config_validates() {
        assert!(HybridRetrievalConfig::default().validate().is_ok());
    }

    #[test]
    fn config_rejects_weights_not_summing_to_one() {
        let cfg = HybridRetrievalConfig {
            index_weight: 0.5,
            vector_weight: 0.6,
            recency_weight: 0.0,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_rejects_negative_weight() {
        let cfg = HybridRetrievalConfig {
            index_weight: -0.1,
            vector_weight: 0.6,
            recency_weight: 0.5,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_rejects_nonpositive_half_life() {
        let cfg = HybridRetrievalConfig {
            recency_half_life_secs: 0.0,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn empty_candidate_set_ranks_empty() {
        let candidates: Vec<Candidate> = vec![];
        let scorer = HybridScorer::new(
            HybridRetrievalConfig::default(),
            tokenise("rust async"),
            vec![1.0, 0.0, 0.0],
            &candidates,
            now(),
        );
        assert!(scorer.rank().is_empty());
    }

    #[test]
    fn single_candidate_gets_zero_normalised_score_with_flat_range() {
        // With one candidate, min == max, so index and recency normalise to 0.
        // Vector score still contributes.
        let candidates = vec![cand(
            "a",
            "rust async trait",
            vec![1.0, 0.0, 0.0],
            now() - Duration::seconds(60),
        )];
        let scorer = HybridScorer::new(
            HybridRetrievalConfig::default(),
            tokenise("rust async"),
            vec![1.0, 0.0, 0.0],
            &candidates,
            now(),
        );
        let scored = scorer.rank();
        assert_eq!(scored.len(), 1);
        // index_weight=0.4 × 0 + vector_weight=0.4 × 1.0 + recency_weight=0.2 × 0 = 0.4
        assert!((scored[0].combined_score - 0.4).abs() < 1e-9);
        assert_eq!(scored[0].index_score, 0.0);
        assert!((scored[0].vector_score - 1.0).abs() < 1e-9);
        assert_eq!(scored[0].recency_score, 0.0);
    }

    #[test]
    fn bm25_prefers_term_matches() {
        // Two candidates: one contains both query tokens, the other contains none.
        let candidates = vec![
            cand("match", "rust async programming", vec![], now()),
            cand("no-match", "python sync programming", vec![], now()),
        ];
        let cfg = HybridRetrievalConfig {
            index_weight: 1.0,
            vector_weight: 0.0,
            recency_weight: 0.0,
            ..Default::default()
        };
        let scorer = HybridScorer::new(
            cfg,
            tokenise("rust async"),
            vec![],
            &candidates,
            now(),
        );
        let ranked = scorer.rank();
        assert_eq!(ranked[0].candidate.id, "match");
        assert!(ranked[0].combined_score > ranked[1].combined_score);
    }

    #[test]
    fn vector_fallback_when_zero_embedding() {
        // Candidates with zero-vector embeddings (gateway stub path) must
        // not crash and must produce a 0.0 vector component — the combined
        // score degrades gracefully to index + recency.
        let candidates = vec![
            cand("a", "rust async", vec![0.0, 0.0, 0.0], now()),
            cand("b", "python sync", vec![0.0, 0.0, 0.0], now() - Duration::days(30)),
        ];
        let scorer = HybridScorer::new(
            HybridRetrievalConfig::default(),
            tokenise("rust async"),
            vec![0.0, 0.0, 0.0],
            &candidates,
            now(),
        );
        let ranked = scorer.rank();
        assert_eq!(ranked.len(), 2);
        for r in &ranked {
            assert_eq!(r.vector_score, 0.0);
        }
        // "a" is both more recent and matches the query → ranks first.
        assert_eq!(ranked[0].candidate.id, "a");
    }

    #[test]
    fn recency_at_now_vs_old() {
        // Two candidates, identical text and vectors; only the timestamps
        // differ. The more recent one must rank first with the default
        // 24-hour half-life.
        let candidates = vec![
            cand("now", "rust async", vec![1.0, 0.0], now()),
            cand("old", "rust async", vec![1.0, 0.0], now() - Duration::days(7)),
        ];
        let scorer = HybridScorer::new(
            HybridRetrievalConfig::default(),
            tokenise("rust async"),
            vec![1.0, 0.0],
            &candidates,
            now(),
        );
        let ranked = scorer.rank();
        assert_eq!(ranked[0].candidate.id, "now");
        assert!(ranked[0].combined_score > ranked[1].combined_score);
    }

    #[test]
    fn recency_half_life_is_respected() {
        // Half-life = 3600s → a 3600s-old doc should score ~0.5 raw recency.
        let cfg = HybridRetrievalConfig {
            index_weight: 0.0,
            vector_weight: 0.0,
            recency_weight: 1.0,
            recency_half_life_secs: 3600.0,
            ..Default::default()
        };
        let candidates = vec![
            cand("fresh", "x", vec![], now()),
            cand("half", "x", vec![], now() - Duration::seconds(3600)),
            cand("day", "x", vec![], now() - Duration::days(1)),
        ];
        let scorer = HybridScorer::new(
            cfg,
            vec!["x".to_string()],
            vec![],
            &candidates,
            now(),
        );
        // Check raw recency curve before normalisation.
        assert!((scorer.recency_raw[0] - 1.0).abs() < 1e-9);
        assert!((scorer.recency_raw[1] - 0.5).abs() < 1e-6);
        assert!(scorer.recency_raw[2] < 0.01); // 24h at 1h half-life → tiny
        let ranked = scorer.rank();
        assert_eq!(ranked[0].candidate.id, "fresh");
        assert_eq!(ranked[2].candidate.id, "day");
    }

    #[test]
    fn weighted_sum_respects_configured_weights() {
        // Give all weight to vector → BM25 and recency should not affect order.
        let cfg = HybridRetrievalConfig {
            index_weight: 0.0,
            vector_weight: 1.0,
            recency_weight: 0.0,
            ..Default::default()
        };
        let candidates = vec![
            // Perfect text + recency match but orthogonal embedding.
            cand("text", "rust async", vec![0.0, 1.0], now()),
            // No text match and old, but perfect embedding match.
            cand(
                "vec",
                "python sync",
                vec![1.0, 0.0],
                now() - Duration::days(30),
            ),
        ];
        let scorer = HybridScorer::new(
            cfg,
            tokenise("rust async"),
            vec![1.0, 0.0],
            &candidates,
            now(),
        );
        let ranked = scorer.rank();
        assert_eq!(ranked[0].candidate.id, "vec");
    }

    #[test]
    fn tokenise_strips_punctuation_and_lowercases() {
        let toks = tokenise("Rust, async! Programming?");
        assert_eq!(toks, vec!["rust", "async", "programming"]);
    }

    #[test]
    fn mismatched_embedding_dims_are_safe() {
        // 2-dim query vs 3-dim candidate → vector score is 0, not a panic.
        let candidates = vec![cand(
            "a",
            "rust",
            vec![1.0, 0.0, 0.0],
            now(),
        )];
        let scorer = HybridScorer::new(
            HybridRetrievalConfig::default(),
            tokenise("rust"),
            vec![1.0, 0.0],
            &candidates,
            now(),
        );
        let ranked = scorer.rank();
        assert_eq!(ranked[0].vector_score, 0.0);
    }

    #[test]
    fn hybrid_retrieval_flag_can_disable() {
        // Sanity check on the kill-switch field — the scorer itself
        // doesn't enforce it (callers do), but the field must be
        // toggleable without breaking Default/Clone.
        let mut cfg = HybridRetrievalConfig::default();
        assert!(cfg.hybrid_retrieval);
        cfg.hybrid_retrieval = false;
        let cloned = cfg.clone();
        assert!(!cloned.hybrid_retrieval);
    }
}
