//! ContextPipeline — wraps the old ContextPipeline as a ContextEngine impl.

use async_trait::async_trait;

use super::{
    CheckpointReason, CompactionCheckpoint, ContextEngine, ContextEngineDescriptor, ContextError,
    ContextWindow, TokenBudget,
};

/// Pipeline-based context engine.
pub struct ContextPipeline {
    messages: Vec<serde_json::Value>,
}

impl ContextPipeline {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }
}

impl Default for ContextPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContextEngine for ContextPipeline {
    async fn ingest(&mut self, msg: serde_json::Value) -> Result<(), ContextError> {
        self.messages.push(msg);
        Ok(())
    }

    async fn assemble(&self, budget: TokenBudget) -> Result<ContextWindow, ContextError> {
        // Simple char/4 token estimation for P0
        let estimated: u32 = self
            .messages
            .iter()
            .map(|m| m.to_string().len() as u32 / 4)
            .sum();

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
        let tokens_before: u32 = self
            .messages
            .iter()
            .map(|m| m.to_string().len() as u32 / 4)
            .sum();

        // Simple compaction: keep first and last quarter
        let len = self.messages.len();
        if len > 4 {
            let keep_end = len / 4;
            let mut compacted = vec![self.messages[0].clone()];
            compacted.extend_from_slice(&self.messages[len - keep_end..]);
            self.messages = compacted;
        }

        let tokens_after: u32 = self
            .messages
            .iter()
            .map(|m| m.to_string().len() as u32 / 4)
            .sum();

        Ok(CompactionCheckpoint {
            checkpoint_id: uuid::Uuid::new_v4(),
            session_key: String::new(),
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
