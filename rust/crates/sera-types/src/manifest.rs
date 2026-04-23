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
    fn spec_schedule_roundtrip() {
        let schedule = SpecSchedule {
            name: "daily-summary".to_string(),
            description: Some("Summarize activity daily".to_string()),
            schedule_type: "cron".to_string(),
            expression: "0 9 * * *".to_string(),
            task: "Generate daily summary report".to_string(),
            status: Some("active".to_string()),
            category: Some("reporting".to_string()),
        };
        let json = serde_json::to_string(&schedule).unwrap();
        let parsed: SpecSchedule = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "daily-summary");
        assert_eq!(parsed.schedule_type, "cron");
        assert_eq!(parsed.expression, "0 9 * * *");
        assert_eq!(
            parsed.description.as_deref(),
            Some("Summarize activity daily")
        );
        assert_eq!(parsed.status.as_deref(), Some("active"));
        assert_eq!(parsed.category.as_deref(), Some("reporting"));
    }

    #[test]
    fn spec_schedule_optional_fields_omitted() {
        let schedule = SpecSchedule {
            name: "ping".to_string(),
            description: None,
            schedule_type: "interval".to_string(),
            expression: "30s".to_string(),
            task: "ping health".to_string(),
            status: None,
            category: None,
        };
        let json = serde_json::to_string(&schedule).unwrap();
        assert!(!json.contains("description"));
        assert!(!json.contains("status"));
        assert!(!json.contains("category"));
    }

    #[test]
    fn spec_context_file_roundtrip() {
        let ctx_file = SpecContextFile {
            path: "docs/ARCHITECTURE.md".to_string(),
            label: "architecture".to_string(),
            max_tokens: Some(4096),
            priority: Some("high".to_string()),
        };
        let json = serde_json::to_string(&ctx_file).unwrap();
        let parsed: SpecContextFile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.path, "docs/ARCHITECTURE.md");
        assert_eq!(parsed.label, "architecture");
        assert_eq!(parsed.max_tokens, Some(4096));
        assert_eq!(parsed.priority.as_deref(), Some("high"));
    }

    #[test]
    fn spec_context_file_minimal_omits_optionals() {
        let ctx_file = SpecContextFile {
            path: "README.md".to_string(),
            label: "readme".to_string(),
            max_tokens: None,
            priority: None,
        };
        let json = serde_json::to_string(&ctx_file).unwrap();
        assert!(!json.contains("max_tokens"));
        assert!(!json.contains("priority"));
    }

    #[test]
    fn subagent_allow_entry_full_roundtrip() {
        let entry = SubagentAllowEntry {
            template_ref: "code-reviewer".to_string(),
            max_instances: Some(3),
            lifecycle: Some(LifecycleMode::Ephemeral),
            requires_approval: Some(true),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: SubagentAllowEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.template_ref, "code-reviewer");
        assert_eq!(parsed.max_instances, Some(3));
        assert_eq!(parsed.lifecycle, Some(LifecycleMode::Ephemeral));
        assert_eq!(parsed.requires_approval, Some(true));
    }

    #[test]
    fn subagent_allow_entry_minimal_omits_optionals() {
        let entry = SubagentAllowEntry {
            template_ref: "helper".to_string(),
            max_instances: None,
            lifecycle: None,
            requires_approval: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("max_instances"));
        assert!(!json.contains("lifecycle"));
        assert!(!json.contains("requires_approval"));
    }

    #[test]
    fn fallback_model_roundtrip() {
        let model = FallbackModel {
            provider: "openai".to_string(),
            name: "gpt-4o-mini".to_string(),
            max_complexity: Some(2),
        };
        let json = serde_json::to_string(&model).unwrap();
        let parsed: FallbackModel = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.provider, "openai");
        assert_eq!(parsed.name, "gpt-4o-mini");
        assert_eq!(parsed.max_complexity, Some(2));
    }

    #[test]
    fn fallback_model_without_max_complexity() {
        let model = FallbackModel {
            provider: "anthropic".to_string(),
            name: "claude-haiku".to_string(),
            max_complexity: None,
        };
        let json = serde_json::to_string(&model).unwrap();
        assert!(!json.contains("max_complexity"));
        let parsed: FallbackModel = serde_json::from_str(&json).unwrap();
        assert!(parsed.max_complexity.is_none());
    }

    #[test]
    fn spec_sandbox_construction_and_roundtrip() {
        let sandbox = SpecSandbox {
            image: Some("sera-byoh:latest".to_string()),
            entrypoint: Some(vec!["/bin/sh".to_string()]),
            command: Some(vec!["--run".to_string(), "agent.sh".to_string()]),
            chat_port: Some(8080),
        };
        let json = serde_json::to_string(&sandbox).unwrap();
        let parsed: SpecSandbox = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.image.as_deref(), Some("sera-byoh:latest"));
        assert_eq!(parsed.entrypoint.as_ref().unwrap(), &["/bin/sh"]);
        assert_eq!(parsed.command.as_ref().unwrap().len(), 2);
        assert_eq!(parsed.chat_port, Some(8080));
    }

    #[test]
    fn spec_sandbox_optional_fields_omitted() {
        let sandbox = SpecSandbox {
            image: None,
            entrypoint: None,
            command: None,
            chat_port: None,
        };
        let json = serde_json::to_string(&sandbox).unwrap();
        assert!(!json.contains("image"));
        assert!(!json.contains("entrypoint"));
        assert!(!json.contains("command"));
        assert!(!json.contains("chat_port"));
    }

    #[test]
    fn spec_model_with_fallback_list() {
        let yaml = r#"
apiVersion: sera/v1
kind: AgentTemplate
metadata:
  name: fallback-test
spec:
  model:
    provider: anthropic
    model: claude-opus-4
    temperature: 0.5
    fallback:
      - provider: openai
        name: gpt-4o
        maxComplexity: 2
      - provider: openai
        name: gpt-4o-mini
"#;
        let template: AgentTemplate = serde_yaml::from_str(yaml).unwrap();
        let model = template.spec.model.unwrap();
        assert_eq!(model.provider.as_deref(), Some("anthropic"));
        assert_eq!(model.temperature, Some(0.5));
        let fallback = model.fallback.as_ref().unwrap();
        assert_eq!(fallback.len(), 2);
        assert_eq!(fallback[0].provider, "openai");
        assert_eq!(fallback[0].name, "gpt-4o");
        assert_eq!(fallback[0].max_complexity, Some(2));
        assert!(fallback[1].max_complexity.is_none());
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
        assert_eq!(
            sandbox.image.as_deref(),
            Some("sera-byoh-rust-agent:latest")
        );
    }
}
