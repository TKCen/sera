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
}
