//! SQ/EQ envelope types — gateway-local extension types.
//!
//! Shared protocol types (Submission, Event, Op, W3cTraceContext, etc.) live in
//! `sera_types::envelope` so both gateway and runtime can reference them without
//! a circular dependency. This module re-exports those and adds gateway-specific
//! types (GenerationMarker, EventContext, DedupeKey, QueueMode, WorkerFailureKind).

pub use sera_types::envelope::*;

use serde::{Deserialize, Serialize};

/// Event context attached to every event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventContext {
    pub agent_id: String,
    pub session_key: String,
    pub sender: String,
    pub recipient: String,
    pub principal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_key: Option<String>,
    pub generation: GenerationMarker,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

/// Generation marker for binary identity tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationMarker {
    pub label: String,
    pub binary_identity: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

impl GenerationMarker {
    /// Create a generation marker from the current build.
    pub fn current() -> Self {
        Self {
            label: env!("CARGO_PKG_VERSION").to_string(),
            binary_identity: format!("sera-gateway@{}", env!("CARGO_PKG_VERSION")),
            started_at: chrono::Utc::now(),
        }
    }
}

/// Deduplication key for submissions.
#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct DedupeKey {
    pub channel: String,
    pub account: String,
    pub peer: String,
    pub session_key: String,
    pub message_id: String,
}

/// Queue processing mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueMode {
    Collect,
    Followup,
    Steer,
    SteerBacklog,
    Interrupt,
}

/// Worker failure classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerFailureKind {
    TrustGate,
    PromptDelivery,
    Protocol,
    Provider,
}
