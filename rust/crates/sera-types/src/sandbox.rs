//! Sandbox types — container lifecycle and security.

use serde::{Deserialize, Serialize};

use crate::LifecycleMode;

/// Sandbox security tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SandboxTier {
    #[serde(rename = "1")]
    Tier1,
    #[serde(rename = "2")]
    Tier2,
    #[serde(rename = "3")]
    Tier3,
}

/// Information about a running sandbox container.
/// Maps from TS: SandboxInfo in sandbox/SandboxManager.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxInfo {
    pub container_id: String,
    pub agent_name: String,
    #[serde(rename = "type")]
    pub sandbox_type: String,
    pub image: String,
    pub status: SandboxStatus,
    pub created_at: String,
    pub tier: SandboxTier,
    pub instance_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle_mode: Option<LifecycleMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_role: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxStatus {
    Running,
    Removing,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_tier_roundtrip() {
        for tier in [SandboxTier::Tier1, SandboxTier::Tier2, SandboxTier::Tier3] {
            let json = serde_json::to_string(&tier).unwrap();
            let parsed: SandboxTier = serde_json::from_str(&json).unwrap();
            assert_eq!(tier, parsed);
        }
    }

    #[test]
    fn sandbox_tier_json_format() {
        assert_eq!(serde_json::to_string(&SandboxTier::Tier1).unwrap(), "\"1\"");
        assert_eq!(serde_json::to_string(&SandboxTier::Tier2).unwrap(), "\"2\"");
        assert_eq!(serde_json::to_string(&SandboxTier::Tier3).unwrap(), "\"3\"");
    }

    #[test]
    fn sandbox_status_roundtrip() {
        for status in [SandboxStatus::Running, SandboxStatus::Removing] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: SandboxStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn sandbox_info_minimal() {
        let info = SandboxInfo {
            container_id: "container-123".to_string(),
            agent_name: "test-agent".to_string(),
            sandbox_type: "docker".to_string(),
            image: "sera-agent:latest".to_string(),
            status: SandboxStatus::Running,
            created_at: "2026-04-05T00:00:00Z".to_string(),
            tier: SandboxTier::Tier2,
            instance_id: "inst-456".to_string(),
            lifecycle_mode: None,
            proxy_enabled: None,
            container_ip: None,
            chat_url: None,
            parent_agent: None,
            subagent_role: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: SandboxInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.container_id, "container-123");
        assert_eq!(parsed.tier, SandboxTier::Tier2);
        assert_eq!(parsed.status, SandboxStatus::Running);
    }

    #[test]
    fn sandbox_info_full() {
        let info = SandboxInfo {
            container_id: "container-789".to_string(),
            agent_name: "subagent".to_string(),
            sandbox_type: "docker".to_string(),
            image: "sera-subagent:v1.0".to_string(),
            status: SandboxStatus::Running,
            created_at: "2026-04-05T12:00:00Z".to_string(),
            tier: SandboxTier::Tier3,
            instance_id: "inst-999".to_string(),
            lifecycle_mode: Some(LifecycleMode::Persistent),
            proxy_enabled: Some(true),
            container_ip: Some("172.17.0.5".to_string()),
            chat_url: Some("http://sera-core:3001/chat/inst-999".to_string()),
            parent_agent: Some("parent-agent".to_string()),
            subagent_role: Some("executor".to_string()),
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: SandboxInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.lifecycle_mode, Some(LifecycleMode::Persistent));
        assert_eq!(parsed.proxy_enabled, Some(true));
        assert_eq!(parsed.parent_agent, Some("parent-agent".to_string()));
    }

    #[test]
    fn sandbox_info_optional_fields_not_serialized() {
        let info = SandboxInfo {
            container_id: "c1".to_string(),
            agent_name: "a1".to_string(),
            sandbox_type: "docker".to_string(),
            image: "img".to_string(),
            status: SandboxStatus::Running,
            created_at: "2026-04-05T00:00:00Z".to_string(),
            tier: SandboxTier::Tier1,
            instance_id: "inst1".to_string(),
            lifecycle_mode: None,
            proxy_enabled: None,
            container_ip: None,
            chat_url: None,
            parent_agent: None,
            subagent_role: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("lifecycle_mode"));
        assert!(!json.contains("proxy_enabled"));
        assert!(!json.contains("parent_agent"));
    }
}
