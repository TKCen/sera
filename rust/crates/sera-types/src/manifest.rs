//! Agent manifest and template types — the YAML contract.
//!
//! These types parse AgentTemplate YAML files from the templates/ directory.
//! The shape must match the Zod schema in core/src/agents/schemas.ts.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::sandbox::{EgressEndpoint, EgressManifest, EgressMode};
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
    /// Declared egress allow-list for sandboxed agents.
    ///
    /// When this template is sandboxed (`sandbox` or `sandbox_boundary` is
    /// set) and `mode != Disabled`, [`EgressEndpoint::InferenceLocal`] must
    /// be present in `allowed` — `validate_and_normalize_egress` either
    /// inserts it as a baseline (when `egress` is absent) or rejects the
    /// manifest (when `egress` is present but drops it). See `sera-eq0m`
    /// and `sera-xgb0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub egress: Option<EgressManifest>,
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

/// Errors raised by [`AgentTemplate::validate_and_normalize_egress`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EgressManifestError {
    /// Sandboxed manifest declared an `egress` block but dropped
    /// `InferenceLocal` from `allowed` while `mode != Disabled`.
    #[error(
        "sandboxed AgentTemplate '{name}' must allow EgressEndpoint::InferenceLocal \
         (the canonical inference.local proxy) unless egress.mode is 'disabled'"
    )]
    MissingInferenceLocalBaseline { name: String },
}

impl AgentTemplate {
    /// Returns true when this template runs in any sandbox — either an
    /// explicit `spec.sandbox` block or a `spec.sandboxBoundary` reference.
    pub fn is_sandboxed(&self) -> bool {
        self.spec.sandbox.is_some() || self.spec.sandbox_boundary.is_some()
    }

    /// Apply the implicit `InferenceLocal` baseline to a sandboxed template
    /// and reject manifests that opt out of it without `mode = Disabled`.
    ///
    /// Behaviour:
    ///
    /// - Non-sandboxed template: no-op. `egress` is left untouched.
    /// - Sandboxed template with no `egress`: an `EgressManifest` with
    ///   `mode = Strict` and `allowed = [InferenceLocal]` is inserted.
    /// - Sandboxed template with `egress.mode = Disabled`: left untouched
    ///   (compliance opt-out — `InferenceLocal` is not required).
    /// - Sandboxed template with `egress` and `mode != Disabled`: must
    ///   already include `InferenceLocal` in `allowed`, or
    ///   [`EgressManifestError::MissingInferenceLocalBaseline`] is
    ///   returned. The `allowed` list is left exactly as the operator
    ///   declared it (no silent re-adds beyond the absent-egress case).
    pub fn validate_and_normalize_egress(&mut self) -> Result<(), EgressManifestError> {
        if !self.is_sandboxed() {
            return Ok(());
        }

        match self.spec.egress.as_mut() {
            None => {
                self.spec.egress = Some(EgressManifest {
                    allowed: vec![EgressEndpoint::InferenceLocal],
                    mode: EgressMode::Strict,
                });
                Ok(())
            }
            Some(manifest) => {
                if manifest.mode == EgressMode::Disabled {
                    return Ok(());
                }
                if manifest.allows_inference_local() {
                    return Ok(());
                }
                Err(EgressManifestError::MissingInferenceLocalBaseline {
                    name: self.metadata.name.clone(),
                })
            }
        }
    }
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

    fn make_minimal_template(name: &str) -> AgentTemplate {
        AgentTemplate {
            api_version: "sera/v1".to_string(),
            kind: "AgentTemplate".to_string(),
            metadata: TemplateMetadata {
                name: name.to_string(),
                display_name: None,
                icon: None,
                builtin: false,
                category: None,
                description: None,
            },
            spec: TemplateSpec {
                identity: None,
                model: None,
                sandbox_boundary: None,
                policy_ref: None,
                lifecycle: None,
                capabilities: None,
                skills: None,
                skill_packages: None,
                tools: None,
                subagents: None,
                resources: None,
                workspace: None,
                memory: None,
                schedules: None,
                context_files: None,
                sandbox: None,
                egress: None,
            },
        }
    }

    #[test]
    fn validate_egress_no_sandbox_is_noop() {
        let mut t = make_minimal_template("plain");
        assert!(!t.is_sandboxed());
        assert!(t.validate_and_normalize_egress().is_ok());
        assert!(t.spec.egress.is_none());
    }

    #[test]
    fn validate_egress_sandboxed_defaults_to_inference_local_baseline() {
        let mut t = make_minimal_template("sandboxed");
        t.spec.sandbox = Some(SpecSandbox {
            image: Some("img".to_string()),
            entrypoint: None,
            command: None,
            chat_port: None,
        });

        assert!(t.is_sandboxed());
        assert!(t.spec.egress.is_none());

        t.validate_and_normalize_egress().unwrap();

        let egress = t.spec.egress.as_ref().expect("baseline must be inserted");
        assert_eq!(egress.mode, EgressMode::Strict);
        assert!(egress.allows_inference_local());
        assert_eq!(egress.allowed.len(), 1);
    }

    #[test]
    fn validate_egress_sandbox_boundary_only_also_triggers_baseline() {
        let mut t = make_minimal_template("boundary-only");
        t.spec.sandbox_boundary = Some("tier-2".to_string());
        t.validate_and_normalize_egress().unwrap();
        assert!(
            t.spec
                .egress
                .as_ref()
                .unwrap()
                .allows_inference_local()
        );
    }

    #[test]
    fn validate_egress_existing_manifest_with_inference_local_is_preserved() {
        let mut t = make_minimal_template("explicit");
        t.spec.sandbox_boundary = Some("tier-2".to_string());
        t.spec.egress = Some(EgressManifest {
            allowed: vec![
                EgressEndpoint::InferenceLocal,
                EgressEndpoint::Domain {
                    name: "api.github.com".to_string(),
                },
            ],
            mode: EgressMode::Strict,
        });

        let original = t.spec.egress.clone();
        t.validate_and_normalize_egress().unwrap();
        assert_eq!(t.spec.egress, original, "operator-declared list must not be mutated");
    }

    #[test]
    fn validate_egress_rejects_sandboxed_manifest_missing_inference_local() {
        let mut t = make_minimal_template("strip-inference");
        t.spec.sandbox = Some(SpecSandbox {
            image: Some("img".to_string()),
            entrypoint: None,
            command: None,
            chat_port: None,
        });
        t.spec.egress = Some(EgressManifest {
            allowed: vec![EgressEndpoint::Domain {
                name: "api.github.com".to_string(),
            }],
            mode: EgressMode::Strict,
        });

        let err = t.validate_and_normalize_egress().unwrap_err();
        assert_eq!(
            err,
            EgressManifestError::MissingInferenceLocalBaseline {
                name: "strip-inference".to_string()
            }
        );
    }

    #[test]
    fn validate_egress_audit_only_still_requires_inference_local() {
        let mut t = make_minimal_template("audit-only-no-inference");
        t.spec.sandbox_boundary = Some("tier-1".to_string());
        t.spec.egress = Some(EgressManifest {
            allowed: vec![EgressEndpoint::Cidr {
                range: "10.0.0.0/8".to_string(),
            }],
            mode: EgressMode::AuditOnly,
        });

        assert!(t.validate_and_normalize_egress().is_err());
    }

    #[test]
    fn validate_egress_disabled_mode_allows_dropping_inference_local() {
        let mut t = make_minimal_template("opted-out");
        t.spec.sandbox_boundary = Some("tier-3".to_string());
        t.spec.egress = Some(EgressManifest {
            allowed: vec![],
            mode: EgressMode::Disabled,
        });

        t.validate_and_normalize_egress().unwrap();

        let egress = t.spec.egress.as_ref().unwrap();
        assert_eq!(egress.mode, EgressMode::Disabled);
        assert!(egress.allowed.is_empty());
    }

    #[test]
    fn template_egress_field_is_omitted_when_none_for_backwards_compat() {
        let t = make_minimal_template("plain");
        let json = serde_json::to_string(&t).unwrap();
        assert!(!json.contains("egress"));
    }

    #[test]
    fn template_with_egress_yaml_parses() {
        let yaml = r#"
apiVersion: sera/v1
kind: AgentTemplate
metadata:
  name: with-egress
spec:
  sandboxBoundary: tier-2
  egress:
    mode: audit_only
    allowed:
      - type: inference_local
      - type: domain
        name: api.github.com
      - type: cidr
        range: 10.0.0.0/8
"#;
        let template: AgentTemplate = serde_yaml::from_str(yaml).unwrap();
        let egress = template.spec.egress.as_ref().unwrap();
        assert_eq!(egress.mode, EgressMode::AuditOnly);
        assert_eq!(egress.allowed.len(), 3);
        assert!(egress.allows_inference_local());
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
