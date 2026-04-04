//! Policy types — named lists and sandbox boundaries.

use serde::{Deserialize, Serialize};

/// Type of named list.
/// Maps from TS: NamedListSchema.metadata.type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NamedListType {
    NetworkAllowlist,
    NetworkDenylist,
    CommandAllowlist,
    CommandDenylist,
    SecretList,
}

/// A named list document (YAML).
/// Maps from TS: NamedListSchema in agents/schemas.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedList {
    pub api_version: String,
    pub kind: String,
    pub metadata: NamedListMetadata,
    pub entries: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedListMetadata {
    pub name: String,
    #[serde(rename = "type")]
    pub list_type: NamedListType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub always_enforced: bool,
}

/// A sandbox boundary policy document (YAML).
/// Maps from TS: SandboxBoundarySchema in agents/schemas.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxBoundary {
    pub api_version: String,
    pub kind: String,
    pub metadata: SandboxBoundaryMetadata,
    pub spec: SandboxBoundarySpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxBoundaryMetadata {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxBoundarySpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linux: Option<LinuxSecuritySpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_images: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxSecuritySpec {
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seccomp: Option<String>,
    #[serde(default)]
    pub read_only_rootfs: bool,
    #[serde(default)]
    pub run_as_non_root: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_list_type_roundtrip() {
        let json = serde_json::to_string(&NamedListType::NetworkAllowlist).unwrap();
        assert_eq!(json, "\"network-allowlist\"");
        let parsed: NamedListType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, NamedListType::NetworkAllowlist);
    }
}
