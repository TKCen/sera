//! Context engine — pluggable context assembly and compaction.

pub mod hybrid;
pub mod kvcache;
pub mod pipeline;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Token budget for context assembly.
#[derive(Debug, Clone)]
pub struct TokenBudget {
    pub max_tokens: u32,
    pub reserved_for_output: u32,
}

/// Assembled context window.
#[derive(Debug, Clone)]
pub struct ContextWindow {
    pub messages: Vec<serde_json::Value>,
    pub estimated_tokens: u32,
}

/// Compaction checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionCheckpoint {
    pub checkpoint_id: uuid::Uuid,
    pub session_key: String,
    pub reason: CheckpointReason,
    pub tokens_before: u32,
    pub tokens_after: u32,
    pub summary: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Reason for creating a compaction checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointReason {
    Manual,
    AutoThreshold,
    OverflowRetry,
    TimeoutRetry,
}

/// Maximum compaction checkpoints per session.
pub const MAX_COMPACTION_CHECKPOINTS_PER_SESSION: u32 = 25;

/// Context engine descriptor.
#[derive(Debug, Clone)]
pub struct ContextEngineDescriptor {
    pub name: String,
    pub version: String,
}

/// Context errors.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("context error: {0}")]
    Internal(String),
    #[error("token budget exceeded: {limit} max, {actual} actual")]
    BudgetExceeded { limit: u32, actual: u32 },
}

/// Pluggable context engine trait — orthogonal to AgentRuntime.
#[async_trait]
pub trait ContextEngine: Send + Sync {
    async fn ingest(&mut self, msg: serde_json::Value) -> Result<(), ContextError>;
    async fn assemble(&self, budget: TokenBudget) -> Result<ContextWindow, ContextError>;
    async fn compact(
        &mut self,
        trigger: CheckpointReason,
    ) -> Result<CompactionCheckpoint, ContextError>;
    async fn maintain(&mut self) -> Result<(), ContextError>;
    fn describe(&self) -> ContextEngineDescriptor;
}
