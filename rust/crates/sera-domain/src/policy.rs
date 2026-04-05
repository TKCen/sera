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

    #[test]
    fn named_list_all_types() {
        let types = vec![
            NamedListType::NetworkAllowlist,
            NamedListType::NetworkDenylist,
            NamedListType::CommandAllowlist,
            NamedListType::CommandDenylist,
            NamedListType::SecretList,
        ];
        for list_type in types {
            let json = serde_json::to_string(&list_type).unwrap();
            let parsed: NamedListType = serde_json::from_str(&json).unwrap();
            assert_eq!(list_type, parsed);
        }
    }

    #[test]
    fn named_list_yaml_parse() {
        let yaml = r#"
apiVersion: sera/v1
kind: NamedList
metadata:
  name: trusted-hosts
  type: network-allowlist
  description: Trusted external hosts
  alwaysEnforced: true
entries:
  - api.github.com
  - registry.docker.com
"#;
        let list: NamedList = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(list.metadata.name, "trusted-hosts");
        assert_eq!(list.metadata.list_type, NamedListType::NetworkAllowlist);
        assert!(list.metadata.always_enforced);
        assert_eq!(list.entries.len(), 2);
    }

    #[test]
    fn sandbox_boundary_yaml_parse() {
        let yaml = r#"
apiVersion: sera/v1
kind: SandboxBoundary
metadata:
  name: tier-2
  description: Tier 2 sandbox with network access
spec:
  linux:
    capabilities:
      - NET_BIND_SERVICE
    seccomp: default
    readOnlyRootfs: false
    runAsNonRoot: true
  allowedImages:
    - sera-agent:*
"#;
        let boundary: SandboxBoundary = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(boundary.metadata.name, "tier-2");
        let linux = boundary.spec.linux.unwrap();
        assert_eq!(linux.capabilities.len(), 1);
        assert!(!linux.read_only_rootfs);
        assert!(linux.run_as_non_root);
    }

    #[test]
    fn sandbox_boundary_minimal() {
        let yaml = r#"
apiVersion: sera/v1
kind: SandboxBoundary
metadata:
  name: tier-1
spec: {}
"#;
        let boundary: SandboxBoundary = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(boundary.metadata.name, "tier-1");
        assert!(boundary.spec.linux.is_none());
        assert!(boundary.spec.allowed_images.is_none());
    }

    #[test]
    fn named_list_json_roundtrip() {
        let list = NamedList {
            api_version: "sera/v1".to_string(),
            kind: "NamedList".to_string(),
            metadata: NamedListMetadata {
                name: "test-list".to_string(),
                list_type: NamedListType::CommandAllowlist,
                description: Some("Test".to_string()),
                always_enforced: false,
            },
            entries: vec![serde_json::json!("echo"), serde_json::json!("ls")],
        };
        let json = serde_json::to_string(&list).unwrap();
        let parsed: NamedList = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.metadata.name, "test-list");
        assert_eq!(parsed.entries.len(), 2);
    }
}
