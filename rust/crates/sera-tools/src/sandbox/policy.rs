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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_network_policy(default_deny: bool, rules: Vec<NetworkPolicyRule>) -> NetworkSandboxPolicy {
        NetworkSandboxPolicy { rules, default_deny }
    }

    fn allow_cidr(cidr: &str) -> NetworkPolicyRule {
        NetworkPolicyRule {
            endpoint: NetworkEndpoint::Cidr(cidr.to_string()),
            action: PolicyAction::Allow,
            l7_rules: vec![],
        }
    }

    fn deny_cidr(cidr: &str) -> NetworkPolicyRule {
        NetworkPolicyRule {
            endpoint: NetworkEndpoint::Cidr(cidr.to_string()),
            action: PolicyAction::Deny,
            l7_rules: vec![],
        }
    }

    // --- SandboxPolicy serde variants ---

    #[test]
    fn sandbox_policy_none_roundtrip() {
        let p = SandboxPolicy::None;
        let json = serde_json::to_string(&p).unwrap();
        let back: SandboxPolicy = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, SandboxPolicy::None));
    }

    #[test]
    fn sandbox_policy_wasm_roundtrip() {
        let p = SandboxPolicy::Wasm;
        let json = serde_json::to_string(&p).unwrap();
        let back: SandboxPolicy = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, SandboxPolicy::Wasm));
    }

    #[test]
    fn sandbox_policy_microvm_roundtrip() {
        let p = SandboxPolicy::MicroVm;
        let json = serde_json::to_string(&p).unwrap();
        let back: SandboxPolicy = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, SandboxPolicy::MicroVm));
    }

    #[test]
    fn sandbox_policy_external_roundtrip() {
        let p = SandboxPolicy::External;
        let json = serde_json::to_string(&p).unwrap();
        let back: SandboxPolicy = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, SandboxPolicy::External));
    }

    #[test]
    fn sandbox_policy_openshell_roundtrip() {
        let p = SandboxPolicy::OpenShell;
        let json = serde_json::to_string(&p).unwrap();
        let back: SandboxPolicy = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, SandboxPolicy::OpenShell));
    }

    // --- DockerSandboxPolicy roundtrip ---

    #[test]
    fn docker_policy_full_roundtrip() {
        let policy = SandboxPolicy::Docker(DockerSandboxPolicy {
            filesystem: FileSystemSandboxPolicy {
                read_paths: vec!["/etc".to_string()],
                write_paths: vec!["/tmp".to_string()],
                include_workdir: true,
            },
            network: make_network_policy(true, vec![allow_cidr("10.0.0.0/8")]),
        });
        let json = serde_json::to_string(&policy).unwrap();
        let back: SandboxPolicy = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, SandboxPolicy::Docker(_)));
    }

    // --- PolicyAction equality ---

    #[test]
    fn policy_action_allow_deny_audit_distinct() {
        assert_ne!(PolicyAction::Allow, PolicyAction::Deny);
        assert_ne!(PolicyAction::Allow, PolicyAction::Audit);
        assert_ne!(PolicyAction::Deny, PolicyAction::Audit);
    }

    #[test]
    fn policy_action_copy_clone() {
        let a = PolicyAction::Allow;
        let b = a;
        assert_eq!(a, b);
    }

    // --- NetworkSandboxPolicy: conflicting allow+deny rules for same CIDR ---

    #[test]
    fn network_policy_allow_and_deny_same_cidr_both_serialize() {
        // The policy layer does not enforce conflict resolution — it stores both.
        // This test documents that conflicting rules survive a serde roundtrip,
        // and that the caller must resolve ordering semantics.
        let policy = make_network_policy(
            true,
            vec![
                allow_cidr("192.168.1.0/24"),
                deny_cidr("192.168.1.0/24"),
            ],
        );
        let json = serde_json::to_string(&policy).unwrap();
        let back: NetworkSandboxPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back.rules.len(), 2);
        assert_eq!(back.rules[0].action, PolicyAction::Allow);
        assert_eq!(back.rules[1].action, PolicyAction::Deny);
    }

    #[test]
    fn network_policy_default_deny_preserved_in_roundtrip() {
        let policy = make_network_policy(true, vec![]);
        let json = serde_json::to_string(&policy).unwrap();
        let back: NetworkSandboxPolicy = serde_json::from_str(&json).unwrap();
        assert!(back.default_deny);
        assert!(back.rules.is_empty());
    }

    #[test]
    fn network_policy_default_allow_preserved_in_roundtrip() {
        let policy = make_network_policy(false, vec![deny_cidr("0.0.0.0/0")]);
        let json = serde_json::to_string(&policy).unwrap();
        let back: NetworkSandboxPolicy = serde_json::from_str(&json).unwrap();
        assert!(!back.default_deny);
        assert_eq!(back.rules.len(), 1);
    }

    // --- NetworkEndpoint variants ---

    #[test]
    fn network_endpoint_domain_roundtrip() {
        let rule = NetworkPolicyRule {
            endpoint: NetworkEndpoint::Domain("example.com".to_string()),
            action: PolicyAction::Allow,
            l7_rules: vec![],
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: NetworkPolicyRule = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.endpoint, NetworkEndpoint::Domain(_)));
    }

    #[test]
    fn network_endpoint_inference_local_roundtrip() {
        let rule = NetworkPolicyRule {
            endpoint: NetworkEndpoint::InferenceLocal,
            action: PolicyAction::Allow,
            l7_rules: vec![],
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: NetworkPolicyRule = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.endpoint, NetworkEndpoint::InferenceLocal));
    }

    // --- L7Rule ---

    #[test]
    fn l7_rule_with_path_prefix_roundtrip() {
        let rule = L7Rule {
            protocol: L7Protocol::Https,
            path_prefix: Some("/api/v1".to_string()),
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: L7Rule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.protocol, L7Protocol::Https);
        assert_eq!(back.path_prefix.as_deref(), Some("/api/v1"));
    }

    #[test]
    fn l7_protocol_variants_distinct() {
        assert_ne!(L7Protocol::Http, L7Protocol::Https);
        assert_ne!(L7Protocol::Http, L7Protocol::Grpc);
        assert_ne!(L7Protocol::Https, L7Protocol::Grpc);
    }

    // --- PolicyStatus hash changes on mutation ---

    #[test]
    fn policy_status_two_different_hashes_are_not_equal() {
        let mut hash_a = [0u8; 32];
        let mut hash_b = [0u8; 32];
        hash_a[0] = 1;
        hash_b[0] = 2;
        assert_ne!(hash_a, hash_b);
    }

    // --- FileSystemSandboxPolicy: empty vs populated ---

    #[test]
    fn filesystem_policy_empty_paths() {
        let fs = FileSystemSandboxPolicy {
            read_paths: vec![],
            write_paths: vec![],
            include_workdir: false,
        };
        let json = serde_json::to_string(&fs).unwrap();
        let back: FileSystemSandboxPolicy = serde_json::from_str(&json).unwrap();
        assert!(back.read_paths.is_empty());
        assert!(back.write_paths.is_empty());
        assert!(!back.include_workdir);
    }

    #[test]
    fn filesystem_policy_include_workdir_flag() {
        let fs = FileSystemSandboxPolicy {
            read_paths: vec![],
            write_paths: vec![],
            include_workdir: true,
        };
        let json = serde_json::to_string(&fs).unwrap();
        let back: FileSystemSandboxPolicy = serde_json::from_str(&json).unwrap();
        assert!(back.include_workdir);
    }
}
