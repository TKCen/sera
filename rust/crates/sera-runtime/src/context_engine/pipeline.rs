//! ContextPipeline — wraps the old ContextPipeline as a ContextEngine impl.

use std::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;
use once_cell::sync::Lazy;
use tiktoken_rs::cl100k_base;
use tiktoken_rs::CoreBPE;

use crate::compaction::Condenser;

use super::hybrid::{tokenise, Candidate, HybridRetrievalConfig, HybridScorer};
use super::{
    CheckpointReason, CompactionCheckpoint, ContextEngine, ContextEngineDescriptor, ContextError,
    ContextWindow, TokenBudget,
};

/// Fallback tokenizer name used when no model-specific tokenizer is configured.
const DEFAULT_TOKENIZER: &str = "cl100k_base";

/// Select the tiktoken encoding name for a given model ID.
///
/// Returns the appropriate BPE encoding name based on known model families.
/// Falls back to `cl100k_base` (GPT-4 / GPT-3.5 family) for unrecognised models.
fn tokenizer_for_model(model_id: &str) -> &'static str {
    if model_id.starts_with("o200k") || model_id.contains("gpt-4o") || model_id.contains("o1") {
        "o200k_base"
    } else if model_id.starts_with("gpt-2") || model_id.contains("davinci") || model_id.contains("curie") {
        "r50k_base"
    } else {
        // cl100k_base covers GPT-4, GPT-3.5-turbo, Claude (token-count approximation),
        // and all non-OpenAI models where tiktoken is used for budgeting only.
        DEFAULT_TOKENIZER
    }
}

/// Pipeline-based context engine.
pub struct ContextPipeline {
    messages: Vec<serde_json::Value>,
    condensers: Vec<Box<dyn Condenser>>,
    /// Session key threaded in from the calling context for checkpoint attribution.
    session_key: String,
    /// Active model ID used to select the appropriate tokenizer.
    model_id: String,
    /// Optional hybrid retrieval configuration. When `None` (default) or when
    /// [`HybridRetrievalConfig::hybrid_retrieval`] is `false`, `assemble` falls
    /// back to the existing single-pass pass-through.
    hybrid_config: Option<HybridRetrievalConfig>,
    /// Pre-computed query embedding for the current turn.
    ///
    /// Populated by [`ContextPipeline::set_query_embedding`] just before
    /// `assemble` runs so the [`HybridScorer`] can use real cosine similarity
    /// instead of the zero-vector fallback. `None` disables the vector
    /// component (lexical-only scoring).
    query_embedding: RwLock<Option<Vec<f32>>>,
}

impl ContextPipeline {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            condensers: Vec::new(),
            session_key: String::new(),
            model_id: String::new(),
            hybrid_config: None,
            query_embedding: RwLock::new(None),
        }
    }

    /// Install the query embedding used by the [`HybridScorer`] on the next
    /// `assemble` call. Passing `None` clears the embedding and reverts to
    /// lexical-only scoring.
    ///
    /// Typically called by [`crate::context_engine::ContextEnricher`]
    /// immediately before the turn loop runs `assemble`. Storing the vector
    /// on the pipeline keeps the `ContextEngine` trait dependency-free while
    /// still letting the scorer consume a real embedding.
    pub fn set_query_embedding(&self, embedding: Option<Vec<f32>>) {
        if let Ok(mut guard) = self.query_embedding.write() {
            *guard = embedding;
        }
    }

    /// Set the session key for checkpoint attribution.
    pub fn with_session_key(mut self, session_key: impl Into<String>) -> Self {
        self.session_key = session_key.into();
        self
    }

    /// Set the model ID used for tokenizer selection.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Add a condenser to the pipeline; condensers are applied in insertion order.
    pub fn with_condenser(mut self, condenser: Box<dyn Condenser>) -> Self {
        self.condensers.push(condenser);
        self
    }

    /// Enable hybrid retrieval scoring for `assemble`.
    ///
    /// When the config's `hybrid_retrieval` flag is `true`, `assemble` will
    /// score every ingested message as a [`Candidate`] against the last user
    /// turn and reorder the window by descending hybrid score. When `false`
    /// or when this method is never called, `assemble` preserves insertion
    /// order (existing single-pass behavior).
    pub fn with_hybrid_config(mut self, config: HybridRetrievalConfig) -> Self {
        self.hybrid_config = Some(config);
        self
    }
}

/// Adapt an ingested message (`{role, content, ...}`) into a scorer
/// [`Candidate`]. Missing or non-string `content` yields empty tokens;
/// missing `created_at` defaults to `now` so the recency decay stays bounded.
/// Embeddings are not carried on these messages, so we pass an empty vector
/// — the scorer degrades to index + recency in that case.
fn message_to_candidate(index: usize, msg: &serde_json::Value, fallback_now: chrono::DateTime<Utc>) -> Candidate {
    let text = msg
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let created_at = msg
        .get("created_at")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or(fallback_now);
    let embedding = msg
        .get("embedding")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|n| n.as_f64().map(|f| f as f32))
                .collect::<Vec<f32>>()
        })
        .unwrap_or_default();
    Candidate {
        id: msg
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("msg-{index}")),
        tokens: tokenise(text),
        embedding,
        created_at,
    }
}

/// Derive query tokens from the last message whose role is `user`. Returns
/// empty tokens when no user turn exists — in which case the scorer's index
/// component is zero for every candidate and recency dominates.
fn derive_query_tokens(messages: &[serde_json::Value]) -> Vec<String> {
    for msg in messages.iter().rev() {
        if msg.get("role").and_then(|v| v.as_str()) == Some("user") {
            let text = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
            return tokenise(text);
        }
    }
    Vec::new()
}

impl Default for ContextPipeline {
    fn default() -> Self {
        Self::new()
    }
}

static TOKENIZER_CL100K: Lazy<CoreBPE> =
    Lazy::new(|| cl100k_base().expect("cl100k_base encoding must be available"));

fn estimate_tokens(messages: &[serde_json::Value], model_id: &str) -> u32 {
    // Currently tiktoken_rs only ships cl100k_base and o200k_base; for other
    // encoding names we fall back to cl100k_base (used for budgeting only).
    let _ = tokenizer_for_model(model_id); // future: select encoder by name
    messages
        .iter()
        .map(|m| TOKENIZER_CL100K.encode_ordinary(&m.to_string()).len() as u32)
        .sum()
}

#[async_trait]
impl ContextEngine for ContextPipeline {
    async fn ingest(&mut self, msg: serde_json::Value) -> Result<(), ContextError> {
        self.messages.push(msg);
        Ok(())
    }

    async fn assemble(&self, budget: TokenBudget) -> Result<ContextWindow, ContextError> {
        let estimated = estimate_tokens(&self.messages, &self.model_id);

        if estimated > budget.max_tokens {
            return Err(ContextError::BudgetExceeded {
                limit: budget.max_tokens,
                actual: estimated,
            });
        }

        // Fast path: single-pass behavior preserved when hybrid retrieval is
        // disabled or no config was provided.
        let hybrid_enabled = self
            .hybrid_config
            .as_ref()
            .map(|c| c.hybrid_retrieval)
            .unwrap_or(false);
        if !hybrid_enabled || self.messages.is_empty() {
            return Ok(ContextWindow {
                messages: self.messages.clone(),
                estimated_tokens: estimated,
            });
        }

        // Hybrid path: score every ingested message as a candidate against
        // the last user turn and reorder by descending combined score.
        let config = self
            .hybrid_config
            .as_ref()
            .expect("hybrid_enabled implies config present")
            .clone();
        let now = Utc::now();
        let candidates: Vec<Candidate> = self
            .messages
            .iter()
            .enumerate()
            .map(|(i, m)| message_to_candidate(i, m, now))
            .collect();
        let query_tokens = derive_query_tokens(&self.messages);
        // Prefer the embedding threaded in from ContextEnricher (sera-0yqq)
        // so the scorer can use real cosine similarity. When none is set —
        // disabled flag, backend failure, or tests — fall back to the empty
        // vector, which the scorer treats as "vector component 0.0" (lexical
        // + recency only). This preserves the historical fallback path.
        let query_embedding: Vec<f32> = self
            .query_embedding
            .read()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_default();
        let scorer = HybridScorer::new(config, query_tokens, query_embedding, &candidates, now);
        let ranked = scorer.rank();

        // Map ranked candidate ids back to ingested messages. `id` is either
        // carried through from the message or a deterministic `msg-{index}`
        // fallback, so we can round-trip the ordering without cloning state.
        let mut reordered: Vec<serde_json::Value> = Vec::with_capacity(self.messages.len());
        for scored in ranked {
            // The candidate id encodes its original index when no `id` was
            // supplied on the message; otherwise we match by id against the
            // source messages. Either way we preserve the original JSON.
            if let Some(stripped) = scored.candidate.id.strip_prefix("msg-")
                && let Ok(idx) = stripped.parse::<usize>()
                && let Some(m) = self.messages.get(idx)
            {
                reordered.push(m.clone());
                continue;
            }
            // Fallback: find by matching `id` field.
            if let Some(m) = self.messages.iter().find(|m| {
                m.get("id").and_then(|v| v.as_str()) == Some(scored.candidate.id.as_str())
            }) {
                reordered.push(m.clone());
            }
        }

        Ok(ContextWindow {
            messages: reordered,
            estimated_tokens: estimated,
        })
    }

    async fn compact(
        &mut self,
        trigger: CheckpointReason,
    ) -> Result<CompactionCheckpoint, ContextError> {
        let tokens_before = estimate_tokens(&self.messages, &self.model_id);

        // Run each condenser in order, passing the output of one into the next.
        let mut messages = self.messages.clone();
        for condenser in &self.condensers {
            messages = condenser.condense(messages).await;
        }
        self.messages = messages;

        let tokens_after = estimate_tokens(&self.messages, &self.model_id);

        // Emit a warning when session_key was not set so operators can diagnose
        // checkpoint records that lack attribution.
        if self.session_key.is_empty() {
            tracing::warn!(
                "CompactionCheckpoint produced with empty session_key — \
                 call with_session_key() when constructing ContextPipeline"
            );
        }

        Ok(CompactionCheckpoint {
            checkpoint_id: uuid::Uuid::new_v4(),
            session_key: self.session_key.clone(),
            reason: trigger,
            tokens_before,
            tokens_after,
            summary: None,
            created_at: chrono::Utc::now(),
        })
    }

    async fn maintain(&mut self) -> Result<(), ContextError> {
        Ok(())
    }

    fn describe(&self) -> ContextEngineDescriptor {
        ContextEngineDescriptor {
            name: "pipeline".to_string(),
            version: "0.1.0".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::compaction::condensers::RecentEventsCondenser;

    use super::*;

    #[tokio::test]
    async fn compact_with_sliding_window_reduces_messages() {
        // Keep only the 3 most recent messages.
        let mut engine = ContextPipeline::new()
            .with_condenser(Box::new(RecentEventsCondenser::new(3)));

        // Ingest 8 messages.
        for i in 0u8..8 {
            engine
                .ingest(json!({ "role": "user", "content": format!("msg {i}") }))
                .await
                .unwrap();
        }

        assert_eq!(engine.messages.len(), 8);

        let checkpoint = engine.compact(CheckpointReason::Manual).await.unwrap();

        // Only 3 messages should remain.
        assert_eq!(engine.messages.len(), 3);
        // The last ingested message is msg 7.
        assert_eq!(engine.messages[2]["content"], "msg 7");
        // Tokens were reduced.
        assert!(checkpoint.tokens_after <= checkpoint.tokens_before);
    }

    #[tokio::test]
    async fn compact_no_condensers_is_identity() {
        let mut engine = ContextPipeline::new();

        for i in 0u8..5 {
            engine
                .ingest(json!({ "role": "user", "content": format!("msg {i}") }))
                .await
                .unwrap();
        }

        let _checkpoint = engine.compact(CheckpointReason::AutoThreshold).await.unwrap();

        // Without any condenser the messages are unchanged.
        assert_eq!(engine.messages.len(), 5);
    }

    fn budget() -> TokenBudget {
        TokenBudget {
            max_tokens: 1_000_000,
            reserved_for_output: 0,
        }
    }

    #[tokio::test]
    async fn assemble_without_hybrid_config_preserves_order() {
        // No hybrid config → existing single-pass behavior.
        let mut engine = ContextPipeline::new();
        for i in 0u8..4 {
            engine
                .ingest(json!({ "role": "user", "content": format!("msg {i}") }))
                .await
                .unwrap();
        }
        let window = engine.assemble(budget()).await.unwrap();
        assert_eq!(window.messages.len(), 4);
        for (i, m) in window.messages.iter().enumerate() {
            assert_eq!(m["content"], format!("msg {i}"));
        }
    }

    #[tokio::test]
    async fn assemble_with_hybrid_disabled_flag_preserves_order() {
        // hybrid_config set but flag is false → single-pass path.
        let cfg = HybridRetrievalConfig {
            hybrid_retrieval: false,
            ..Default::default()
        };
        let mut engine = ContextPipeline::new().with_hybrid_config(cfg);
        engine.ingest(json!({ "role": "user", "content": "rust async" })).await.unwrap();
        engine.ingest(json!({ "role": "assistant", "content": "python sync" })).await.unwrap();
        engine.ingest(json!({ "role": "user", "content": "rust async again" })).await.unwrap();

        let window = engine.assemble(budget()).await.unwrap();
        assert_eq!(window.messages.len(), 3);
        assert_eq!(window.messages[0]["content"], "rust async");
        assert_eq!(window.messages[1]["content"], "python sync");
        assert_eq!(window.messages[2]["content"], "rust async again");
    }

    #[tokio::test]
    async fn assemble_with_hybrid_enabled_ranks_by_score() {
        // Pure BM25 weighting → the message matching the query tokens wins
        // regardless of insertion order.
        let cfg = HybridRetrievalConfig {
            index_weight: 1.0,
            vector_weight: 0.0,
            recency_weight: 0.0,
            hybrid_retrieval: true,
            ..Default::default()
        };
        let mut engine = ContextPipeline::new().with_hybrid_config(cfg);
        // Ingest order: non-match first, match second, then the user query.
        engine
            .ingest(json!({ "id": "no-match", "role": "assistant", "content": "python sync programming" }))
            .await
            .unwrap();
        engine
            .ingest(json!({ "id": "match", "role": "assistant", "content": "rust async programming" }))
            .await
            .unwrap();
        engine
            .ingest(json!({ "id": "query", "role": "user", "content": "rust async" }))
            .await
            .unwrap();

        let window = engine.assemble(budget()).await.unwrap();
        assert_eq!(window.messages.len(), 3);
        // The matching candidate must outrank the non-matching one. (The user
        // query turn itself also contains the tokens, so we assert relative
        // order rather than a specific winner.)
        let pos_match = window
            .messages
            .iter()
            .position(|m| m["id"] == "match")
            .unwrap();
        let pos_no_match = window
            .messages
            .iter()
            .position(|m| m["id"] == "no-match")
            .unwrap();
        assert!(
            pos_match < pos_no_match,
            "expected 'match' to outrank 'no-match', got window = {:?}",
            window.messages
        );
    }

    #[tokio::test]
    async fn assemble_with_empty_query_falls_back_to_recency() {
        // No user turn → query tokens are empty, BM25 is zero for all docs.
        // With all weight on recency the newest created_at should win.
        let cfg = HybridRetrievalConfig {
            index_weight: 0.0,
            vector_weight: 0.0,
            recency_weight: 1.0,
            hybrid_retrieval: true,
            ..Default::default()
        };
        let mut engine = ContextPipeline::new().with_hybrid_config(cfg);
        let now = chrono::Utc::now();
        engine
            .ingest(json!({
                "id": "old",
                "role": "assistant",
                "content": "older entry",
                "created_at": (now - chrono::Duration::days(7)).to_rfc3339(),
            }))
            .await
            .unwrap();
        engine
            .ingest(json!({
                "id": "fresh",
                "role": "assistant",
                "content": "fresh entry",
                "created_at": now.to_rfc3339(),
            }))
            .await
            .unwrap();

        let window = engine.assemble(budget()).await.unwrap();
        assert_eq!(window.messages.len(), 2);
        assert_eq!(window.messages[0]["id"], "fresh");
    }

    #[tokio::test]
    async fn assemble_with_zero_vectors_does_not_panic() {
        // Explicit zero-vector embeddings on candidates → vector component
        // is clamped to 0.0, scorer uses index + recency only and produces
        // a deterministic ranking without panicking.
        let cfg = HybridRetrievalConfig::default();
        let mut engine = ContextPipeline::new().with_hybrid_config(cfg);
        engine
            .ingest(json!({
                "id": "a",
                "role": "assistant",
                "content": "rust async",
                "embedding": [0.0, 0.0, 0.0],
            }))
            .await
            .unwrap();
        engine
            .ingest(json!({
                "id": "b",
                "role": "assistant",
                "content": "python sync",
                "embedding": [0.0, 0.0, 0.0],
            }))
            .await
            .unwrap();
        engine
            .ingest(json!({ "role": "user", "content": "rust async" }))
            .await
            .unwrap();

        let window = engine.assemble(budget()).await.unwrap();
        assert_eq!(window.messages.len(), 3);
    }
}
