//! KvCachePipeline — stub ContextEngine for KV-cache-aware reordering.
//! Phase 0 no-op implementation.

use async_trait::async_trait;

use super::{
    CheckpointReason, CompactionCheckpoint, ContextEngine, ContextEngineDescriptor, ContextError,
    ContextWindow, TokenBudget,
};

pub struct KvCachePipeline {
    messages: Vec<serde_json::Value>,
}

impl KvCachePipeline {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }
}

impl Default for KvCachePipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContextEngine for KvCachePipeline {
    async fn ingest(&mut self, msg: serde_json::Value) -> Result<(), ContextError> {
        self.messages.push(msg);
        Ok(())
    }

    async fn assemble(&self, _budget: TokenBudget) -> Result<ContextWindow, ContextError> {
        let estimated = self
            .messages
            .iter()
            .map(|m| m.to_string().len() as u32 / 4)
            .sum();
        Ok(ContextWindow {
            messages: self.messages.clone(),
            estimated_tokens: estimated,
        })
    }

    async fn compact(
        &mut self,
        trigger: CheckpointReason,
    ) -> Result<CompactionCheckpoint, ContextError> {
        Ok(CompactionCheckpoint {
            checkpoint_id: uuid::Uuid::new_v4(),
            session_key: String::new(),
            reason: trigger,
            tokens_before: 0,
            tokens_after: 0,
            summary: None,
            created_at: chrono::Utc::now(),
        })
    }

    async fn maintain(&mut self) -> Result<(), ContextError> {
        Ok(())
    }

    fn describe(&self) -> ContextEngineDescriptor {
        ContextEngineDescriptor {
            name: "kvcache".to_string(),
            version: "0.1.0".to_string(),
        }
    }
}
