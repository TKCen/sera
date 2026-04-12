//! Agent instance types — runtime state for spawned agents.

use serde::{Deserialize, Serialize};

use crate::LifecycleMode;

/// Runtime status of an agent instance.
/// Maps from TS: 'created' | 'running' | 'stopped' | 'error' | 'unresponsive' | 'throttled' | 'active' | 'inactive'
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Created,
    Running,
    Stopped,
    Error,
    Unresponsive,
    Throttled,
    Active,
    Inactive,
}

/// Database-backed agent instance record.
/// Maps from TS: AgentInstance in agents/types.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInstance {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub template_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub circle: Option<String>,
    pub status: AgentStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overrides: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle_mode: Option<LifecycleMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_instance_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_config: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_capabilities: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_used_gb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub circle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_heartbeat_at: Option<String>,
    pub updated_at: String,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_status_roundtrip() {
        for status in [
            AgentStatus::Created,
            AgentStatus::Running,
            AgentStatus::Stopped,
            AgentStatus::Error,
            AgentStatus::Unresponsive,
            AgentStatus::Throttled,
            AgentStatus::Active,
            AgentStatus::Inactive,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: AgentStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn agent_instance_minimal_json() {
        let json = r#"{
            "id": "inst-1",
            "name": "test-agent",
            "template_ref": "example-minimal",
            "status": "running",
            "updated_at": "2026-01-01T00:00:00Z",
            "created_at": "2026-01-01T00:00:00Z"
        }"#;
        let inst: AgentInstance = serde_json::from_str(json).unwrap();
        assert_eq!(inst.name, "test-agent");
        assert_eq!(inst.status, AgentStatus::Running);
        assert!(inst.display_name.is_none());
    }
}
