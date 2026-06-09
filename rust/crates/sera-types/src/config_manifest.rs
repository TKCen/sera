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
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceKind {
    Instance,
    Provider,
    Agent,
    Connector,
    HookChain,
    ToolProfile,
    WorkflowDef,
    ApprovalPolicy,
    SecretProvider,
    InteropConfig,
    SandboxPolicy,
    Circle,
    ChangeArtifact,
    CapabilityPolicy,
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
            "SandboxPolicy" => Ok(Self::SandboxPolicy),
            "Circle" => Ok(Self::Circle),
            "ChangeArtifact" => Ok(Self::ChangeArtifact),
            "CapabilityPolicy" => Ok(Self::CapabilityPolicy),
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
            Self::SandboxPolicy => write!(f, "SandboxPolicy"),
            Self::Circle => write!(f, "Circle"),
            Self::ChangeArtifact => write!(f, "ChangeArtifact"),
            Self::CapabilityPolicy => write!(f, "CapabilityPolicy"),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_artifact: Option<crate::evolution::ChangeArtifactId>,
    #[serde(default)]
    pub shadow: bool,
}

/// Current schema version for new manifests. Persisted in the `_configVersion`
/// (camelCase) field of every manifest so future schema migrations can detect
/// older shapes. Older files without the field are interpreted as v1.
pub const CONFIG_VERSION: u32 = 1;

fn default_config_version() -> u32 {
    CONFIG_VERSION
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
    /// Schema version for this manifest. Defaults to [`CONFIG_VERSION`] when
    /// absent; future migrations bump this and gate parsing on the value.
    #[serde(default = "default_config_version", rename = "_configVersion")]
    pub config_version: u32,
    #[serde(default)]
    pub spec: serde_json::Value,
}

/// A validated config manifest with parsed apiVersion and kind.
#[derive(Debug, Clone)]
pub struct ConfigManifest {
    pub api_version: ApiVersion,
    pub kind: ResourceKind,
    pub metadata: ResourceMetadata,
    pub config_version: u32,
    pub spec: serde_json::Value,
}

impl ConfigManifest {
    /// Parse and validate a RawManifest into a ConfigManifest.
    pub fn from_raw(raw: RawManifest) -> Result<Self, ConfigManifestError> {
        let api_version = ApiVersion::parse(&raw.api_version)
            .ok_or_else(|| ConfigManifestError::InvalidApiVersion(raw.api_version.clone()))?;

        let kind = raw
            .kind
            .parse::<ResourceKind>()
            .map_err(|_| ConfigManifestError::UnknownKind(raw.kind.clone()))?;

        Ok(Self {
            api_version,
            kind,
            metadata: raw.metadata,
            config_version: raw.config_version,
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
/// MVS scope: basic settings only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceSpec {
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
    /// Name of a CapabilityPolicy (by `metadata.name`) loaded from the
    /// configured `SERA_CAPABILITY_POLICIES_DIR`. When set, every tool call
    /// made by this agent is checked against the policy's allowed-tool list
    /// by the gateway's `CapabilityRegistry` (sera-ifjl). When absent, no
    /// policy is enforced — the agent is permissive by default.
    #[serde(default, alias = "policyRef", skip_serializing_if = "Option::is_none")]
    pub policy_ref: Option<String>,
    /// HITL enforcement mode — `autonomous` (no approvals), `standard`
    /// (policy-driven), or `strict` (every tool call needs approval).
    /// Stored as an opaque lowercase string so this crate stays free of a
    /// cyclic dep on `sera-hitl`. The gateway parses it into
    /// `sera_hitl::HitlMode` via serde when consulted. Defaults to
    /// `autonomous` when absent. Wave D (sera-z6ql).
    #[serde(
        default,
        alias = "enforcementMode",
        skip_serializing_if = "Option::is_none"
    )]
    pub enforcement_mode: Option<String>,
    /// HITL approval routing — serialised `sera_hitl::ApprovalRouting`. Kept
    /// as a generic JSON value for the same crate-layering reason as
    /// `enforcement_mode`. The gateway deserialises it into the concrete
    /// type before calling `ApprovalRouter::needs_approval`. Absent =
    /// `{ "mode": "autonomous" }`. Wave D (sera-z6ql).
    #[serde(
        default,
        alias = "approvalPolicy",
        skip_serializing_if = "Option::is_none"
    )]
    pub approval_policy: Option<serde_json::Value>,
    /// Agent IDs this agent may hand off control to (sera-q9i5).
    ///
    /// When non-empty, the gateway forwards this list as
    /// `SERA_AGENT_SUBAGENTS_ALLOWED` to the runtime; the runtime injects
    /// `metadata["subagents_allowed"]` into every `TurnContext` so
    /// `default_runtime` builds `handoff_to_<id>` tool definitions for the
    /// LLM. Empty/absent (the default) means no handoff tools are exposed.
    #[serde(
        default,
        alias = "subagentsAllowed",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub subagents_allowed: Vec<String>,
    /// Hermes-parity feature surface toggles (sera-ibkr.7).
    ///
    /// This is intentionally broader than the legacy `tools.allow` list:
    /// Hermes separates base memory injection, the built-in memory write tool,
    /// external memory-provider tools, context-engine tools, skills, and
    /// session-search into separately gated surfaces. SERA mirrors that shape
    /// here so gateway/runtime assemblers can decide which schemas and context
    /// blocks are exposed per agent without collapsing everything into one
    /// generic `memory` boolean.
    #[serde(default, skip_serializing_if = "AgentFeatureSetSpec::is_default")]
    pub features: AgentFeatureSetSpec,
}

/// Persona configuration within an agent spec.
/// MVS scope: immutable_anchor only (no mutable persona, no introspection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub immutable_anchor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mutable_persona: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mutable_token_budget: Option<u32>,
}

/// Tool configuration within an agent spec.
/// MVS scope: simple allow list with glob patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolsSpec {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
}

/// Per-agent Hermes-parity feature toggles (sera-ibkr.7).
///
/// These switches model **surfaces**, not backends. They let the gateway and
/// runtime independently decide whether to inject base identity memory, expose
/// built-in memory write tools, expose external provider tools, expose
/// context-engine tools, and load skills/session-search. This mirrors Hermes's
/// separation between the `memory` toolset, external `MemoryProvider` schemas,
/// and `context_engine` schemas.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AgentFeatureSetSpec {
    #[serde(
        default,
        alias = "identityMemory",
        skip_serializing_if = "IdentityMemoryFeatureSpec::is_default"
    )]
    pub identity_memory: IdentityMemoryFeatureSpec,
    #[serde(
        default,
        alias = "semanticMemory",
        skip_serializing_if = "SemanticMemoryFeatureSpec::is_default"
    )]
    pub semantic_memory: SemanticMemoryFeatureSpec,
    #[serde(
        default,
        alias = "memoryProviders",
        skip_serializing_if = "MemoryProvidersFeatureSpec::is_default"
    )]
    pub memory_providers: MemoryProvidersFeatureSpec,
    #[serde(
        default,
        alias = "contextEngine",
        skip_serializing_if = "ContextEngineFeatureSpec::is_default"
    )]
    pub context_engine: ContextEngineFeatureSpec,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub toolsets: HashMap<String, ToolsetGateSpec>,
}

impl AgentFeatureSetSpec {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct IdentityMemoryFeatureSpec {
    /// Enables base memory assembly for this agent.
    #[serde(default)]
    pub enabled: bool,
    /// Inject immutable identity/persona anchor such as `soul.md`.
    #[serde(default)]
    pub soul: bool,
    /// Inject compact durable notes analogous to Hermes `MEMORY.md`.
    #[serde(default, alias = "durableMemory")]
    pub durable_memory: bool,
    /// Inject compact user profile analogous to Hermes `USER.md`.
    #[serde(default, alias = "userProfile")]
    pub user_profile: bool,
    /// Inject environment/setup facts.
    #[serde(default)]
    pub environment: bool,
    /// Expose the built-in memory mutation tool surface when the memory
    /// toolset also allows it.
    #[serde(default, alias = "memoryTool")]
    pub memory_tool: bool,
    /// Write policy for built-in memory mutations: `disabled`, `gated`, or
    /// `allowed`. Kept as string pending policy enum unification.
    #[serde(
        default,
        alias = "writePolicy",
        skip_serializing_if = "Option::is_none"
    )]
    pub write_policy: Option<String>,
    #[serde(
        default,
        alias = "memoryCharLimit",
        skip_serializing_if = "Option::is_none"
    )]
    pub memory_char_limit: Option<u32>,
    #[serde(
        default,
        alias = "userCharLimit",
        skip_serializing_if = "Option::is_none"
    )]
    pub user_char_limit: Option<u32>,
}

impl IdentityMemoryFeatureSpec {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SemanticMemoryFeatureSpec {
    #[serde(default)]
    pub enabled: bool,
    /// Backend selector, e.g. `sqlite`, `pgvector`, or `plugin:<name>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default, alias = "topK", skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(
        default,
        alias = "maxInjectedChars",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_injected_chars: Option<u32>,
    #[serde(
        default,
        alias = "writePolicy",
        skip_serializing_if = "Option::is_none"
    )]
    pub write_policy: Option<String>,
}

impl SemanticMemoryFeatureSpec {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemoryProvidersFeatureSpec {
    #[serde(default)]
    pub enabled: bool,
    /// Active provider name. Hermes allows one external provider at a time;
    /// SERA starts with the same default constraint to avoid schema bloat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(
        default,
        alias = "allowMultiple",
        skip_serializing_if = "Option::is_none"
    )]
    pub allow_multiple: Option<bool>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub providers: HashMap<String, MemoryProviderFeatureSpec>,
}

impl MemoryProvidersFeatureSpec {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemoryProviderFeatureSpec {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub recall: bool,
    #[serde(default)]
    pub reflect: bool,
    #[serde(
        default,
        alias = "retainPolicy",
        skip_serializing_if = "Option::is_none"
    )]
    pub retain_policy: Option<String>,
    #[serde(
        default,
        alias = "exposeTools",
        skip_serializing_if = "Option::is_none"
    )]
    pub expose_tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContextEngineFeatureSpec {
    #[serde(default)]
    pub enabled: bool,
    /// Engine selector, e.g. `compressor`, `lcm`, or `plugin:<name>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    #[serde(default, alias = "exposeTools")]
    pub expose_tools: bool,
    #[serde(default, alias = "toolAllow", skip_serializing_if = "Vec::is_empty")]
    pub tool_allow: Vec<String>,
}

impl ContextEngineFeatureSpec {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolsetGateSpec {
    #[serde(default)]
    pub enabled: bool,
    /// Memory toolset-specific: expose SERA's built-in memory action tool.
    #[serde(default, alias = "builtinMemoryTool")]
    pub builtin_memory_tool: bool,
    /// Memory toolset-specific: expose external MemoryProvider schemas.
    #[serde(default, alias = "providerTools")]
    pub provider_tools: bool,
    /// Context-engine/toolset-specific explicit allow list.
    #[serde(default, alias = "toolAllow", skip_serializing_if = "Vec::is_empty")]
    pub tool_allow: Vec<String>,
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
    /// Peer bots that are allowed to hand off to SERA on this connector
    /// (sera-yeg.1). Matched against the message author's Discord user id
    /// OR username. Empty/absent = strict default (no peer bots accepted).
    ///
    /// Acceptance still requires the peer bot's message to be an explicit
    /// handoff — either a DM to SERA or an @-mention of SERA in a guild
    /// channel — so a chatty configured peer cannot create a feedback loop
    /// by talking past SERA.
    #[serde(default, alias = "peerBots", skip_serializing_if = "Vec::is_empty")]
    pub peer_bots: Vec<String>,
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
spec: {}
"#;
        let raw: RawManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(raw.api_version, "sera.dev/v1");
        assert_eq!(raw.kind, "Instance");
        assert_eq!(raw.metadata.name, "my-sera");

        let manifest = ConfigManifest::from_raw(raw).unwrap();
        assert_eq!(manifest.kind, ResourceKind::Instance);

        let _spec: InstanceSpec = serde_json::from_value(manifest.spec).unwrap();
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
        assert!(spec
            .persona
            .unwrap()
            .immutable_anchor
            .unwrap()
            .contains("Sera"));
        assert_eq!(spec.tools.unwrap().allow.len(), 4);
        assert!(spec.features.is_default());
    }

    #[test]
    fn raw_manifest_parse_agent_hermes_parity_feature_surfaces() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: sera
spec:
  provider: minimax
  model: MiniMax-M2.7
  features:
    identityMemory:
      enabled: true
      soul: true
      durableMemory: true
      userProfile: true
      environment: true
      memoryTool: true
      writePolicy: gated
      memoryCharLimit: 2200
      userCharLimit: 1375
    semanticMemory:
      enabled: true
      backend: sqlite
      topK: 3
      maxInjectedChars: 3000
      writePolicy: gated
    memoryProviders:
      enabled: true
      active: hindsight
      providers:
        hindsight:
          enabled: true
          recall: true
          reflect: true
          retainPolicy: gated
          exposeTools: true
          tags: [sera, default]
    contextEngine:
      enabled: true
      engine: lcm
      exposeTools: true
      toolAllow: [lcm_grep, lcm_expand, lcm_expand_query]
    toolsets:
      memory:
        enabled: false
        builtinMemoryTool: true
        providerTools: false
      context_engine:
        enabled: false
        toolAllow: [lcm_grep, lcm_expand]
      session_search:
        enabled: true
      skills:
        enabled: true
"#;
        let raw: RawManifest = serde_yaml::from_str(yaml).unwrap();
        let manifest = ConfigManifest::from_raw(raw).unwrap();
        let spec: AgentSpec = serde_json::from_value(manifest.spec).unwrap();

        assert!(spec.features.identity_memory.enabled);
        assert!(spec.features.identity_memory.soul);
        assert!(spec.features.identity_memory.durable_memory);
        assert!(spec.features.identity_memory.user_profile);
        assert_eq!(
            spec.features.identity_memory.write_policy.as_deref(),
            Some("gated")
        );
        assert_eq!(spec.features.identity_memory.memory_char_limit, Some(2200));
        assert_eq!(spec.features.identity_memory.user_char_limit, Some(1375));

        assert!(spec.features.semantic_memory.enabled);
        assert_eq!(
            spec.features.semantic_memory.backend.as_deref(),
            Some("sqlite")
        );
        assert_eq!(spec.features.semantic_memory.top_k, Some(3));

        assert!(spec.features.memory_providers.enabled);
        assert_eq!(
            spec.features.memory_providers.active.as_deref(),
            Some("hindsight")
        );
        let hindsight = spec
            .features
            .memory_providers
            .providers
            .get("hindsight")
            .expect("hindsight provider gate");
        assert!(hindsight.enabled);
        assert!(hindsight.recall);
        assert!(hindsight.reflect);
        assert_eq!(hindsight.retain_policy.as_deref(), Some("gated"));
        assert_eq!(hindsight.expose_tools, Some(true));

        assert!(spec.features.context_engine.enabled);
        assert_eq!(spec.features.context_engine.engine.as_deref(), Some("lcm"));
        assert!(spec.features.context_engine.expose_tools);
        assert_eq!(
            spec.features.context_engine.tool_allow,
            vec!["lcm_grep", "lcm_expand", "lcm_expand_query"]
        );

        let memory_toolset = spec.features.toolsets.get("memory").unwrap();
        assert!(
            !memory_toolset.enabled,
            "live memory tools stay gated off by default"
        );
        assert!(memory_toolset.builtin_memory_tool);
        assert!(!memory_toolset.provider_tools);
        assert!(
            spec.features
                .toolsets
                .get("session_search")
                .unwrap()
                .enabled
        );
        assert!(spec.features.toolsets.get("skills").unwrap().enabled);
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
                change_artifact: None,
                shadow: false,
            },
            config_version: CONFIG_VERSION,
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
                change_artifact: None,
                shadow: false,
            },
            config_version: CONFIG_VERSION,
            spec: serde_json::Value::Null,
        };
        let err = ConfigManifest::from_raw(raw).unwrap_err();
        assert!(err.to_string().contains("invalid apiVersion"));
    }

    #[test]
    fn raw_manifest_parse_capability_policy() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: CapabilityPolicy
metadata:
  name: full-dev
spec:
  allowedTools:
    - memory_read
    - memory_write
"#;
        let raw: RawManifest = serde_yaml::from_str(yaml).unwrap();
        let manifest = ConfigManifest::from_raw(raw).unwrap();
        assert_eq!(manifest.kind, ResourceKind::CapabilityPolicy);
        assert_eq!(manifest.metadata.name, "full-dev");
    }

    #[test]
    fn config_version_defaults_to_one_when_absent() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: test
spec: {}
"#;
        let raw: RawManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(raw.config_version, CONFIG_VERSION);
    }

    #[test]
    fn config_version_explicit_value_round_trips() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: test
_configVersion: 2
spec: {}
"#;
        let raw: RawManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(raw.config_version, 2);
        let manifest = ConfigManifest::from_raw(raw).unwrap();
        assert_eq!(manifest.config_version, 2);
    }

    #[test]
    fn metadata_with_labels_and_annotations() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: test
  labels:
    env: local
    team: platform
  annotations:
    description: Test instance
spec: {}
"#;
        let raw: RawManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(raw.metadata.labels.get("env").unwrap(), "local");
        assert_eq!(
            raw.metadata.annotations.get("description").unwrap(),
            "Test instance"
        );
    }
}
