//! K8s-style YAML manifest loader for SERA configuration.
//!
//! Parses single-file or multi-file YAML with `---` document separators
//! into typed ConfigManifest objects. This is the MVS config format
//! per SPEC-config §2.4.
//!
//! Secret references (`{ secret: "path/to/secret" }`) are resolved from
//! environment variables: `SERA_SECRET_<PATH>` where path separators
//! become underscores and the whole thing is uppercased.

use sera_types::config_manifest::{
    AgentSpec, ConfigManifest, ConfigManifestError, ConnectorSpec, InstanceSpec, ProviderSpec,
    RawManifest, ResourceKind,
};
use sera_types::hook::HookChain;
use std::path::Path;

/// All parsed and validated manifests from a SERA config file, organized by kind.
#[derive(Debug, Clone, Default)]
pub struct ManifestSet {
    pub instances: Vec<ConfigManifest>,
    pub providers: Vec<ConfigManifest>,
    pub agents: Vec<ConfigManifest>,
    pub connectors: Vec<ConfigManifest>,
    pub hook_chains: Vec<ConfigManifest>,
    pub capability_policies: Vec<ConfigManifest>,
    pub workflow_defs: Vec<ConfigManifest>,
}

impl ManifestSet {
    /// Append all manifests from `other` into this set, kind-by-kind.
    /// Used by [`load_manifest_dir`] to fold multiple files into one set.
    pub fn merge_in(&mut self, other: ManifestSet) {
        self.instances.extend(other.instances);
        self.providers.extend(other.providers);
        self.agents.extend(other.agents);
        self.connectors.extend(other.connectors);
        self.hook_chains.extend(other.hook_chains);
        self.capability_policies.extend(other.capability_policies);
        self.workflow_defs.extend(other.workflow_defs);
    }
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

    /// Find a hook chain by name.
    pub fn hook_chain(&self, name: &str) -> Option<&ConfigManifest> {
        self.hook_chains.iter().find(|m| m.metadata.name == name)
    }

    /// Get all typed HookChain specs.
    pub fn hook_chain_specs(&self) -> Vec<HookChain> {
        self.hook_chains
            .iter()
            .filter_map(|m| serde_json::from_value(m.spec.clone()).ok())
            .collect()
    }

    /// List all hook chain names.
    pub fn hook_chain_names(&self) -> Vec<&str> {
        self.hook_chains.iter().map(|m| m.metadata.name.as_str()).collect()
    }

    /// Find a capability policy by name.
    pub fn capability_policy(&self, name: &str) -> Option<&ConfigManifest> {
        self.capability_policies.iter().find(|m| m.metadata.name == name)
    }

    /// List all capability-policy names.
    pub fn capability_policy_names(&self) -> Vec<&str> {
        self.capability_policies
            .iter()
            .map(|m| m.metadata.name.as_str())
            .collect()
    }

    /// Find a workflow definition by name.
    pub fn workflow_def(&self, name: &str) -> Option<&ConfigManifest> {
        self.workflow_defs.iter().find(|m| m.metadata.name == name)
    }

    /// List all workflow-def names.
    pub fn workflow_def_names(&self) -> Vec<&str> {
        self.workflow_defs
            .iter()
            .map(|m| m.metadata.name.as_str())
            .collect()
    }

    /// Insert or replace an agent manifest by name.
    ///
    /// If an agent with `name` already exists it is replaced in-place;
    /// otherwise a new manifest is appended. Returns `true` when the agent
    /// was replaced, `false` when it was newly inserted.
    pub fn upsert_agent(&mut self, manifest: ConfigManifest) -> bool {
        if let Some(pos) = self.agents.iter().position(|m| m.metadata.name == manifest.metadata.name) {
            self.agents[pos] = manifest;
            true
        } else {
            self.agents.push(manifest);
            false
        }
    }

    /// Remove an agent manifest by name.
    ///
    /// Returns `true` when the agent was found and removed, `false` when it
    /// did not exist.
    pub fn remove_agent(&mut self, name: &str) -> bool {
        let before = self.agents.len();
        self.agents.retain(|m| m.metadata.name != name);
        self.agents.len() < before
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
            ResourceKind::HookChain => set.hook_chains.push(manifest),
            ResourceKind::CapabilityPolicy => set.capability_policies.push(manifest),
            ResourceKind::WorkflowDef => set.workflow_defs.push(manifest),
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

/// Recursively load every `*.yaml` and `*.yml` file under `dir` and merge
/// them into a single [`ManifestSet`]. Files are processed in sorted order
/// for deterministic output. Hidden files (leading `.`) are skipped.
///
/// A missing `dir` returns an empty [`ManifestSet`] (not an error) — this
/// matches the convention used by [`crate::config_root::ConfigRoot`]
/// subdirectories, which only exist after a user has populated them.
pub fn load_manifest_dir(dir: &Path) -> Result<ManifestSet, ManifestLoadError> {
    if !dir.exists() {
        return Ok(ManifestSet::default());
    }
    if !dir.is_dir() {
        return Err(ManifestLoadError::NotADirectory {
            path: dir.display().to_string(),
        });
    }

    let mut set = ManifestSet::default();
    let mut files = collect_yaml_files(dir)?;
    files.sort();

    for file in files {
        let parsed = load_manifest_file(&file)?;
        set.merge_in(parsed);
    }

    Ok(set)
}

/// Walk `dir` recursively and collect every `*.yaml` / `*.yml` regular file.
/// Skips entries whose file name begins with `.` (dotfiles, editor swap files).
fn collect_yaml_files(dir: &Path) -> Result<Vec<std::path::PathBuf>, ManifestLoadError> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current).map_err(|e| ManifestLoadError::IoError {
            path: current.display().to_string(),
            source: e,
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| ManifestLoadError::IoError {
                path: current.display().to_string(),
                source: e,
            })?;
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if name.starts_with('.') {
                continue;
            }

            let file_type = entry.file_type().map_err(|e| ManifestLoadError::IoError {
                path: path.display().to_string(),
                source: e,
            })?;

            if file_type.is_dir() {
                stack.push(path);
                continue;
            }

            if !file_type.is_file() {
                continue;
            }

            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "yaml" || ext == "yml" {
                out.push(path);
            }
        }
    }

    Ok(out)
}

/// Resolve a secret reference path. Checks env vars only (legacy API).
/// Prefer `resolve_secret_with` when a `SecretResolver` is available.
pub fn resolve_secret(secret_path: &str) -> Option<String> {
    crate::secrets::resolve_from_env(secret_path)
}

/// Resolve a secret using the file-based resolver (with env var fallback).
pub fn resolve_secret_with(
    secret_path: &str,
    resolver: &crate::secrets::SecretResolver,
) -> Option<String> {
    resolver.resolve(secret_path)
}

/// Resolve all secret references in a ConnectorSpec, returning the resolved token value.
pub fn resolve_connector_token(spec: &ConnectorSpec) -> Option<String> {
    spec.token.as_ref().and_then(|r| resolve_secret(&r.secret))
}

/// Resolve connector token using a SecretResolver (file + env fallback).
pub fn resolve_connector_token_with(
    spec: &ConnectorSpec,
    resolver: &crate::secrets::SecretResolver,
) -> Option<String> {
    spec.token
        .as_ref()
        .and_then(|r| resolve_secret_with(&r.secret, resolver))
}

/// Resolve the API key for a ProviderSpec.
pub fn resolve_provider_api_key(spec: &ProviderSpec) -> Option<String> {
    spec.api_key.as_ref().and_then(|r| resolve_secret(&r.secret))
}

/// Resolve provider API key using a SecretResolver (file + env fallback).
pub fn resolve_provider_api_key_with(
    spec: &ProviderSpec,
    resolver: &crate::secrets::SecretResolver,
) -> Option<String> {
    spec.api_key
        .as_ref()
        .and_then(|r| resolve_secret_with(&r.secret, resolver))
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
    #[error("unsupported resource kind '{kind}' in document {document_index} (supported: Instance, Provider, Agent, Connector, HookChain, CapabilityPolicy, WorkflowDef)")]
    UnsupportedKind {
        kind: String,
        document_index: usize,
    },
    #[error("expected directory at '{path}', found a file")]
    NotADirectory { path: String },
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
spec: {}
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
        assert!(spec.docs_dir.is_none());
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
        // peer_bots defaults to empty when absent — strict default
        // (sera-yeg.1): no peer bots accepted unless explicitly listed.
        assert!(spec.peer_bots.is_empty());
    }

    #[test]
    fn connector_spec_peer_bots_parses_list() {
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
  peer_bots:
    - "987654321012345678"
    - "Sera 2.0"
"#;
        let set = parse_manifests(yaml).unwrap();
        let spec = set.connector_spec("discord-main").unwrap().unwrap();
        assert_eq!(
            spec.peer_bots,
            vec!["987654321012345678".to_string(), "Sera 2.0".to_string()],
        );
    }

    #[test]
    fn connector_spec_peer_bots_accepts_camel_case_alias() {
        // YAML conventions vary; accept both `peer_bots` and `peerBots`
        // so operators following the camelCase convention used by some
        // other specs in this repo aren't surprised.
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Connector
metadata:
  name: discord-main
spec:
  kind: discord
  agent: sera
  peerBots:
    - hermes-bot-id
"#;
        let set = parse_manifests(yaml).unwrap();
        let spec = set.connector_spec("discord-main").unwrap().unwrap();
        assert_eq!(spec.peer_bots, vec!["hermes-bot-id".to_string()]);
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
spec: {}
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
    fn invalid_yaml_error_includes_line_number() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: test
spec:
  bad: [unclosed
"#;
        let err = parse_manifests(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            matches!(err, ManifestLoadError::ParseError { .. }),
            "expected ParseError, got: {err:?}",
        );
        assert!(
            msg.to_lowercase().contains("line"),
            "parse error should surface line info to make debugging tractable; got: {msg}",
        );
    }

    #[test]
    fn parse_bad_api_version() {
        let yaml = r#"
apiVersion: noslash
kind: Instance
metadata:
  name: test
spec: {}
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
            api_key: Some(sera_types::config_manifest::SecretRef {
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
    fn parse_capability_policy_manifest() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: CapabilityPolicy
metadata:
  name: read-only
spec:
  allowedTools:
    - memory_read
    - file_read
"#;
        let set = parse_manifests(yaml).unwrap();
        assert_eq!(set.capability_policies.len(), 1);
        assert_eq!(set.capability_policy("read-only").unwrap().metadata.name, "read-only");
        assert_eq!(set.capability_policy_names(), vec!["read-only"]);
    }

    #[test]
    fn parse_workflow_def_manifest() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: WorkflowDef
metadata:
  name: nightly-recap
spec:
  trigger: cron
  schedule: "0 2 * * *"
"#;
        let set = parse_manifests(yaml).unwrap();
        assert_eq!(set.workflow_defs.len(), 1);
        assert_eq!(set.workflow_def("nightly-recap").unwrap().metadata.name, "nightly-recap");
        assert_eq!(set.workflow_def_names(), vec!["nightly-recap"]);
    }

    #[test]
    fn load_manifest_dir_missing_returns_empty() {
        let set = load_manifest_dir(Path::new("/nonexistent/sera-config")).unwrap();
        assert_eq!(set.instances.len(), 0);
        assert_eq!(set.capability_policies.len(), 0);
    }

    #[test]
    fn load_manifest_dir_walks_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        let policies = dir.path().join("policies");
        let providers = dir.path().join("providers");
        std::fs::create_dir_all(&policies).unwrap();
        std::fs::create_dir_all(&providers).unwrap();

        std::fs::write(
            policies.join("read-only.yaml"),
            "apiVersion: sera.dev/v1\nkind: CapabilityPolicy\nmetadata:\n  name: read-only\nspec:\n  allowedTools: [memory_read]\n",
        ).unwrap();
        std::fs::write(
            providers.join("lm-studio.yml"),
            "apiVersion: sera.dev/v1\nkind: Provider\nmetadata:\n  name: lm-studio\nspec:\n  kind: openai-compatible\n  base_url: \"http://localhost:1234/v1\"\n",
        ).unwrap();
        // Ignored — wrong extension
        std::fs::write(policies.join("notes.txt"), "ignored").unwrap();
        // Ignored — dotfile
        std::fs::write(policies.join(".hidden.yaml"), "ignored").unwrap();

        let set = load_manifest_dir(dir.path()).unwrap();
        assert_eq!(set.capability_policies.len(), 1);
        assert_eq!(set.providers.len(), 1);
        assert_eq!(set.capability_policy_names(), vec!["read-only"]);
    }

    #[test]
    fn load_manifest_dir_rejects_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-a-dir.yaml");
        std::fs::write(&path, "apiVersion: sera.dev/v1\nkind: Instance\nmetadata: { name: x }\nspec: {}\n").unwrap();
        let err = load_manifest_dir(&path).unwrap_err();
        assert!(matches!(err, ManifestLoadError::NotADirectory { .. }));
    }

    #[test]
    fn parse_hook_chain_manifest() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: HookChain
metadata:
  name: content-filter-chain
spec:
  name: content-filter-chain
  point: pre_route
  hooks:
    - hook_ref: rate-limiter
      config:
        requests_per_minute: 60
      enabled: true
  timeout_ms: 5000
  fail_open: true
"#;
        let set = parse_manifests(yaml).unwrap();
        assert_eq!(set.hook_chains.len(), 1);
        assert_eq!(set.hook_chain_names(), vec!["content-filter-chain"]);
        let specs = set.hook_chain_specs();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "content-filter-chain");
        assert_eq!(specs[0].point, sera_types::hook::HookPoint::PreRoute);
        assert_eq!(specs[0].hooks.len(), 1);
        assert!(specs[0].fail_open);
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
