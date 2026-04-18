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
    /// Allow-list of agent ids this principal may invoke as a sub-agent
    /// via the agent-as-tool registry (bead `sera-8d1.1`, GH#144).
    ///
    /// `None` means "no sub-agent dispatch allowed". An empty `Some(vec![])`
    /// also denies all targets — only ids explicitly listed are permitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagents_allowed: Option<Vec<String>>,
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

/// Agent-level capability for self-evolution operations.
/// Distinct from `ResolvedCapabilities` which are container-level grants.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCapability {
    MetaChange,
    CodeChange,
    MetaApprover,
    ConfigRead,
    ConfigPropose,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_capabilities_default() {
        let caps = ResolvedCapabilities::default();
        assert!(caps.filesystem.is_none());
        assert!(caps.network.is_none());
        assert!(caps.exec.is_none());
    }

    #[test]
    fn resolved_capabilities_full() {
        let caps = ResolvedCapabilities {
            filesystem: Some(FilesystemCapability {
                write: true,
                max_workspace_size_gb: Some(50.0),
            }),
            network: Some(NetworkCapability {
                outbound: Some(vec!["api.github.com".to_string()]),
            }),
            exec: Some(ExecCapability {
                commands: Some(vec!["bash".to_string(), "python".to_string()]),
            }),
            resources: Some(ResourceCapability {
                cpu_shares: Some(1024),
                memory_limit: Some(1_000_000_000),
            }),
            security: Some(SecurityCapability {
                readonly_rootfs: false,
            }),
            secrets: Some(SecretsCapability {
                access: Some(vec!["db-password".to_string()]),
            }),
            capabilities: Some(vec!["CAP_NET_BIND_SERVICE".to_string()]),
            skill_packages: Some(vec!["core-skills".to_string()]),
            subagents_allowed: Some(vec!["researcher".to_string()]),
        };
        let json = serde_json::to_string(&caps).unwrap();
        let parsed: ResolvedCapabilities = serde_json::from_str(&json).unwrap();
        assert!(parsed.filesystem.unwrap().write);
        assert_eq!(parsed.exec.unwrap().commands.unwrap().len(), 2);
    }

    #[test]
    fn filesystem_capability_write_flag() {
        let cap = FilesystemCapability {
            write: true,
            max_workspace_size_gb: Some(100.0),
        };
        let json = serde_json::to_string(&cap).unwrap();
        let parsed: FilesystemCapability = serde_json::from_str(&json).unwrap();
        assert!(parsed.write);
        assert_eq!(parsed.max_workspace_size_gb, Some(100.0));
    }

    #[test]
    fn network_capability_outbound_list() {
        let cap = NetworkCapability {
            outbound: Some(vec![
                "api.github.com".to_string(),
                "registry.docker.com".to_string(),
            ]),
        };
        let json = serde_json::to_string(&cap).unwrap();
        let parsed: NetworkCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.outbound.unwrap().len(), 2);
    }

    #[test]
    fn exec_capability_commands() {
        let cap = ExecCapability {
            commands: Some(vec!["ls".to_string(), "cat".to_string(), "echo".to_string()]),
        };
        let json = serde_json::to_string(&cap).unwrap();
        let parsed: ExecCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.commands.unwrap().len(), 3);
    }

    #[test]
    fn resource_capability_limits() {
        let cap = ResourceCapability {
            cpu_shares: Some(2048),
            memory_limit: Some(4_000_000_000),
        };
        let json = serde_json::to_string(&cap).unwrap();
        let parsed: ResourceCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.cpu_shares, Some(2048));
        assert_eq!(parsed.memory_limit, Some(4_000_000_000));
    }
}
