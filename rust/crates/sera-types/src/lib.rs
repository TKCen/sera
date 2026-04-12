//! SERA Domain Types — shared types matching the BYOH contract schemas
//! and the full sera-core domain model.

pub mod evolution;
pub mod versioning;
pub mod content_block;
pub mod agent;
pub mod connector;
pub mod runtime;
pub mod audit;
pub mod capability;
pub mod chat;
pub mod config_manifest;
pub mod event;
pub mod hook;
pub mod intercom;
pub mod manifest;
pub mod memory;
pub mod model;
pub mod metering;
pub mod observability;
pub mod policy;
pub mod principal;
pub mod queue;
pub mod sandbox;
pub mod secrets;
pub mod session;
pub mod skill;
pub mod tool;

pub use evolution::*;
pub use versioning::BuildIdentity;
pub use content_block::{ContentBlock, ConversationMessage, ConversationRole};

use serde::{Deserialize, Serialize};

/// Lifecycle mode for agent instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleMode {
    Persistent,
    Ephemeral,
}

/// Task input sent to agent containers via stdin.
/// Matches schemas/byoh-task-input.schema.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInput {
    #[serde(rename = "taskId")]
    pub task_id: String,
    pub task: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

/// Task output written to stdout by agent containers.
/// Matches schemas/byoh-task-output.schema.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutput {
    #[serde(rename = "taskId")]
    pub task_id: String,
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Health response from GET /health endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub ready: bool,
    pub busy: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_input_deserialize() {
        let json = r#"{"taskId":"task-123","task":"Hello","context":"test"}"#;
        let input: TaskInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.task_id, "task-123");
        assert_eq!(input.task, "Hello");
        assert_eq!(input.context.as_deref(), Some("test"));
    }

    #[test]
    fn task_output_serialize() {
        let output = TaskOutput {
            task_id: "task-123".to_string(),
            result: Some("Done".to_string()),
            error: None,
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"taskId\":\"task-123\""));
        assert!(json.contains("\"result\":\"Done\""));
        assert!(!json.contains("error"));
    }

    #[test]
    fn task_input_without_context() {
        let json = r#"{"taskId":"t1","task":"do something"}"#;
        let input: TaskInput = serde_json::from_str(json).unwrap();
        assert!(input.context.is_none());
    }

    #[test]
    fn lifecycle_mode_serialize() {
        let mode = LifecycleMode::Persistent;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "\"persistent\"");

        let parsed: LifecycleMode = serde_json::from_str("\"ephemeral\"").unwrap();
        assert_eq!(parsed, LifecycleMode::Ephemeral);
    }
}
