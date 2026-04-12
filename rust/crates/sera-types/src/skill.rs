//! Skill and tool types.

use serde::{Deserialize, Serialize};

/// A skill definition (tool available to agents).
/// Maps from TS: SkillDefinition in skills/types.ts
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_definition_minimal() {
        let skill = SkillDefinition {
            name: "shell-exec".to_string(),
            description: None,
            version: None,
            parameters: None,
            source: None,
        };
        let json = serde_json::to_string(&skill).unwrap();
        assert!(json.contains("\"name\":\"shell-exec\""));
        assert!(!json.contains("description"));
        let parsed: SkillDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "shell-exec");
    }

    #[test]
    fn skill_definition_full() {
        let skill = SkillDefinition {
            name: "file-manager".to_string(),
            description: Some("Manage files on the filesystem".to_string()),
            version: Some("1.0.0".to_string()),
            parameters: Some(serde_json::json!({
                "operations": ["read", "write", "delete"],
                "max_file_size_mb": 100
            })),
            source: Some("builtin".to_string()),
        };
        let json = serde_json::to_string(&skill).unwrap();
        let parsed: SkillDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "file-manager");
        assert_eq!(parsed.version, Some("1.0.0".to_string()));
        let params = parsed.parameters.unwrap();
        assert_eq!(params["operations"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn skill_definition_json_roundtrip() {
        let skill = SkillDefinition {
            name: "network-request".to_string(),
            description: Some("Make HTTP requests".to_string()),
            version: Some("2.0.0".to_string()),
            parameters: None,
            source: Some("custom".to_string()),
        };
        let json = serde_json::to_string(&skill).unwrap();
        let parsed: SkillDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(skill.name, parsed.name);
        assert_eq!(skill.description, parsed.description);
        assert_eq!(skill.version, parsed.version);
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
