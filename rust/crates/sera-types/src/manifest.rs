//! Agent manifest and template types — the YAML contract.
//!
//! These types parse AgentTemplate YAML files from the templates/ directory.
//! The shape must match the Zod schema in core/src/agents/schemas.ts.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::LifecycleMode;

/// Top-level AgentTemplate YAML document.
/// Maps from TS: AgentTemplateSchema in agents/schemas.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplate {
    pub api_version: String,
    pub kind: String,
    pub metadata: TemplateMetadata,
    pub spec: TemplateSpec,
}

/// Template metadata block.
/// Maps from TS: MetadataSchema
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateMetadata {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub builtin: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Template spec — the agent's configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<SpecIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<SpecModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_boundary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<SpecLifecycle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_packages: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<SpecTools>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagents: Option<SpecSubagents>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<SpecResources>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedules: Option<Vec<SpecSchedule>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_files: Option<Vec<SpecContextFile>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SpecSandbox>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecIdentity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principles: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecModel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(rename = "model", skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Vec<FallbackModel>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FallbackModel {
    pub provider: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_complexity: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecLifecycle {
    pub mode: LifecycleMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecTools {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub denied: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecSubagents {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed: Option<Vec<SubagentAllowEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentAllowEntry {
    pub template_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_instances: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<LifecycleMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_approval: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecResources {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_llm_tokens_per_hour: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_llm_tokens_per_day: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecSchedule {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub schedule_type: String,
    pub expression: String,
    pub task: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecContextFile {
    pub path: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecSandbox {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_port: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_template() {
        let yaml = r#"
apiVersion: sera/v1
kind: AgentTemplate
metadata:
  name: example-minimal
  displayName: Minimal Example
spec:
  model:
    provider: openai
    model: gpt-4o
  sandboxBoundary: tier-3
  lifecycle:
    mode: ephemeral
"#;
        let template: AgentTemplate = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(template.metadata.name, "example-minimal");
        assert_eq!(template.spec.sandbox_boundary.as_deref(), Some("tier-3"));
        assert_eq!(
            template.spec.lifecycle.as_ref().unwrap().mode,
            LifecycleMode::Ephemeral
        );
        let model = template.spec.model.unwrap();
        assert_eq!(model.provider.as_deref(), Some("openai"));
        assert_eq!(model.model_name.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn parse_full_template() {
        let yaml = r#"
apiVersion: sera/v1
kind: AgentTemplate
metadata:
  name: example-full
  displayName: Full Example Template
  icon: "🚀"
  category: example
  description: A comprehensive example showing all available fields.
spec:
  identity:
    name: example-agent
  model:
    provider: openai
    model: gpt-4o
  sandboxBoundary: tier-2
  policyRef: default-restricted
  lifecycle:
    mode: persistent
  capabilities:
    network-allowlist: ["api.github.com", "google.com"]
    command-allowlist: ["ls", "echo"]
  skills:
    - shell-exec
    - file-manager
  tools:
    allowed: ["bash", "read_file"]
    denied: ["rm"]
  workspace:
    enabled: true
    persist: true
  memory:
    enabled: true
    strategy: buffer
"#;
        let template: AgentTemplate = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(template.metadata.name, "example-full");
        assert_eq!(template.metadata.icon.as_deref(), Some("🚀"));
        assert_eq!(template.metadata.category.as_deref(), Some("example"));
        assert_eq!(
            template.spec.lifecycle.as_ref().unwrap().mode,
            LifecycleMode::Persistent
        );
        assert_eq!(template.spec.skills.as_ref().unwrap().len(), 2);
        let tools = template.spec.tools.as_ref().unwrap();
        assert_eq!(tools.allowed.as_ref().unwrap().len(), 2);
        assert_eq!(tools.denied.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn parse_byoh_rust_template() {
        let yaml = r#"
apiVersion: sera/v1
kind: AgentTemplate
metadata:
  name: byoh-rust-example
  displayName: BYOH Rust Example
  icon: "🦀"
  builtin: false
  category: examples
  description: Minimal Rust agent demonstrating the BYOH contract.
spec:
  identity:
    role: "Example BYOH Rust agent"
  model:
    name: default
  sandboxBoundary: tier-1
  lifecycle:
    mode: ephemeral
  sandbox:
    image: sera-byoh-rust-agent:latest
"#;
        let template: AgentTemplate = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(template.metadata.name, "byoh-rust-example");
        assert!(!template.metadata.builtin);
        let sandbox = template.spec.sandbox.as_ref().unwrap();
        assert_eq!(sandbox.image.as_deref(), Some("sera-byoh-rust-agent:latest"));
    }
}
