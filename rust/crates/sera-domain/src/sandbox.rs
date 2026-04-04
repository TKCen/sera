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
}
