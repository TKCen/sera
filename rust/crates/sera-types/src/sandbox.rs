//! Sandbox types — container lifecycle and security.

use serde::{Deserialize, Serialize};

use crate::LifecycleMode;

/// Declared egress endpoint for a sandboxed agent.
///
/// Lives in `sera-types` (rather than `sera-tools::sandbox::policy`) so the
/// gateway, manifest layer, and runtime can all consume it without depending
/// on `sera-tools`. Mirrors `NetworkEndpoint` from
/// `sera-tools/src/sandbox/policy.rs`; the two enums must stay aligned.
///
/// `InferenceLocal` is the canonical default — it names the gateway-side
/// `inference.local:443` proxy that sandboxed agents must use to reach any
/// upstream LLM provider. See `sera-eq0m` and the egress scout report at
/// `artifacts/reports/research/eq0m-egress-restriction-scout-2026-04-30.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EgressEndpoint {
    /// The gateway-managed `inference.local` LLM proxy. Always implicitly
    /// allowed for sandboxed agents unless egress is explicitly disabled.
    InferenceLocal,
    /// A named DNS domain, e.g. `api.github.com`.
    Domain { name: String },
    /// A CIDR range, e.g. `10.0.0.0/8`.
    Cidr { range: String },
}

/// Egress enforcement mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressMode {
    /// Implicit-deny. Anything not in `allowed` is blocked at enforcement
    /// time. The compliance posture.
    #[default]
    Strict,
    /// Log violations but do not block. Used as a migration ramp before
    /// flipping to `Strict`.
    AuditOnly,
    /// Egress restriction is opted out entirely. The agent runs without an
    /// `InferenceLocal` baseline and may dial anywhere. Reserved for
    /// non-sandboxed and dev-mode flows.
    Disabled,
}

/// The egress manifest block declared on an `AgentTemplate.spec.egress`.
///
/// Implicit-deny: only endpoints in `allowed` are permitted (subject to
/// `mode`). Sandboxed manifests must include [`EgressEndpoint::InferenceLocal`]
/// in `allowed` unless `mode = Disabled` — this invariant is enforced by
/// `AgentTemplate::validate_and_normalize_egress`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressManifest {
    #[serde(default)]
    pub allowed: Vec<EgressEndpoint>,
    #[serde(default)]
    pub mode: EgressMode,
}

impl EgressManifest {
    /// True iff `allowed` already contains `InferenceLocal`.
    pub fn allows_inference_local(&self) -> bool {
        self.allowed
            .iter()
            .any(|e| matches!(e, EgressEndpoint::InferenceLocal))
    }
}

/// A read-only source bind-mount for agent containers.
/// Separates operator-provided reference material from agent-generated knowledge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMount {
    /// Host path to the source material.
    pub host_path: String,
    /// Container path (default: /sources/<name>).
    pub container_path: String,
    /// Human-readable label for this source.
    pub label: Option<String>,
}

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceMount>,
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
            sources: vec![],
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
            sources: vec![],
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
            sources: vec![],
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("lifecycle_mode"));
        assert!(!json.contains("proxy_enabled"));
        assert!(!json.contains("parent_agent"));
    }

    #[test]
    fn source_mount_roundtrip() {
        let mount = SourceMount {
            host_path: "/host/docs".to_string(),
            container_path: "/sources/docs".to_string(),
            label: Some("Reference docs".to_string()),
        };
        let json = serde_json::to_string(&mount).unwrap();
        let parsed: SourceMount = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.host_path, mount.host_path);
        assert_eq!(parsed.container_path, mount.container_path);
        assert_eq!(parsed.label, mount.label);
    }

    #[test]
    fn source_mount_no_label_roundtrip() {
        let mount = SourceMount {
            host_path: "/host/data".to_string(),
            container_path: "/sources/data".to_string(),
            label: None,
        };
        let json = serde_json::to_string(&mount).unwrap();
        let parsed: SourceMount = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.host_path, mount.host_path);
        assert!(parsed.label.is_none());
    }

    #[test]
    fn sandbox_info_with_sources_serializes_correctly() {
        let info = SandboxInfo {
            container_id: "c2".to_string(),
            agent_name: "a2".to_string(),
            sandbox_type: "docker".to_string(),
            image: "img".to_string(),
            status: SandboxStatus::Running,
            created_at: "2026-04-05T00:00:00Z".to_string(),
            tier: SandboxTier::Tier1,
            instance_id: "inst2".to_string(),
            lifecycle_mode: None,
            proxy_enabled: None,
            container_ip: None,
            chat_url: None,
            parent_agent: None,
            subagent_role: None,
            sources: vec![SourceMount {
                host_path: "/host/ref".to_string(),
                container_path: "/sources/ref".to_string(),
                label: Some("Ref".to_string()),
            }],
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("sources"));
        assert!(json.contains("/sources/ref"));
        let parsed: SandboxInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.sources.len(), 1);
        assert_eq!(parsed.sources[0].host_path, "/host/ref");
    }

    #[test]
    fn egress_endpoint_inference_local_roundtrip() {
        let endpoint = EgressEndpoint::InferenceLocal;
        let json = serde_json::to_string(&endpoint).unwrap();
        assert_eq!(json, r#"{"type":"inference_local"}"#);
        let parsed: EgressEndpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, EgressEndpoint::InferenceLocal);
    }

    #[test]
    fn egress_endpoint_domain_roundtrip() {
        let endpoint = EgressEndpoint::Domain {
            name: "api.github.com".to_string(),
        };
        let json = serde_json::to_string(&endpoint).unwrap();
        let parsed: EgressEndpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, endpoint);
        assert!(json.contains(r#""type":"domain""#));
        assert!(json.contains(r#""name":"api.github.com""#));
    }

    #[test]
    fn egress_endpoint_cidr_roundtrip() {
        let endpoint = EgressEndpoint::Cidr {
            range: "10.0.0.0/8".to_string(),
        };
        let json = serde_json::to_string(&endpoint).unwrap();
        let parsed: EgressEndpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, endpoint);
        assert!(json.contains(r#""type":"cidr""#));
        assert!(json.contains(r#""range":"10.0.0.0/8""#));
    }

    #[test]
    fn egress_mode_default_is_strict() {
        assert_eq!(EgressMode::default(), EgressMode::Strict);
    }

    #[test]
    fn egress_mode_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&EgressMode::Strict).unwrap(),
            "\"strict\"",
        );
        assert_eq!(
            serde_json::to_string(&EgressMode::AuditOnly).unwrap(),
            "\"audit_only\"",
        );
        assert_eq!(
            serde_json::to_string(&EgressMode::Disabled).unwrap(),
            "\"disabled\"",
        );
    }

    #[test]
    fn egress_mode_all_roundtrip() {
        for mode in [EgressMode::Strict, EgressMode::AuditOnly, EgressMode::Disabled] {
            let json = serde_json::to_string(&mode).unwrap();
            let parsed: EgressMode = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, mode);
        }
    }

    #[test]
    fn egress_manifest_default_is_strict_with_no_allowed() {
        let m = EgressManifest::default();
        assert!(m.allowed.is_empty());
        assert_eq!(m.mode, EgressMode::Strict);
        assert!(!m.allows_inference_local());
    }

    #[test]
    fn egress_manifest_full_roundtrip() {
        let m = EgressManifest {
            allowed: vec![
                EgressEndpoint::InferenceLocal,
                EgressEndpoint::Domain {
                    name: "api.github.com".to_string(),
                },
                EgressEndpoint::Cidr {
                    range: "10.0.0.0/8".to_string(),
                },
            ],
            mode: EgressMode::AuditOnly,
        };
        let json = serde_json::to_string(&m).unwrap();
        let parsed: EgressManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, m);
        assert!(parsed.allows_inference_local());
    }

    #[test]
    fn egress_manifest_omitted_fields_default() {
        let parsed: EgressManifest = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed.mode, EgressMode::Strict);
        assert!(parsed.allowed.is_empty());
    }

    #[test]
    fn egress_manifest_allows_inference_local_detects_presence() {
        let with = EgressManifest {
            allowed: vec![EgressEndpoint::InferenceLocal],
            mode: EgressMode::Strict,
        };
        let without = EgressManifest {
            allowed: vec![EgressEndpoint::Domain {
                name: "x".to_string(),
            }],
            mode: EgressMode::Strict,
        };
        assert!(with.allows_inference_local());
        assert!(!without.allows_inference_local());
    }

    #[test]
    fn sandbox_info_empty_sources_not_serialized() {
        let info = SandboxInfo {
            container_id: "c3".to_string(),
            agent_name: "a3".to_string(),
            sandbox_type: "docker".to_string(),
            image: "img".to_string(),
            status: SandboxStatus::Running,
            created_at: "2026-04-05T00:00:00Z".to_string(),
            tier: SandboxTier::Tier1,
            instance_id: "inst3".to_string(),
            lifecycle_mode: None,
            proxy_enabled: None,
            container_ip: None,
            chat_url: None,
            parent_agent: None,
            subagent_role: None,
            sources: vec![],
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("sources"));
    }
}
