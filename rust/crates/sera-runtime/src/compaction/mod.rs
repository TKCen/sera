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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compaction::condensers::{NoOpCondenser, RecentEventsCondenser};

    fn make_messages(n: usize) -> Vec<serde_json::Value> {
        (0..n)
            .map(|i| {
                serde_json::json!({"role": if i % 2 == 0 { "user" } else { "assistant" }, "content": format!("msg {}", i)})
            })
            .collect()
    }

    #[tokio::test]
    async fn pipeline_empty_pipeline_is_passthrough() {
        let pipeline = PipelineCondenser::new();
        let msgs = make_messages(5);
        let result = pipeline.run(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn pipeline_empty_input_empty_output() {
        let pipeline = PipelineCondenser::new();
        let result = pipeline.run(vec![]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn pipeline_single_noop_condenser_passthrough() {
        let mut pipeline = PipelineCondenser::new();
        pipeline.add(Box::new(NoOpCondenser));
        let msgs = make_messages(4);
        let result = pipeline.run(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn pipeline_single_recent_events_condenser() {
        let mut pipeline = PipelineCondenser::new();
        pipeline.add(Box::new(RecentEventsCondenser::new(3)));
        let msgs = make_messages(7);
        let result = pipeline.run(msgs.clone()).await;
        assert_eq!(result.len(), 3);
        assert_eq!(result, msgs[4..]);
    }

    #[tokio::test]
    async fn pipeline_condensers_applied_in_order() {
        // First keep 5, then keep 3 — net effect: keep 3
        let mut pipeline = PipelineCondenser::new();
        pipeline.add(Box::new(RecentEventsCondenser::new(5)));
        pipeline.add(Box::new(RecentEventsCondenser::new(3)));
        let msgs = make_messages(10);
        let result = pipeline.run(msgs.clone()).await;
        assert_eq!(result.len(), 3);
        assert_eq!(result, msgs[7..]);
    }

    #[tokio::test]
    async fn pipeline_noop_then_recent_events() {
        let mut pipeline = PipelineCondenser::new();
        pipeline.add(Box::new(NoOpCondenser));
        pipeline.add(Box::new(RecentEventsCondenser::new(2)));
        let msgs = make_messages(6);
        let result = pipeline.run(msgs.clone()).await;
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn pipeline_default_is_empty_pipeline() {
        let pipeline = PipelineCondenser::default();
        let msgs = make_messages(3);
        let result = pipeline.run(msgs.clone()).await;
        assert_eq!(result, msgs);
    }
}
