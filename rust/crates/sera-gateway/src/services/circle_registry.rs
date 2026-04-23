//! Circle Registry — merges YAML circle definitions with database state.
//!
//! Provides a unified interface for loading, listing, and querying circles
//! from both YAML manifests and the database.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Circle definition from YAML manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircleDefinition {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: CircleMetadata,
    pub agents: Vec<String>,
    #[serde(default)]
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub knowledge: Option<Knowledge>,
    #[serde(default)]
    pub project_context: Option<ProjectContext>,
    #[serde(default)]
    pub party_mode: Option<PartyMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircleMetadata {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Knowledge {
    #[serde(rename = "qdrantCollection")]
    pub qdrant_collection: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContext {
    pub path: String,
    #[serde(rename = "autoLoad")]
    pub auto_load: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyMode {
    pub enabled: bool,
    pub orchestrator: Option<String>,
    #[serde(rename = "selectionStrategy")]
    pub selection_strategy: Option<String>,
}

/// Merged circle with both YAML and database information.
#[derive(Debug, Clone)]
pub struct Circle {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub agents: Vec<String>,
    pub channels: Vec<Channel>,
    pub knowledge: Option<Knowledge>,
    pub project_context: Option<ProjectContext>,
    pub party_mode: Option<PartyMode>,
}

/// Errors that can occur in circle registry operations.
#[derive(Debug, Error)]
pub enum CircleRegistryError {
    #[error("database error: {0}")]
    Db(#[from] sera_db::error::DbError),

    #[error("YAML parse error: {0}")]
    YamlParse(#[from] serde_yaml::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("circle not found: {0}")]
    NotFound(String),

    #[error("invalid circle definition: {0}")]
    InvalidDefinition(String),
}

/// CircleRegistry manages circles from both YAML and database sources.
pub struct CircleRegistry {
    pool: Arc<PgPool>,
    circles_dir: PathBuf,
    definitions: std::sync::RwLock<std::collections::HashMap<String, CircleDefinition>>,
}

impl CircleRegistry {
    /// Create a new CircleRegistry and load circles from both DB and YAML.
    pub async fn new(pool: Arc<PgPool>, circles_dir: PathBuf) -> Result<Self, CircleRegistryError> {
        let definitions = Self::load_from_yaml(&circles_dir)?;

        let mut definition_map = std::collections::HashMap::new();
        for def in definitions {
            definition_map.insert(def.metadata.name.clone(), def);
        }

        let registry = Self {
            pool,
            circles_dir,
            definitions: std::sync::RwLock::new(definition_map),
        };

        info!(
            circle_count = registry.definitions.read().unwrap().len(),
            "Circle registry initialized"
        );

        Ok(registry)
    }

    /// Load circle definitions from YAML directory.
    pub fn load_from_yaml(dir: &Path) -> Result<Vec<CircleDefinition>, CircleRegistryError> {
        let mut definitions = Vec::new();

        // Create directory if it doesn't exist
        if !dir.exists() {
            debug!("Circles directory does not exist: {}", dir.display());
            return Ok(definitions);
        }

        // Read all YAML files in the directory
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            // Only process YAML files
            if !matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("yaml" | "yml")
            ) {
                continue;
            }

            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    match serde_yaml::from_str::<CircleDefinition>(&content) {
                        Ok(def) => {
                            // Validate circle definition
                            if def.metadata.name.is_empty() {
                                warn!(
                                    path = %path.display(),
                                    "Circle definition missing name field"
                                );
                                continue;
                            }
                            debug!(
                                name = %def.metadata.name,
                                path = %path.display(),
                                "Loaded circle definition from YAML"
                            );
                            definitions.push(def);
                        }
                        Err(e) => {
                            warn!(
                                path = %path.display(),
                                error = %e,
                                "Failed to parse circle YAML"
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "Failed to read circle YAML file"
                    );
                }
            }
        }

        Ok(definitions)
    }

    /// Get a circle by ID or name.
    pub async fn get_circle(&self, id: &str) -> Result<Circle, CircleRegistryError> {
        // First, try to get from YAML definitions
        if let Some(def) = self.definitions.read().unwrap().get(id) {
            return Ok(self.definition_to_circle(def.clone(), id.to_string()));
        }

        // Try to get from database
        match sera_db::circles::CircleRepository::get_by_name(&self.pool, id).await {
            Ok(row) => {
                // Merge with YAML definition if available
                let def = self.definitions.read().unwrap().get(&row.name).cloned();

                let circle = if let Some(def) = def {
                    self.definition_to_circle(def, row.id.to_string())
                } else {
                    Circle {
                        id: row.id.to_string(),
                        name: row.name.clone(),
                        display_name: row.display_name.clone(),
                        description: row.description.clone(),
                        agents: Vec::new(),
                        channels: Vec::new(),
                        knowledge: None,
                        project_context: None,
                        party_mode: None,
                    }
                };

                Ok(circle)
            }
            Err(sera_db::error::DbError::NotFound { .. }) => {
                Err(CircleRegistryError::NotFound(id.to_string()))
            }
            Err(e) => Err(CircleRegistryError::Db(e)),
        }
    }

    /// List all circles.
    pub async fn list_circles(&self) -> Result<Vec<Circle>, CircleRegistryError> {
        let db_circles = sera_db::circles::CircleRepository::list_circles(&self.pool).await?;
        let definitions = self.definitions.read().unwrap();

        let mut circles = Vec::new();

        for row in db_circles {
            let def = definitions.get(&row.name).cloned();

            let circle = if let Some(def) = def {
                self.definition_to_circle(def, row.id.to_string())
            } else {
                Circle {
                    id: row.id.to_string(),
                    name: row.name.clone(),
                    display_name: row.display_name.clone(),
                    description: row.description.clone(),
                    agents: Vec::new(),
                    channels: Vec::new(),
                    knowledge: None,
                    project_context: None,
                    party_mode: None,
                }
            };

            circles.push(circle);
        }

        Ok(circles)
    }

    /// List all agents in a circle.
    pub async fn list_agents_in_circle(
        &self,
        circle_id: &str,
    ) -> Result<Vec<String>, CircleRegistryError> {
        let circle = self.get_circle(circle_id).await?;
        Ok(circle.agents)
    }

    /// Reload circles from YAML directory.
    pub fn reload_from_yaml(&self) -> Result<(), CircleRegistryError> {
        let definitions = Self::load_from_yaml(&self.circles_dir)?;
        let mut definition_map = std::collections::HashMap::new();

        for def in definitions {
            definition_map.insert(def.metadata.name.clone(), def);
        }

        *self.definitions.write().unwrap() = definition_map;

        info!(
            circle_count = self.definitions.read().unwrap().len(),
            "Circle registry reloaded from YAML"
        );

        Ok(())
    }

    /// Convert a CircleDefinition to a Circle.
    fn definition_to_circle(&self, def: CircleDefinition, id: String) -> Circle {
        Circle {
            id,
            name: def.metadata.name,
            display_name: def.metadata.display_name,
            description: def.metadata.description,
            agents: def.agents,
            channels: def.channels,
            knowledge: def.knowledge,
            project_context: def.project_context,
            party_mode: def.party_mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_yaml(dir: &Path, name: &str, content: &str) {
        let path = dir.join(format!("{}.yaml", name));
        fs::write(path, content).expect("Failed to write test YAML");
    }

    #[test]
    fn test_load_from_yaml_valid() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let yaml_content = r#"
apiVersion: sera/v1
kind: Circle
metadata:
  name: test-circle
  displayName: Test Circle
  description: A test circle
agents:
  - agent1
  - agent2
channels:
  - id: ch1
    name: Channel 1
    description: First channel
knowledge:
  qdrantCollection: test-knowledge
"#;

        create_test_yaml(temp_dir.path(), "test", yaml_content);

        let definitions =
            CircleRegistry::load_from_yaml(temp_dir.path()).expect("Failed to load circles");

        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].metadata.name, "test-circle");
        assert_eq!(definitions[0].agents.len(), 2);
        assert_eq!(definitions[0].channels.len(), 1);
    }

    #[test]
    fn test_load_from_yaml_multiple_files() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        let circle1 = r#"
apiVersion: sera/v1
kind: Circle
metadata:
  name: circle1
  displayName: Circle One
agents:
  - agent1
"#;

        let circle2 = r#"
apiVersion: sera/v1
kind: Circle
metadata:
  name: circle2
  displayName: Circle Two
agents:
  - agent2
"#;

        create_test_yaml(temp_dir.path(), "circle1", circle1);
        create_test_yaml(temp_dir.path(), "circle2", circle2);

        let definitions =
            CircleRegistry::load_from_yaml(temp_dir.path()).expect("Failed to load circles");

        assert_eq!(definitions.len(), 2);
        let names: Vec<_> = definitions
            .iter()
            .map(|d| d.metadata.name.as_str())
            .collect();
        assert!(names.contains(&"circle1"));
        assert!(names.contains(&"circle2"));
    }

    #[test]
    fn test_load_from_yaml_invalid_yaml() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let invalid_yaml = "{ invalid: [unclosed";
        create_test_yaml(temp_dir.path(), "invalid", invalid_yaml);

        let definitions =
            CircleRegistry::load_from_yaml(temp_dir.path()).expect("Failed to load circles");

        // Invalid YAML should be skipped, not error
        assert_eq!(definitions.len(), 0);
    }

    #[test]
    fn test_load_from_yaml_missing_directory() {
        let non_existent = PathBuf::from("/tmp/sera-nonexistent-12345");
        let definitions =
            CircleRegistry::load_from_yaml(&non_existent).expect("Failed to load circles");

        // Non-existent directory should return empty list
        assert_eq!(definitions.len(), 0);
    }

    #[test]
    fn test_load_from_yaml_skips_non_yaml() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        let circle_yaml = r#"
apiVersion: sera/v1
kind: Circle
metadata:
  name: test-circle
  displayName: Test Circle
agents: []
"#;

        create_test_yaml(temp_dir.path(), "circle", circle_yaml);

        // Create a non-YAML file
        fs::write(temp_dir.path().join("readme.md"), "# Test").expect("Failed to write readme");
        fs::write(temp_dir.path().join("config.json"), "{}").expect("Failed to write json");

        let definitions =
            CircleRegistry::load_from_yaml(temp_dir.path()).expect("Failed to load circles");

        // Only the YAML file should be loaded
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].metadata.name, "test-circle");
    }

    #[test]
    fn test_load_from_yaml_missing_name_field() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        let circle_yaml = r#"
apiVersion: sera/v1
kind: Circle
metadata:
  name: ""
  displayName: Test Circle
agents: []
"#;

        create_test_yaml(temp_dir.path(), "circle", circle_yaml);

        let definitions =
            CircleRegistry::load_from_yaml(temp_dir.path()).expect("Failed to load circles");

        // Circle with empty name should be skipped
        assert_eq!(definitions.len(), 0);
    }

    #[test]
    fn test_circle_definition_fields() {
        let def = CircleDefinition {
            api_version: "sera/v1".to_string(),
            kind: "Circle".to_string(),
            metadata: CircleMetadata {
                name: "test".to_string(),
                display_name: "Test Circle".to_string(),
                description: Some("Test description".to_string()),
            },
            agents: vec!["agent1".to_string(), "agent2".to_string()],
            channels: vec![Channel {
                id: "ch1".to_string(),
                name: "Channel 1".to_string(),
                description: Some("Desc".to_string()),
            }],
            knowledge: Some(Knowledge {
                qdrant_collection: Some("test-knowledge".to_string()),
            }),
            project_context: None,
            party_mode: None,
        };

        assert_eq!(def.metadata.name, "test");
        assert_eq!(def.metadata.display_name, "Test Circle");
        assert_eq!(def.agents.len(), 2);
        assert_eq!(def.channels.len(), 1);
        assert!(def.knowledge.is_some());
    }
}
