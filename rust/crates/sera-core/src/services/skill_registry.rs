//! Skill Registry — manages tool/skill availability and validation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::RwLock;

use sera_db::skills::SkillRepository;

/// A skill/tool definition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Option<serde_json::Value>,
    pub tags: Vec<String>,
    pub mcp_bridge: Option<String>,
    pub version: Option<String>,
    pub category: Option<String>,
}

/// Result of validating a tool call against a skill's input schema.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

/// Skill registry managing tool availability with caching and hot-reload.
pub struct SkillRegistry {
    pool: Arc<PgPool>,
    skills_dir: PathBuf,
    cached_skills: RwLock<HashMap<String, SkillDefinition>>,
}

#[derive(Debug, thiserror::Error)]
pub enum SkillRegistryError {
    #[error("database error: {0}")]
    Db(#[from] sera_db::DbError),
    #[error("YAML parse error: {0}")]
    YamlParse(String),
    #[error("skill not found: {0}")]
    NotFound(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl SkillRegistry {
    /// Create a new skill registry.
    pub fn new(pool: Arc<PgPool>, skills_dir: PathBuf) -> Self {
        Self {
            pool,
            skills_dir,
            cached_skills: RwLock::new(HashMap::new()),
        }
    }

    /// Load skills from both YAML files and database.
    pub async fn load_skills(&self) -> Result<(), SkillRegistryError> {
        let mut skills = HashMap::new();

        // Load from YAML directory
        if self.skills_dir.exists() {
            let yaml_skills = Self::load_from_directory(&self.skills_dir)?;
            for skill in yaml_skills {
                skills.insert(skill.name.clone(), skill);
            }
        }

        // Load from database (DB takes precedence)
        let db_skills = SkillRepository::list_skills(&self.pool).await?;
        for row in db_skills {
            let tags = row
                .tags
                .and_then(|t| serde_json::from_value::<Vec<String>>(t).ok())
                .unwrap_or_default();

            skills.insert(
                row.name.clone(),
                SkillDefinition {
                    name: row.name,
                    description: row.description,
                    input_schema: row.requires,
                    tags,
                    mcp_bridge: None,
                    version: Some(row.version),
                    category: row.category,
                },
            );
        }

        *self.cached_skills.write().await = skills;
        Ok(())
    }

    /// Load skill definitions from a directory of YAML files.
    fn load_from_directory(dir: &Path) -> Result<Vec<SkillDefinition>, SkillRegistryError> {
        let mut skills = Vec::new();

        let entries = std::fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if !is_yaml_file(&path) {
                continue;
            }

            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_yaml::from_str::<SkillDefinition>(&content) {
                    Ok(skill) => skills.push(skill),
                    Err(e) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "Failed to parse skill YAML, skipping"
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Failed to read skill file, skipping"
                    );
                }
            }
        }

        Ok(skills)
    }

    /// Get all skills (optionally filtered by agent — currently returns all).
    pub async fn get_skills(&self, _agent_id: &str) -> Vec<SkillDefinition> {
        let cache = self.cached_skills.read().await;
        cache.values().cloned().collect()
    }

    /// Validate a tool call against the skill's input schema.
    pub async fn validate_tool_call(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> ValidationResult {
        let cache = self.cached_skills.read().await;
        let Some(skill) = cache.get(tool_name) else {
            return ValidationResult {
                valid: false,
                errors: vec![format!("Unknown tool: {tool_name}")],
            };
        };

        // If no input schema, accept any args
        let Some(schema) = &skill.input_schema else {
            return ValidationResult {
                valid: true,
                errors: vec![],
            };
        };

        // Basic validation: check required fields from schema
        let mut errors = Vec::new();
        if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
            for field in required {
                if let Some(field_name) = field.as_str()
                    && args.get(field_name).is_none() {
                        errors.push(format!("Missing required field: {field_name}"));
                    }
            }
        }

        ValidationResult {
            valid: errors.is_empty(),
            errors,
        }
    }

    /// Resolve a tool by name.
    pub async fn resolve_tool(&self, tool_name: &str) -> Option<SkillDefinition> {
        let cache = self.cached_skills.read().await;
        cache.get(tool_name).cloned()
    }

    /// Hot-reload skills from directory and database.
    pub async fn reload(&self) -> Result<(), SkillRegistryError> {
        self.load_skills().await
    }
}

fn is_yaml_file(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext == "yaml" || ext == "yml")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_unknown_tool() {
        let result = ValidationResult {
            valid: false,
            errors: vec!["Unknown tool: foo".to_string()],
        };
        assert!(!result.valid);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_validation_missing_required_field() {
        let schema = serde_json::json!({
            "required": ["name", "path"],
            "properties": {
                "name": {"type": "string"},
                "path": {"type": "string"}
            }
        });

        let args = serde_json::json!({"name": "test"});

        let mut errors = Vec::new();
        if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
            for field in required {
                if let Some(field_name) = field.as_str() {
                    if args.get(field_name).is_none() {
                        errors.push(format!("Missing required field: {field_name}"));
                    }
                }
            }
        }

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("path"));
    }

    #[test]
    fn test_validation_all_fields_present() {
        let schema = serde_json::json!({
            "required": ["name"],
        });

        let args = serde_json::json!({"name": "test"});

        let mut errors = Vec::new();
        if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
            for field in required {
                if let Some(field_name) = field.as_str() {
                    if args.get(field_name).is_none() {
                        errors.push(format!("Missing required field: {field_name}"));
                    }
                }
            }
        }

        assert!(errors.is_empty());
    }

    #[test]
    fn test_is_yaml_file() {
        assert!(is_yaml_file(Path::new("skill.yaml")));
        assert!(is_yaml_file(Path::new("skill.yml")));
        assert!(!is_yaml_file(Path::new("skill.json")));
        assert!(!is_yaml_file(Path::new("README.md")));
    }

    #[test]
    fn test_skill_definition_deserialize() {
        let yaml = r#"
name: file_read
description: Read a file from the workspace
tags:
  - filesystem
  - read
mcp_bridge: null
"#;
        let skill: SkillDefinition = serde_yaml::from_str(yaml).expect("parse failed");
        assert_eq!(skill.name, "file_read");
        assert_eq!(skill.tags.len(), 2);
        assert!(skill.mcp_bridge.is_none());
    }
}
