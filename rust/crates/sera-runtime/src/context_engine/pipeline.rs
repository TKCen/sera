//! ContextPipeline — wraps the old ContextPipeline as a ContextEngine impl.

use async_trait::async_trait;
use once_cell::sync::Lazy;
use tiktoken_rs::cl100k_base;
use tiktoken_rs::CoreBPE;

use crate::compaction::Condenser;

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
}

impl ContextPipeline {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            condensers: Vec::new(),
            session_key: String::new(),
            model_id: String::new(),
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

        Ok(ContextWindow {
            messages: self.messages.clone(),
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
}
