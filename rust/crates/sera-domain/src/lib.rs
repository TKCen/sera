//! SERA Domain Types — shared types matching the BYOH contract schemas.

use serde::{Deserialize, Serialize};

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
}
