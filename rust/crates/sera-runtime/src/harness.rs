//! Agent harness — coordinates submission processing and event emission.

use serde::{Deserialize, Serialize};

/// Harness support level for a given context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "support", rename_all = "snake_case")]
pub enum HarnessSupport {
    Supported,
    Unsupported { reason: String },
    RequiresUpgrade { required_tier: String },
}

/// Context for checking harness support.
#[derive(Debug, Clone)]
pub struct HarnessSupportContext {
    pub agent_id: String,
    pub tier: String,
}

/// Parameters for compaction.
#[derive(Debug, Clone)]
pub struct CompactionParams {
    pub session_key: String,
    pub trigger: CompactionTrigger,
}

/// Compaction triggers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionTrigger {
    Manual,
    AutoThreshold,
    OverflowRetry,
    TimeoutRetry,
}

/// Result of compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionResult {
    pub tokens_before: u32,
    pub tokens_after: u32,
    pub messages_removed: u32,
}

/// Parameters for reset.
#[derive(Debug, Clone)]
pub struct ResetParams {
    pub session_key: String,
}

/// Harness errors.
#[derive(Debug, thiserror::Error)]
pub enum HarnessError {
    #[error("harness error: {0}")]
    Internal(String),
    #[error("not supported: {0}")]
    NotSupported(String),
}

/// The default harness implementation.
pub struct DefaultHarness;

impl DefaultHarness {
    pub fn new() -> Self {
        Self
    }

    pub fn supports(&self, _ctx: &HarnessSupportContext) -> HarnessSupport {
        HarnessSupport::Supported
    }
}

impl Default for DefaultHarness {
    fn default() -> Self {
        Self::new()
    }
}
