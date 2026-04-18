//! Skill and tool types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A skill definition (tool available to agents).
/// Maps from TS: SkillDefinition in skills/types.ts
///
/// # AgentSkills markdown extensions
///
/// The optional fields `body`, `triggers`, `model_override`,
/// `context_budget_tokens`, `tool_bindings`, and `mcp_servers` are populated
/// when a skill is loaded from an AgentSkills-compatible markdown file
/// (see `sera-skills::markdown`). They are backward-compatible via
/// `#[serde(default)]` so legacy JSON-sourced definitions still parse.
///
/// ```
/// use sera_types::skill::SkillDefinition;
/// let json = r#"{ "name": "code-review" }"#;
/// let def: SkillDefinition = serde_json::from_str(json).unwrap();
/// assert_eq!(def.name, "code-review");
/// assert!(def.body.is_none());
/// assert!(def.triggers.is_empty());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// Markdown body injected into agent context when this skill is active.
    /// For markdown-sourced skills this replaces [`SkillConfig::context_injection`].
    ///
    /// ```
    /// use sera_types::skill::SkillDefinition;
    /// let def = SkillDefinition {
    ///     name: "x".into(), description: None, version: None, parameters: None,
    ///     source: None, body: Some("You are a reviewer.".into()),
    ///     triggers: vec![], model_override: None, context_budget_tokens: None,
    ///     tool_bindings: vec![], mcp_servers: vec![],
    /// };
    /// assert_eq!(def.body.as_deref(), Some("You are a reviewer."));
    /// ```
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,

    /// Trigger keywords that activate this skill. See AgentSkills `triggers:` frontmatter.
    ///
    /// ```
    /// use sera_types::skill::SkillDefinition;
    /// let def = SkillDefinition {
    ///     name: "x".into(), description: None, version: None, parameters: None,
    ///     source: None, body: None,
    ///     triggers: vec!["review".into(), "audit".into()],
    ///     model_override: None, context_budget_tokens: None,
    ///     tool_bindings: vec![], mcp_servers: vec![],
    /// };
    /// assert_eq!(def.triggers.len(), 2);
    /// ```
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<String>,

    /// Optional model override (e.g. `"claude-opus-4"`). Interpreted by the runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_override: Option<String>,

    /// Context budget hint in tokens. Runtime may truncate `body` to fit this budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_budget_tokens: Option<u32>,

    /// Tool names bound to this skill (moved up from [`SkillConfig`] for
    /// frontmatter parity). Duplicated access is preserved so existing callers
    /// reading [`SkillConfig::tools`] continue to work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_bindings: Vec<String>,

    /// Per-skill MCP server declarations. At wire time a runtime adapter maps
    /// [`SkillMcpServer`] to `sera-mcp::McpServerConfig`; sera-types does not
    /// depend on sera-mcp to keep the crate graph acyclic.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<SkillMcpServer>,
}

/// Transport flavour for a per-skill MCP server declaration.
///
/// Mirrors the three transport variants used by the MCP ecosystem.
/// Kept local to `sera-types` so `SkillDefinition` does not require
/// a dependency on `sera-mcp`; the runtime is responsible for mapping
/// this enum to the concrete `sera-mcp::McpServerConfig` at wire time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillMcpTransport {
    /// Stdio subprocess transport (most AgentSkills declarations use this).
    #[default]
    Stdio,
    /// Server-Sent Events transport.
    Sse,
    /// Streamable HTTP transport.
    StreamableHttp,
}

/// A per-skill MCP server declaration.
///
/// ```
/// use sera_types::skill::{SkillMcpServer, SkillMcpTransport};
/// let server = SkillMcpServer {
///     name: "github".into(),
///     transport: SkillMcpTransport::Stdio,
///     command: Some("npx".into()),
///     args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
///     url: None,
///     env: Default::default(),
/// };
/// let json = serde_json::to_string(&server).unwrap();
/// let back: SkillMcpServer = serde_json::from_str(&json).unwrap();
/// assert_eq!(back.name, "github");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMcpServer {
    /// Server nickname (unique within the declaring skill).
    pub name: String,

    /// Transport flavour. Defaults to `stdio` for backward compat with
    /// AgentSkills frontmatter that omits the field.
    #[serde(default)]
    pub transport: SkillMcpTransport,

    /// Subprocess command (for stdio transports).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Command-line arguments for the subprocess.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,

    /// Endpoint URL (for SSE / streamable HTTP transports).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Extra environment variables to inject when launching the server.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
}

/// Operating mode for a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillMode {
    Active,
    Background,
    OnDemand,
    Disabled,
}

/// Activation trigger for a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SkillTrigger {
    /// Activated by explicit user/agent command.
    Manual,
    /// Activated when an event pattern matches.
    Event(String),
    /// Always active when the agent is running.
    Always,
}

/// Full configuration for a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillConfig {
    pub name: String,
    pub version: String,
    pub description: String,
    pub mode: SkillMode,
    pub trigger: SkillTrigger,
    /// Tool names this skill requires.
    pub tools: Vec<String>,
    /// Text injected into context when this skill is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_injection: Option<String>,
    /// Skill-specific arbitrary configuration.
    pub config: serde_json::Value,
}

/// Runtime state of an active skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillState {
    pub name: String,
    pub mode: SkillMode,
    pub activated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

/// Records a mode transition for a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTransition {
    pub from: SkillMode,
    pub to: SkillMode,
    pub reason: String,
}

/// Errors produced by [`SkillRegistry`] operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SkillError {
    #[error("skill not found: {0}")]
    NotFound(String),
    #[error("skill already active: {0}")]
    AlreadyActive(String),
    #[error("skill already inactive: {0}")]
    AlreadyInactive(String),
    #[error("skill config error: {0}")]
    ConfigError(String),
}

/// In-memory registry of skill configurations and their runtime states.
#[derive(Debug, Default)]
pub struct SkillRegistry {
    skills: std::collections::HashMap<String, SkillConfig>,
    active_states: std::collections::HashMap<String, SkillState>,
}

impl SkillRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a skill configuration.
    pub fn register(&mut self, config: SkillConfig) {
        self.skills.insert(config.name.clone(), config);
    }

    /// Activate a registered skill. Returns the transition record.
    pub fn activate(&mut self, name: &str) -> Result<SkillTransition, SkillError> {
        let config = self
            .skills
            .get(name)
            .ok_or_else(|| SkillError::NotFound(name.to_string()))?;

        if self.active_states.contains_key(name) {
            return Err(SkillError::AlreadyActive(name.to_string()));
        }

        let from = config.mode.clone();
        let state = SkillState {
            name: name.to_string(),
            mode: SkillMode::Active,
            activated_at: Some(chrono::Utc::now()),
            metadata: std::collections::HashMap::new(),
        };
        self.active_states.insert(name.to_string(), state);

        Ok(SkillTransition {
            from,
            to: SkillMode::Active,
            reason: "activated".to_string(),
        })
    }

    /// Deactivate an active skill. Returns the transition record.
    pub fn deactivate(&mut self, name: &str) -> Result<SkillTransition, SkillError> {
        let state = self
            .active_states
            .remove(name)
            .ok_or_else(|| SkillError::AlreadyInactive(name.to_string()))?;

        Ok(SkillTransition {
            from: state.mode,
            to: SkillMode::Disabled,
            reason: "deactivated".to_string(),
        })
    }

    /// Returns all currently active skill states.
    pub fn active_skills(&self) -> Vec<&SkillState> {
        self.active_states.values().collect()
    }

    /// Look up a skill's configuration by name.
    pub fn get_config(&self, name: &str) -> Option<&SkillConfig> {
        self.skills.get(name)
    }

    /// Returns the `context_injection` strings for all active skills that have one.
    pub fn context_injections(&self) -> Vec<&str> {
        self.active_states
            .keys()
            .filter_map(|name| {
                self.skills
                    .get(name)
                    .and_then(|c| c.context_injection.as_deref())
            })
            .collect()
    }
}

/// A knowledge schema defines structured conventions for circle knowledge.
/// Loaded into agent context when writing to the circle's scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeSchema {
    /// Schema name (e.g., "engineering-wiki", "research-notes").
    pub name: String,
    /// Schema version for compatibility tracking.
    pub version: String,
    /// Allowed page types with their naming conventions.
    pub page_types: Vec<PageTypeRule>,
    /// Category definitions for organizing pages.
    pub categories: Vec<CategoryRule>,
    /// Cross-reference requirements between page types.
    pub cross_reference_rules: Vec<CrossReferenceRule>,
    /// Whether to enforce validation or just provide advisory guidance.
    pub enforcement_mode: EnforcementMode,
}

/// Defines a page type and its naming convention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageTypeRule {
    /// Page type name (e.g., "decision", "architecture", "runbook").
    pub name: String,
    /// Naming pattern (e.g., "YYYY-MM-DD-<slug>").
    pub naming_pattern: String,
    /// Required frontmatter fields.
    pub required_fields: Vec<String>,
    /// Optional description of this page type.
    pub description: Option<String>,
}

/// Defines a category for organizing pages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryRule {
    pub name: String,
    /// Allowed page types in this category.
    pub allowed_page_types: Vec<String>,
    pub description: Option<String>,
}

/// Defines cross-reference requirements between page types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossReferenceRule {
    /// Source page type that must reference the target.
    pub from_type: String,
    /// Target page type that must be referenced.
    pub to_type: String,
    /// Whether this cross-reference is required or optional.
    pub required: bool,
}

/// Whether schema rules are enforced or advisory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnforcementMode {
    /// Reject non-conforming writes.
    Enforced,
    /// Warn but allow non-conforming writes.
    Advisory,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_skill_def(name: &str) -> SkillDefinition {
        SkillDefinition {
            name: name.to_string(),
            description: None,
            version: None,
            parameters: None,
            source: None,
            body: None,
            triggers: vec![],
            model_override: None,
            context_budget_tokens: None,
            tool_bindings: vec![],
            mcp_servers: vec![],
        }
    }

    #[test]
    fn skill_definition_minimal() {
        let skill = empty_skill_def("shell-exec");
        let json = serde_json::to_string(&skill).unwrap();
        assert!(json.contains("\"name\":\"shell-exec\""));
        assert!(!json.contains("description"));
        let parsed: SkillDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "shell-exec");
    }

    #[test]
    fn skill_definition_full() {
        let mut skill = empty_skill_def("file-manager");
        skill.description = Some("Manage files on the filesystem".to_string());
        skill.version = Some("1.0.0".to_string());
        skill.parameters = Some(serde_json::json!({
            "operations": ["read", "write", "delete"],
            "max_file_size_mb": 100
        }));
        skill.source = Some("builtin".to_string());
        let json = serde_json::to_string(&skill).unwrap();
        let parsed: SkillDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "file-manager");
        assert_eq!(parsed.version, Some("1.0.0".to_string()));
        let params = parsed.parameters.unwrap();
        assert_eq!(params["operations"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn skill_definition_json_roundtrip() {
        let mut skill = empty_skill_def("network-request");
        skill.description = Some("Make HTTP requests".to_string());
        skill.version = Some("2.0.0".to_string());
        skill.source = Some("custom".to_string());
        let json = serde_json::to_string(&skill).unwrap();
        let parsed: SkillDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(skill.name, parsed.name);
        assert_eq!(skill.description, parsed.description);
        assert_eq!(skill.version, parsed.version);
    }

    #[test]
    fn skill_definition_markdown_extensions() {
        let mut skill = empty_skill_def("code-review");
        skill.body = Some("You are a reviewer.".to_string());
        skill.triggers = vec!["review".into(), "audit".into()];
        skill.model_override = Some("claude-opus-4".into());
        skill.context_budget_tokens = Some(4096);
        skill.tool_bindings = vec!["read_file".into()];
        skill.mcp_servers = vec![SkillMcpServer {
            name: "github".into(),
            transport: SkillMcpTransport::Stdio,
            command: Some("npx".into()),
            args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
            url: None,
            env: HashMap::new(),
        }];
        let json = serde_json::to_string(&skill).unwrap();
        let parsed: SkillDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.body.as_deref(), Some("You are a reviewer."));
        assert_eq!(parsed.triggers, vec!["review", "audit"]);
        assert_eq!(parsed.model_override.as_deref(), Some("claude-opus-4"));
        assert_eq!(parsed.context_budget_tokens, Some(4096));
        assert_eq!(parsed.tool_bindings, vec!["read_file"]);
        assert_eq!(parsed.mcp_servers.len(), 1);
        assert_eq!(parsed.mcp_servers[0].name, "github");
        assert_eq!(parsed.mcp_servers[0].transport, SkillMcpTransport::Stdio);
    }

    #[test]
    fn skill_definition_legacy_json_back_compat() {
        // Legacy JSON without any markdown extensions must still parse.
        let legacy = r#"{"name":"legacy","version":"1.0.0"}"#;
        let parsed: SkillDefinition = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.name, "legacy");
        assert!(parsed.body.is_none());
        assert!(parsed.triggers.is_empty());
        assert!(parsed.tool_bindings.is_empty());
        assert!(parsed.mcp_servers.is_empty());
    }

    #[test]
    fn skill_definition_yaml_parse() {
        let yaml = r#"
name: code-analysis
description: Analyze code for quality and security
version: 1.5.0
source: marketplace
parameters:
  languages:
    - python
    - rust
    - typescript
  checks:
    - lint
    - type-check
"#;
        let skill: SkillDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(skill.name, "code-analysis");
        assert_eq!(skill.version, Some("1.5.0".to_string()));
        let params = skill.parameters.unwrap();
        assert_eq!(params["languages"].as_array().unwrap().len(), 3);
    }

    // --- SkillMode / SkillRegistry tests ---

    fn make_config(name: &str) -> SkillConfig {
        SkillConfig {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: "test skill".to_string(),
            mode: SkillMode::OnDemand,
            trigger: SkillTrigger::Manual,
            tools: vec!["tool-a".to_string()],
            context_injection: Some(format!("Injected context for {name}")),
            config: serde_json::json!({}),
        }
    }

    #[test]
    fn skill_mode_serde_roundtrip() {
        for mode in [
            SkillMode::Active,
            SkillMode::Background,
            SkillMode::OnDemand,
            SkillMode::Disabled,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let parsed: SkillMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, parsed);
        }
        // Verify snake_case serialisation
        assert_eq!(
            serde_json::to_string(&SkillMode::OnDemand).unwrap(),
            "\"on_demand\""
        );
    }

    #[test]
    fn register_and_activate_skill() {
        let mut registry = SkillRegistry::new();
        registry.register(make_config("my-skill"));

        let transition = registry.activate("my-skill").unwrap();
        assert_eq!(transition.from, SkillMode::OnDemand);
        assert_eq!(transition.to, SkillMode::Active);

        let active = registry.active_skills();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "my-skill");
        assert_eq!(active[0].mode, SkillMode::Active);
        assert!(active[0].activated_at.is_some());
    }

    #[test]
    fn deactivate_skill() {
        let mut registry = SkillRegistry::new();
        registry.register(make_config("my-skill"));
        registry.activate("my-skill").unwrap();

        let transition = registry.deactivate("my-skill").unwrap();
        assert_eq!(transition.from, SkillMode::Active);
        assert_eq!(transition.to, SkillMode::Disabled);

        assert!(registry.active_skills().is_empty());
    }

    #[test]
    fn context_injection_from_active_skills() {
        let mut registry = SkillRegistry::new();
        registry.register(make_config("skill-a"));
        let mut cfg_b = make_config("skill-b");
        cfg_b.context_injection = None;
        registry.register(cfg_b);

        registry.activate("skill-a").unwrap();
        registry.activate("skill-b").unwrap();

        let injections = registry.context_injections();
        // Only skill-a has context_injection
        assert_eq!(injections.len(), 1);
        assert!(injections[0].contains("skill-a"));
    }

    #[test]
    fn activate_unknown_skill_returns_error() {
        let mut registry = SkillRegistry::new();
        let err = registry.activate("nonexistent").unwrap_err();
        assert!(matches!(err, SkillError::NotFound(_)));
    }

    #[test]
    fn activate_already_active_returns_error() {
        let mut registry = SkillRegistry::new();
        registry.register(make_config("my-skill"));
        registry.activate("my-skill").unwrap();
        let err = registry.activate("my-skill").unwrap_err();
        assert!(matches!(err, SkillError::AlreadyActive(_)));
    }

    #[test]
    fn deactivate_inactive_skill_returns_error() {
        let mut registry = SkillRegistry::new();
        registry.register(make_config("my-skill"));
        let err = registry.deactivate("my-skill").unwrap_err();
        assert!(matches!(err, SkillError::AlreadyInactive(_)));
    }
}
