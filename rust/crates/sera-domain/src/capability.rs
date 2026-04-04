//! Capability resolution types — runtime permission grants.

use serde::{Deserialize, Serialize};

/// Resolved capabilities for an agent instance.
/// Maps from TS: ResolvedCapabilities in agents/manifest/types.ts
///
/// Unlike the TS version which uses Record<string, any>, this uses
/// typed fields to prevent invalid state at compile time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResolvedCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<FilesystemCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec: Option<ExecCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<SecurityCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secrets: Option<SecretsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_packages: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemCapability {
    #[serde(default)]
    pub write: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_workspace_size_gb: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outbound: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_shares: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityCapability {
    #[serde(default)]
    pub readonly_rootfs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<Vec<String>>,
}
