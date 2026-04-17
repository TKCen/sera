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

    #[test]
    fn agent_status_serde_key_names() {
        // Verify the exact lowercase snake_case wire names.
        let cases = [
            (AgentStatus::Created, "created"),
            (AgentStatus::Running, "running"),
            (AgentStatus::Stopped, "stopped"),
            (AgentStatus::Error, "error"),
            (AgentStatus::Unresponsive, "unresponsive"),
            (AgentStatus::Throttled, "throttled"),
            (AgentStatus::Active, "active"),
            (AgentStatus::Inactive, "inactive"),
        ];
        for (status, expected) in cases {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, format!("\"{}\"", expected));
        }
    }

    #[test]
    fn agent_instance_full_construction_roundtrip() {
        let inst = AgentInstance {
            id: "inst-99".to_string(),
            name: "full-agent".to_string(),
            display_name: Some("Full Agent".to_string()),
            template_ref: "example-full".to_string(),
            circle: Some("circle-alpha".to_string()),
            status: AgentStatus::Active,
            overrides: Some(serde_json::json!({"model": "gpt-4o"})),
            lifecycle_mode: Some(LifecycleMode::Persistent),
            parent_instance_id: Some("inst-parent".to_string()),
            resolved_config: Some(serde_json::json!({"key": "val"})),
            resolved_capabilities: Some(serde_json::json!({"network": {}})),
            workspace_path: Some("/workspace/agent-99".to_string()),
            workspace_used_gb: Some(1.5),
            container_id: Some("container-abc".to_string()),
            circle_id: Some("circle-id-1".to_string()),
            last_heartbeat_at: Some("2026-04-17T10:00:00Z".to_string()),
            updated_at: "2026-04-17T10:00:00Z".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&inst).unwrap();
        let parsed: AgentInstance = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "inst-99");
        assert_eq!(parsed.display_name.as_deref(), Some("Full Agent"));
        assert_eq!(parsed.circle.as_deref(), Some("circle-alpha"));
        assert_eq!(parsed.status, AgentStatus::Active);
        assert_eq!(parsed.workspace_used_gb, Some(1.5));
        assert_eq!(parsed.parent_instance_id.as_deref(), Some("inst-parent"));
    }

    #[test]
    fn agent_instance_optional_fields_omitted_in_json() {
        let inst = AgentInstance {
            id: "inst-min".to_string(),
            name: "min-agent".to_string(),
            display_name: None,
            template_ref: "tmpl".to_string(),
            circle: None,
            status: AgentStatus::Stopped,
            overrides: None,
            lifecycle_mode: None,
            parent_instance_id: None,
            resolved_config: None,
            resolved_capabilities: None,
            workspace_path: None,
            workspace_used_gb: None,
            container_id: None,
            circle_id: None,
            last_heartbeat_at: None,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&inst).unwrap();
        // All optional fields must be absent from the serialized JSON.
        assert!(!json.contains("display_name"));
        assert!(!json.contains("circle"));
        assert!(!json.contains("overrides"));
        assert!(!json.contains("lifecycle_mode"));
        assert!(!json.contains("parent_instance_id"));
        assert!(!json.contains("workspace_path"));
        assert!(!json.contains("container_id"));
        assert!(!json.contains("last_heartbeat_at"));
    }

    #[test]
    fn agent_instance_lifecycle_modes_roundtrip() {
        for mode in [LifecycleMode::Ephemeral, LifecycleMode::Persistent] {
            let inst = AgentInstance {
                id: "inst-lc".to_string(),
                name: "lc-agent".to_string(),
                display_name: None,
                template_ref: "tmpl".to_string(),
                circle: None,
                status: AgentStatus::Created,
                overrides: None,
                lifecycle_mode: Some(mode),
                parent_instance_id: None,
                resolved_config: None,
                resolved_capabilities: None,
                workspace_path: None,
                workspace_used_gb: None,
                container_id: None,
                circle_id: None,
                last_heartbeat_at: None,
                updated_at: "2026-01-01T00:00:00Z".to_string(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
            };
            let json = serde_json::to_string(&inst).unwrap();
            let parsed: AgentInstance = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.lifecycle_mode, Some(mode));
        }
    }
}
