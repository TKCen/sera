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
    #[error("harness error: {0}")]
    Internal(String),
    #[error("not supported: {0}")]
    NotSupported(String),
}

impl From<HarnessError> for sera_errors::SeraError {
    fn from(err: HarnessError) -> Self {
        use sera_errors::SeraErrorCode;
        let code = match &err {
            HarnessError::AgentNotFound(_) => SeraErrorCode::NotFound,
            HarnessError::Failed(_) => SeraErrorCode::Internal,
            HarnessError::ShuttingDown => SeraErrorCode::Unavailable,
            HarnessError::Internal(_) => SeraErrorCode::Internal,
            HarnessError::NotSupported(_) => SeraErrorCode::NotImplemented,
        };
        sera_errors::SeraError::with_source(code, err.to_string(), err)
    }
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
pub type PluginRegistry = Arc<RwLock<Vec<PluginHookEntry>>>;

/// A registered plugin.
#[derive(Debug, Clone)]
pub struct PluginHookEntry {
    pub name: String,
    pub namespace: String,
}

/// Create a new empty plugin registry.
pub fn new_plugin_registry() -> PluginRegistry {
    Arc::new(RwLock::new(Vec::new()))
}

/// Validate that a plugin event namespace matches the registered plugin.
pub fn validate_plugin_event_namespace(plugin: &PluginHookEntry, event: &PluginEvent) -> bool {
    event.event_type.starts_with(&plugin.namespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plugin_event(event_type: &str) -> PluginEvent {
        PluginEvent {
            event_id: uuid::Uuid::new_v4(),
            event_type: event_type.to_string(),
            correlation_id: uuid::Uuid::new_v4(),
            circle_id: None,
            session_key: None,
            occurred_at: chrono::Utc::now(),
            entity_id: "entity-1".to_string(),
            entity_type: "agent".to_string(),
            payload: serde_json::json!({}),
            actor_type: "user".to_string(),
            actor_id: "user-1".to_string(),
        }
    }

    #[test]
    fn harness_error_display_agent_not_found() {
        let err = HarnessError::AgentNotFound("agent-xyz".to_string());
        assert_eq!(err.to_string(), "agent not found: agent-xyz");
    }

    #[test]
    fn harness_error_display_failed() {
        let err = HarnessError::Failed("connection refused".to_string());
        assert_eq!(err.to_string(), "harness failed: connection refused");
    }

    #[test]
    fn harness_error_display_shutting_down() {
        let err = HarnessError::ShuttingDown;
        assert_eq!(err.to_string(), "harness shutting down");
    }

    #[test]
    fn new_harness_registry_starts_empty() {
        let registry = new_harness_registry();
        // The registry is an Arc<RwLock<HashMap>>; we can get a sync read via blocking.
        let guard = registry.blocking_read();
        assert!(guard.is_empty());
    }

    #[test]
    fn new_plugin_registry_starts_empty() {
        let registry = new_plugin_registry();
        let guard = registry.blocking_read();
        assert!(guard.is_empty());
    }

    #[test]
    fn validate_plugin_event_namespace_matching_prefix() {
        let plugin = PluginHookEntry {
            name: "my-plugin".to_string(),
            namespace: "my_plugin.".to_string(),
        };
        let event = make_plugin_event("my_plugin.tool_called");
        assert!(validate_plugin_event_namespace(&plugin, &event));
    }

    #[test]
    fn validate_plugin_event_namespace_exact_match() {
        let plugin = PluginHookEntry {
            name: "my-plugin".to_string(),
            namespace: "my_plugin".to_string(),
        };
        let event = make_plugin_event("my_plugin");
        assert!(validate_plugin_event_namespace(&plugin, &event));
    }

    #[test]
    fn validate_plugin_event_namespace_no_match() {
        let plugin = PluginHookEntry {
            name: "my-plugin".to_string(),
            namespace: "my_plugin.".to_string(),
        };
        let event = make_plugin_event("other_plugin.something");
        assert!(!validate_plugin_event_namespace(&plugin, &event));
    }

    #[test]
    fn validate_plugin_event_namespace_empty_namespace_matches_all() {
        let plugin = PluginHookEntry {
            name: "catch-all".to_string(),
            namespace: String::new(),
        };
        let event = make_plugin_event("anything.at.all");
        // An empty namespace prefix matches every event_type.
        assert!(validate_plugin_event_namespace(&plugin, &event));
    }

    #[test]
    fn plugin_event_serde_roundtrip() {
        let event = PluginEvent {
            event_id: uuid::Uuid::nil(),
            event_type: "sera.tool.bash.executed".to_string(),
            correlation_id: uuid::Uuid::nil(),
            circle_id: Some("circle-1".to_string()),
            session_key: Some("sess-abc".to_string()),
            occurred_at: chrono::DateTime::parse_from_rfc3339("2026-04-17T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
            entity_id: "agent-42".to_string(),
            entity_type: "agent".to_string(),
            payload: serde_json::json!({"tool": "bash", "exit_code": 0}),
            actor_type: "agent".to_string(),
            actor_id: "agent-42".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: PluginEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event_id, event.event_id);
        assert_eq!(parsed.event_type, "sera.tool.bash.executed");
        assert_eq!(parsed.circle_id.as_deref(), Some("circle-1"));
        assert_eq!(parsed.session_key.as_deref(), Some("sess-abc"));
        assert_eq!(parsed.entity_id, "agent-42");
    }

    #[test]
    fn plugin_event_optional_fields_omitted_in_json() {
        let event = make_plugin_event("test.event");
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("circle_id"));
        assert!(!json.contains("session_key"));
    }
}
