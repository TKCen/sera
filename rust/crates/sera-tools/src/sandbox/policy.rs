//! Three-layer sandbox policy types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Top-level sandbox policy — selects which sandbox backend to use.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SandboxPolicy {
    Docker(DockerSandboxPolicy),
    Wasm,
    MicroVm,
    External,
    OpenShell,
    None,
}

/// Policy for Docker sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerSandboxPolicy {
    pub filesystem: FileSystemSandboxPolicy,
    pub network: NetworkSandboxPolicy,
}

/// Filesystem access policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSystemSandboxPolicy {
    pub read_paths: Vec<String>,
    pub write_paths: Vec<String>,
    pub include_workdir: bool,
}

/// Network access policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSandboxPolicy {
    pub rules: Vec<NetworkPolicyRule>,
    pub default_deny: bool,
}

/// A single network policy rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicyRule {
    pub endpoint: NetworkEndpoint,
    pub action: PolicyAction,
    pub l7_rules: Vec<L7Rule>,
}

/// A network endpoint that a rule applies to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkEndpoint {
    Cidr(String),
    Domain(String),
    InferenceLocal,
}

/// Policy action for a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyAction {
    Allow,
    Deny,
    Audit,
}

/// Layer-7 protocol rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L7Rule {
    pub protocol: L7Protocol,
    pub path_prefix: Option<String>,
}

/// Layer-7 protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum L7Protocol {
    Http,
    Https,
    Grpc,
}

/// Metadata about a policy version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyStatus {
    pub version: u64,
    pub content_hash: [u8; 32],
    pub loaded_at: DateTime<Utc>,
}
