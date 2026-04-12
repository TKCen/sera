//! Compaction module — condenser pipeline for context window management.

pub mod condensers;

use async_trait::async_trait;

/// Condenser trait — transforms a message list to reduce token count.
#[async_trait]
pub trait Condenser: Send + Sync {
    fn name(&self) -> &str;
    async fn condense(&self, messages: Vec<serde_json::Value>) -> Vec<serde_json::Value>;
}

/// Pipeline of condensers applied in order.
pub struct PipelineCondenser {
    condensers: Vec<Box<dyn Condenser>>,
}

impl PipelineCondenser {
    pub fn new() -> Self {
        Self {
            condensers: Vec::new(),
        }
    }

    pub fn add(&mut self, condenser: Box<dyn Condenser>) {
        self.condensers.push(condenser);
    }

    pub async fn run(&self, mut messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        for c in &self.condensers {
            messages = c.condense(messages).await;
        }
        messages
    }
}

impl Default for PipelineCondenser {
    fn default() -> Self {
        Self::new()
    }
}
