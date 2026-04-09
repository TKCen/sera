//! K8s-style YAML manifest loader for SERA configuration.
//!
//! Parses single-file or multi-file YAML with `---` document separators
//! into typed ConfigManifest objects. This is the MVS config format
//! per SPEC-config §2.4.
//!
//! Secret references (`{ secret: "path/to/secret" }`) are resolved from
//! environment variables: `SERA_SECRET_<PATH>` where path separators
//! become underscores and the whole thing is uppercased.

use sera_domain::config_manifest::{
    AgentSpec, ConfigManifest, ConfigManifestError, ConnectorSpec, InstanceSpec, ProviderSpec,
    RawManifest, ResourceKind,
};
use std::collections::HashMap;
use std::path::Path;

/// All parsed and validated manifests from a SERA config file, organized by kind.
#[derive(Debug, Clone, Default)]
pub struct ManifestSet {
    pub instances: Vec<ConfigManifest>,
    pub providers: Vec<ConfigManifest>,
    pub agents: Vec<ConfigManifest>,
    pub connectors: Vec<ConfigManifest>,
}

impl ManifestSet {
    /// Get the first Instance manifest (there should be exactly one for MVS).
    pub fn instance(&self) -> Option<&ConfigManifest> {
        self.instances.first()
    }

    /// Find a provider by name.
    pub fn provider(&self, name: &str) -> Option<&ConfigManifest> {
        self.providers.iter().find(|m| m.metadata.name == name)
    }

    /// Find an agent by name.
    pub fn agent(&self, name: &str) -> Option<&ConfigManifest> {
        self.agents.iter().find(|m| m.metadata.name == name)
    }

    /// Find a connector by name.
    pub fn connector(&self, name: &str) -> Option<&ConfigManifest> {
        self.connectors.iter().find(|m| m.metadata.name == name)
    }

    /// Get typed InstanceSpec from the first Instance manifest.
    pub fn instance_spec(&self) -> Result<Option<InstanceSpec>, serde_json::Error> {
        match self.instance() {
            Some(m) => Ok(Some(serde_json::from_value(m.spec.clone())?)),
            None => Ok(None),
        }
    }

    /// Get typed ProviderSpec for a named provider.
    pub fn provider_spec(&self, name: &str) -> Result<Option<ProviderSpec>, serde_json::Error> {
        match self.provider(name) {
            Some(m) => Ok(Some(serde_json::from_value(m.spec.clone())?)),
            None => Ok(None),
        }
    }

    /// Get typed AgentSpec for a named agent.
    pub fn agent_spec(&self, name: &str) -> Result<Option<AgentSpec>, serde_json::Error> {
        match self.agent(name) {
            Some(m) => Ok(Some(serde_json::from_value(m.spec.clone())?)),
            None => Ok(None),
        }
    }

    /// Get typed ConnectorSpec for a named connector.
    pub fn connector_spec(&self, name: &str) -> Result<Option<ConnectorSpec>, serde_json::Error> {
        match self.connector(name) {
            Some(m) => Ok(Some(serde_json::from_value(m.spec.clone())?)),
            None => Ok(None),
        }
    }

    /// List all agent names.
    pub fn agent_names(&self) -> Vec<&str> {
        self.agents.iter().map(|m| m.metadata.name.as_str()).collect()
    }

    /// List all connector names.
    pub fn connector_names(&self) -> Vec<&str> {
        self.connectors.iter().map(|m| m.metadata.name.as_str()).collect()
    }
}

/// Parse a YAML string containing one or more `---`-separated manifests.
pub fn parse_manifests(yaml_content: &str) -> Result<ManifestSet, ManifestLoadError> {
    let mut set = ManifestSet::default();

    // serde_yaml doesn't natively handle multi-document YAML,
    // so we split on document separators ourselves.
    let documents = split_yaml_documents(yaml_content);

    for (idx, doc) in documents.iter().enumerate() {
        let trimmed = doc.trim();
        if trimmed.is_empty() || trimmed.chars().all(|c| c == '-' || c.is_whitespace()) {
            continue;
        }

        let raw: RawManifest = serde_yaml::from_str(trimmed).map_err(|e| {
            ManifestLoadError::ParseError {
                document_index: idx,
                source: e,
            }
        })?;

        let manifest = ConfigManifest::from_raw(raw).map_err(|e| {
            ManifestLoadError::ValidationError {
                document_index: idx,
                source: e,
            }
        })?;

        match manifest.kind {
            ResourceKind::Instance => set.instances.push(manifest),
            ResourceKind::Provider => set.providers.push(manifest),
            ResourceKind::Agent => set.agents.push(manifest),
            ResourceKind::Connector => set.connectors.push(manifest),
            other => {
                return Err(ManifestLoadError::UnsupportedKind {
                    kind: other.to_string(),
                    document_index: idx,
                });
            }
        }
    }

    Ok(set)
}

/// Load and parse a YAML manifest file from disk.
pub fn load_manifest_file(path: &Path) -> Result<ManifestSet, ManifestLoadError> {
    let content = std::fs::read_to_string(path).map_err(|e| ManifestLoadError::IoError {
        path: path.display().to_string(),
        source: e,
    })?;
    parse_manifests(&content)
}

/// Resolve a secret reference path to its environment variable value.
///
/// Path `"connectors/discord-main/token"` → env var `SERA_SECRET_CONNECTORS_DISCORD_MAIN_TOKEN`.
pub fn resolve_secret(secret_path: &str) -> Option<String> {
    let env_key = format!(
        "SERA_SECRET_{}",
        secret_path.to_uppercase().replace('/', "_").replace('-', "_")
    );
    std::env::var(&env_key).ok()
}

/// Resolve all secret references in a ConnectorSpec, returning the resolved token value.
pub fn resolve_connector_token(spec: &ConnectorSpec) -> Option<String> {
    spec.token.as_ref().and_then(|r| resolve_secret(&r.secret))
}

/// Resolve the API key for a ProviderSpec.
pub fn resolve_provider_api_key(spec: &ProviderSpec) -> Option<String> {
    spec.api_key.as_ref().and_then(|r| resolve_secret(&r.secret))
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Split a YAML string into separate documents on `---` boundaries.
/// Handles leading `---`, trailing `---`, and `...` document end markers.
fn split_yaml_documents(content: &str) -> Vec<String> {
    let mut documents = Vec::new();
    let mut current = String::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            if !current.trim().is_empty() {
                documents.push(current.clone());
            }
            current.clear();
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }

    if !current.trim().is_empty() {
        documents.push(current);
    }

    documents
}

/// Errors from manifest loading.
#[derive(Debug, thiserror::Error)]
pub enum ManifestLoadError {
    #[error("failed to read config file '{path}': {source}")]
    IoError {
        path: String,
        source: std::io::Error,
    },
    #[error("YAML parse error in document {document_index}: {source}")]
    ParseError {
        document_index: usize,
        source: serde_yaml::Error,
    },
    #[error("validation error in document {document_index}: {source}")]
    ValidationError {
        document_index: usize,
        source: ConfigManifestError,
    },
    #[error("unsupported resource kind '{kind}' in document {document_index} (MVS supports Instance, Provider, Agent, Connector)")]
    UnsupportedKind {
        kind: String,
        document_index: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const MVS_CONFIG: &str = r#"
---
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: my-sera
spec:
  tier: local
---
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: lm-studio
spec:
  kind: openai-compatible
  base_url: "http://localhost:1234/v1"
  default_model: gemma-4-12b
---
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
---
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

    #[test]
    fn parse_full_mvs_config() {
        let set = parse_manifests(MVS_CONFIG).unwrap();
        assert_eq!(set.instances.len(), 1);
        assert_eq!(set.providers.len(), 1);
        assert_eq!(set.agents.len(), 1);
        assert_eq!(set.connectors.len(), 1);
    }

    #[test]
    fn instance_spec_extraction() {
        let set = parse_manifests(MVS_CONFIG).unwrap();
        let spec = set.instance_spec().unwrap().unwrap();
        assert_eq!(spec.tier, "local");
    }

    #[test]
    fn provider_spec_extraction() {
        let set = parse_manifests(MVS_CONFIG).unwrap();
        let spec = set.provider_spec("lm-studio").unwrap().unwrap();
        assert_eq!(spec.kind, "openai-compatible");
        assert_eq!(spec.base_url, "http://localhost:1234/v1");
        assert_eq!(spec.default_model.as_deref(), Some("gemma-4-12b"));
    }

    #[test]
    fn agent_spec_extraction() {
        let set = parse_manifests(MVS_CONFIG).unwrap();
        let spec = set.agent_spec("sera").unwrap().unwrap();
        assert_eq!(spec.provider, "lm-studio");
        assert_eq!(spec.model.as_deref(), Some("gemma-4-12b"));
        let persona = spec.persona.unwrap();
        assert!(persona.immutable_anchor.unwrap().contains("Sera"));
        let tools = spec.tools.unwrap();
        assert_eq!(tools.allow.len(), 4);
        assert!(tools.allow.contains(&"memory_*".to_string()));
    }

    #[test]
    fn connector_spec_extraction() {
        let set = parse_manifests(MVS_CONFIG).unwrap();
        let spec = set.connector_spec("discord-main").unwrap().unwrap();
        assert_eq!(spec.kind, "discord");
        assert_eq!(spec.agent.as_deref(), Some("sera"));
        assert_eq!(spec.token.unwrap().secret, "connectors/discord-main/token");
    }

    #[test]
    fn lookup_by_name() {
        let set = parse_manifests(MVS_CONFIG).unwrap();
        assert!(set.provider("lm-studio").is_some());
        assert!(set.provider("nonexistent").is_none());
        assert!(set.agent("sera").is_some());
        assert!(set.agent("nonexistent").is_none());
        assert!(set.connector("discord-main").is_some());
    }

    #[test]
    fn agent_names_list() {
        let set = parse_manifests(MVS_CONFIG).unwrap();
        assert_eq!(set.agent_names(), vec!["sera"]);
    }

    #[test]
    fn parse_single_manifest() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: test
spec:
  tier: local
"#;
        let set = parse_manifests(yaml).unwrap();
        assert_eq!(set.instances.len(), 1);
        assert_eq!(set.providers.len(), 0);
    }

    #[test]
    fn parse_empty_string() {
        let set = parse_manifests("").unwrap();
        assert_eq!(set.instances.len(), 0);
    }

    #[test]
    fn parse_only_separators() {
        let set = parse_manifests("---\n---\n---").unwrap();
        assert_eq!(set.instances.len(), 0);
    }

    #[test]
    fn parse_invalid_kind() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Bogus
metadata:
  name: test
spec: {}
"#;
        let err = parse_manifests(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown resource kind"));
    }

    #[test]
    fn parse_invalid_yaml() {
        let yaml = "this is not: [valid yaml: {";
        let err = parse_manifests(yaml).unwrap_err();
        assert!(matches!(err, ManifestLoadError::ParseError { .. }));
    }

    #[test]
    fn parse_bad_api_version() {
        let yaml = r#"
apiVersion: noslash
kind: Instance
metadata:
  name: test
spec:
  tier: local
"#;
        let err = parse_manifests(yaml).unwrap_err();
        assert!(err.to_string().contains("invalid apiVersion"));
    }

    #[test]
    fn resolve_secret_from_env() {
        // Set env var for test
        unsafe {
            std::env::set_var("SERA_SECRET_CONNECTORS_DISCORD_MAIN_TOKEN", "test-token-123");
        }
        let result = resolve_secret("connectors/discord-main/token");
        assert_eq!(result.as_deref(), Some("test-token-123"));

        // Clean up
        unsafe {
            std::env::remove_var("SERA_SECRET_CONNECTORS_DISCORD_MAIN_TOKEN");
        }
    }

    #[test]
    fn resolve_secret_missing() {
        let result = resolve_secret("nonexistent/secret/path");
        assert!(result.is_none());
    }

    #[test]
    fn resolve_connector_token_integration() {
        unsafe {
            std::env::set_var("SERA_SECRET_PROVIDERS_OPENAI_API_KEY", "sk-test");
        }
        let spec = ProviderSpec {
            kind: "openai".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            default_model: None,
            api_key: Some(sera_domain::config_manifest::SecretRef {
                secret: "providers/openai/api-key".to_string(),
            }),
        };
        let key = resolve_provider_api_key(&spec);
        assert_eq!(key.as_deref(), Some("sk-test"));

        unsafe {
            std::env::remove_var("SERA_SECRET_PROVIDERS_OPENAI_API_KEY");
        }
    }

    #[test]
    fn split_yaml_documents_basic() {
        let docs = split_yaml_documents("a: 1\n---\nb: 2");
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn split_yaml_documents_leading_separator() {
        let docs = split_yaml_documents("---\na: 1\n---\nb: 2");
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn split_yaml_documents_trailing_separator() {
        let docs = split_yaml_documents("a: 1\n---\nb: 2\n---");
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn split_yaml_handles_dot_dot_dot() {
        let docs = split_yaml_documents("a: 1\n...\nb: 2");
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn load_manifest_file_not_found() {
        let err = load_manifest_file(Path::new("/nonexistent/sera.yaml")).unwrap_err();
        assert!(matches!(err, ManifestLoadError::IoError { .. }));
    }

    #[test]
    fn load_manifest_file_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sera.yaml");
        std::fs::write(&path, MVS_CONFIG).unwrap();

        let set = load_manifest_file(&path).unwrap();
        assert_eq!(set.instances.len(), 1);
        assert_eq!(set.agents.len(), 1);
    }

    #[test]
    fn multiple_agents() {
        let yaml = r#"
---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: sera
spec:
  provider: lm-studio
  model: gemma-4-12b
---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: reviewer
spec:
  provider: lm-studio
  model: gemma-4-12b
"#;
        let set = parse_manifests(yaml).unwrap();
        assert_eq!(set.agents.len(), 2);
        assert_eq!(set.agent_names(), vec!["sera", "reviewer"]);
    }
}
