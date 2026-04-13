//! AgentHarness trait and supporting types — shared between gateway and runtime.
//!
//! The gateway is a thin dispatcher; harness implementations live in the runtime
//! and must not depend on the gateway crate. Both reference this shared definition.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio_stream::Stream;

use crate::envelope::{Event, Submission};

/// Unique identifier for an agent (type alias for ergonomics).
pub type AgentId = String;

/// Agent harness trait — implemented by each runtime backend.
#[async_trait]
pub trait AgentHarness: Send + Sync {
    /// Process a submission and return an event stream.
    async fn handle(
        &self,
        submission: Submission,
    ) -> Result<Pin<Box<dyn Stream<Item = Event> + Send>>, HarnessError>;

    /// Report whether this harness is healthy.
    async fn health(&self) -> bool;

    /// Shut down the harness gracefully.
    async fn shutdown(&self) -> Result<(), HarnessError>;
}

/// Errors from harness operations.
#[derive(Debug, thiserror::Error)]
pub enum HarnessError {
    #[error("agent not found: {0}")]
    AgentNotFound(String),
    #[error("harness failed: {0}")]
    Failed(String),
    #[error("harness shutting down")]
    ShuttingDown,
}

/// Registry of active agent harnesses.
pub type HarnessRegistry = Arc<RwLock<HashMap<AgentId, Box<dyn AgentHarness>>>>;

/// Create a new empty harness registry.
pub fn new_harness_registry() -> HarnessRegistry {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Dispatch a submission to the correct harness.
pub async fn dispatch(
    submission: &Submission,
    agent_id: &str,
    registry: &HarnessRegistry,
) -> Result<Pin<Box<dyn Stream<Item = Event> + Send>>, HarnessError> {
    let reg = registry.read().await;
    let harness = reg
        .get(agent_id)
        .ok_or_else(|| HarnessError::AgentNotFound(agent_id.to_string()))?;
    harness.handle(submission.clone()).await
}

// ── Plugin registry ─────────────────────────────────────────────────────────

/// Plugin event for the plugin bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEvent {
    pub event_id: uuid::Uuid,
    pub event_type: String,
    pub correlation_id: uuid::Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub circle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub entity_id: String,
    pub entity_type: String,
    pub payload: serde_json::Value,
    pub actor_type: String,
    pub actor_id: String,
}

/// Plugin registry for plugin hooks.
pub type PluginRegistry = Arc<RwLock<Vec<PluginRegistration>>>;

/// A registered plugin.
#[derive(Debug, Clone)]
pub struct PluginRegistration {
    pub name: String,
    pub namespace: String,
}

/// Create a new empty plugin registry.
pub fn new_plugin_registry() -> PluginRegistry {
    Arc::new(RwLock::new(Vec::new()))
}

/// Validate that a plugin event namespace matches the registered plugin.
pub fn validate_plugin_event_namespace(
    plugin: &PluginRegistration,
    event: &PluginEvent,
) -> bool {
    event.event_type.starts_with(&plugin.namespace)
}
