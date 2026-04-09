//! K8s-style configuration manifest types for SERA.
//!
//! Every config object follows a uniform envelope: apiVersion, kind, metadata, spec.
//! This matches SPEC-config §2.1 and the MVS single-file format (§2.4).
//! The spec field is kind-specific and validated after deserialization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// API version identifier (e.g., "sera.dev/v1").
/// Parsed from the `apiVersion` field in YAML manifests.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApiVersion {
    pub group: String,
    pub version: String,
}

impl ApiVersion {
    /// Parse "sera.dev/v1" into group="sera.dev", version="v1".
    pub fn parse(s: &str) -> Option<Self> {
        let (group, version) = s.rsplit_once('/')?;
        Some(Self {
            group: group.to_string(),
            version: version.to_string(),
        })
    }

    /// Canonical string form: "group/version".
    pub fn as_str(&self) -> String {
        format!("{}/{}", self.group, self.version)
    }
}

/// Resource kinds supported by the SERA config system.
/// MVS supports: Instance, Provider, Agent, Connector.
/// Post-MVS adds: HookChain, ToolProfile, WorkflowDef, ApprovalPolicy, SecretProvider, InteropConfig.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceKind {
    Instance,
    Provider,
    Agent,
    Connector,
    // POST-MVS kinds (defined for forward compatibility in parsing):
    HookChain,
    ToolProfile,
    WorkflowDef,
    ApprovalPolicy,
    SecretProvider,
    InteropConfig,
}

impl std::str::FromStr for ResourceKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Instance" => Ok(Self::Instance),
            "Provider" => Ok(Self::Provider),
            "Agent" => Ok(Self::Agent),
            "Connector" => Ok(Self::Connector),
            "HookChain" => Ok(Self::HookChain),
            "ToolProfile" => Ok(Self::ToolProfile),
            "WorkflowDef" => Ok(Self::WorkflowDef),
            "ApprovalPolicy" => Ok(Self::ApprovalPolicy),
            "SecretProvider" => Ok(Self::SecretProvider),
            "InteropConfig" => Ok(Self::InteropConfig),
            _ => Err(format!("unknown resource kind: {}", s)),
        }
    }
}

impl std::fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Instance => write!(f, "Instance"),
            Self::Provider => write!(f, "Provider"),
            Self::Agent => write!(f, "Agent"),
            Self::Connector => write!(f, "Connector"),
            Self::HookChain => write!(f, "HookChain"),
            Self::ToolProfile => write!(f, "ToolProfile"),
            Self::WorkflowDef => write!(f, "WorkflowDef"),
            Self::ApprovalPolicy => write!(f, "ApprovalPolicy"),
            Self::SecretProvider => write!(f, "SecretProvider"),
            Self::InteropConfig => write!(f, "InteropConfig"),
        }
    }
}

/// Resource metadata — name, labels, annotations.
/// Every manifest carries this envelope regardless of kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMetadata {
    pub name: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub annotations: HashMap<String, String>,
}

/// A raw config manifest as parsed from YAML before kind-specific validation.
/// The `spec` field is a generic JSON value that gets validated against
/// the schema for the specific `kind`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawManifest {
    pub api_version: String,
    pub kind: String,
    pub metadata: ResourceMetadata,
    #[serde(default)]
    pub spec: serde_json::Value,
}

/// A validated config manifest with parsed apiVersion and kind.
#[derive(Debug, Clone)]
pub struct ConfigManifest {
    pub api_version: ApiVersion,
    pub kind: ResourceKind,
    pub metadata: ResourceMetadata,
    pub spec: serde_json::Value,
}

impl ConfigManifest {
    /// Parse and validate a RawManifest into a ConfigManifest.
    pub fn from_raw(raw: RawManifest) -> Result<Self, ConfigManifestError> {
        let api_version = ApiVersion::parse(&raw.api_version).ok_or_else(|| {
            ConfigManifestError::InvalidApiVersion(raw.api_version.clone())
        })?;

        let kind = raw.kind.parse::<ResourceKind>()
            .map_err(|_| ConfigManifestError::UnknownKind(raw.kind.clone()))?;

        Ok(Self {
            api_version,
            kind,
            metadata: raw.metadata,
            spec: raw.spec,
        })
    }
}

/// Errors from manifest parsing/validation.
#[derive(Debug, thiserror::Error)]
pub enum ConfigManifestError {
    #[error("invalid apiVersion format: {0} (expected 'group/version')")]
    InvalidApiVersion(String),
    #[error("unknown resource kind: {0}")]
    UnknownKind(String),
    #[error("YAML parse error: {0}")]
    YamlError(#[from] serde_yaml::Error),
    #[error("validation error for {kind} '{name}': {message}")]
    ValidationError {
        kind: String,
        name: String,
        message: String,
    },
}

// ── MVS Kind-Specific Spec Types ────────────────────────────────────────────

/// Instance spec — top-level SERA instance configuration.
/// MVS scope: tier and basic settings only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceSpec {
    pub tier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs_dir: Option<String>,
}

/// Provider spec — model provider configuration.
/// MVS scope: OpenAI-compatible providers only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSpec {
    pub kind: String,
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<SecretRef>,
}

/// Agent spec — agent configuration within a manifest.
/// MVS scope: provider, model, persona, tools allow list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona: Option<PersonaSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<AgentToolsSpec>,
    /// Agent workspace directory for file/memory operations.
    /// Defaults to `./data/agents/{agent_name}` if not set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
}

/// Persona configuration within an agent spec.
/// MVS scope: immutable_anchor only (no mutable persona, no introspection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub immutable_anchor: Option<String>,
}

/// Tool configuration within an agent spec.
/// MVS scope: simple allow list with glob patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolsSpec {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
}

/// Connector spec — channel connector configuration.
/// MVS scope: Discord connector only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorSpec {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<SecretRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

/// A reference to a secret value, resolved at runtime.
/// Format in YAML: `{ secret: "path/to/secret" }`
/// Resolved from SERA_SECRET_<PATH> environment variables (MVS).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretRef {
    pub secret: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_version_parse() {
        let v = ApiVersion::parse("sera.dev/v1").unwrap();
        assert_eq!(v.group, "sera.dev");
        assert_eq!(v.version, "v1");
        assert_eq!(v.as_str(), "sera.dev/v1");
    }

    #[test]
    fn api_version_parse_invalid() {
        assert!(ApiVersion::parse("no-slash").is_none());
    }

    #[test]
    fn resource_kind_roundtrip() {
        for kind in [
            ResourceKind::Instance,
            ResourceKind::Provider,
            ResourceKind::Agent,
            ResourceKind::Connector,
        ] {
            let s = kind.to_string();
            let parsed = s.parse::<ResourceKind>().unwrap();
            assert_eq!(kind, parsed);
        }
    }

    #[test]
    fn raw_manifest_parse_instance() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: my-sera
spec:
  tier: local
"#;
        let raw: RawManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(raw.api_version, "sera.dev/v1");
        assert_eq!(raw.kind, "Instance");
        assert_eq!(raw.metadata.name, "my-sera");

        let manifest = ConfigManifest::from_raw(raw).unwrap();
        assert_eq!(manifest.kind, ResourceKind::Instance);

        let spec: InstanceSpec = serde_json::from_value(manifest.spec).unwrap();
        assert_eq!(spec.tier, "local");
    }

    #[test]
    fn raw_manifest_parse_provider() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: lm-studio
spec:
  kind: openai-compatible
  base_url: "http://localhost:1234/v1"
  default_model: gemma-4-12b
"#;
        let raw: RawManifest = serde_yaml::from_str(yaml).unwrap();
        let manifest = ConfigManifest::from_raw(raw).unwrap();
        assert_eq!(manifest.kind, ResourceKind::Provider);

        let spec: ProviderSpec = serde_json::from_value(manifest.spec).unwrap();
        assert_eq!(spec.kind, "openai-compatible");
        assert_eq!(spec.base_url, "http://localhost:1234/v1");
        assert_eq!(spec.default_model.as_deref(), Some("gemma-4-12b"));
    }

    #[test]
    fn raw_manifest_parse_agent() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: sera
spec:
  provider: lm-studio
  model: gemma-4-12b
  persona:
    immutable_anchor: |
      You are Sera, an autonomous assistant.
  tools:
    allow: ["memory_*", "file_*", "shell", "session_*"]
"#;
        let raw: RawManifest = serde_yaml::from_str(yaml).unwrap();
        let manifest = ConfigManifest::from_raw(raw).unwrap();
        assert_eq!(manifest.kind, ResourceKind::Agent);

        let spec: AgentSpec = serde_json::from_value(manifest.spec).unwrap();
        assert_eq!(spec.provider, "lm-studio");
        assert_eq!(spec.model.as_deref(), Some("gemma-4-12b"));
        assert!(spec.persona.unwrap().immutable_anchor.unwrap().contains("Sera"));
        assert_eq!(spec.tools.unwrap().allow.len(), 4);
    }

    #[test]
    fn raw_manifest_parse_connector_with_secret_ref() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Connector
metadata:
  name: discord-main
spec:
  kind: discord
  token:
    secret: connectors/discord-main/token
  agent: sera
"#;
        let raw: RawManifest = serde_yaml::from_str(yaml).unwrap();
        let manifest = ConfigManifest::from_raw(raw).unwrap();
        assert_eq!(manifest.kind, ResourceKind::Connector);

        let spec: ConnectorSpec = serde_json::from_value(manifest.spec).unwrap();
        assert_eq!(spec.kind, "discord");
        assert_eq!(spec.token.unwrap().secret, "connectors/discord-main/token");
        assert_eq!(spec.agent.as_deref(), Some("sera"));
    }

    #[test]
    fn config_manifest_rejects_unknown_kind() {
        let raw = RawManifest {
            api_version: "sera.dev/v1".to_string(),
            kind: "Bogus".to_string(),
            metadata: ResourceMetadata {
                name: "test".to_string(),
                labels: HashMap::new(),
                annotations: HashMap::new(),
            },
            spec: serde_json::Value::Null,
        };
        let err = ConfigManifest::from_raw(raw).unwrap_err();
        assert!(err.to_string().contains("unknown resource kind: Bogus"));
    }

    #[test]
    fn config_manifest_rejects_bad_api_version() {
        let raw = RawManifest {
            api_version: "noslash".to_string(),
            kind: "Instance".to_string(),
            metadata: ResourceMetadata {
                name: "test".to_string(),
                labels: HashMap::new(),
                annotations: HashMap::new(),
            },
            spec: serde_json::Value::Null,
        };
        let err = ConfigManifest::from_raw(raw).unwrap_err();
        assert!(err.to_string().contains("invalid apiVersion"));
    }

    #[test]
    fn metadata_with_labels_and_annotations() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: test
  labels:
    tier: local
    team: platform
  annotations:
    description: Test instance
spec:
  tier: local
"#;
        let raw: RawManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(raw.metadata.labels.get("tier").unwrap(), "local");
        assert_eq!(raw.metadata.annotations.get("description").unwrap(), "Test instance");
    }
}
