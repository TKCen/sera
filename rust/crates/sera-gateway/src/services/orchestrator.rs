//! Orchestrator service — agent lifecycle management.
//!
//! Manages agent creation, startup, shutdown, and lifecycle transitions.
//! Coordinates between the database, Docker container manager, and manifest validation.

use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

use std::sync::Arc;
use sera_config::DataRoot;
use sera_db::{agents::AgentRepository, DbError};
use sera_tools::sandbox::{SandboxConfig, SandboxHandle, SandboxProvider};

/// High-level agent lifecycle orchestration.
///
/// Manages agent manifest validation, database persistence, and container lifecycle.
pub struct Orchestrator {
    pool: PgPool,
    sandbox: Arc<dyn SandboxProvider>,
    data_root: DataRoot,
}

impl Orchestrator {
    /// Create a new Orchestrator.
    ///
    /// # Arguments
    /// * `pool` — PostgreSQL connection pool
    /// * `sandbox` — Sandbox provider (Docker, WASM, etc.)
    ///
    /// The host-side data root is resolved from `SERA_DATA_ROOT` or the
    /// platform default via [`DataRoot::from_env`]. Use [`Self::with_data_root`]
    /// to override in tests or embedded deployments.
    pub fn new(pool: PgPool, sandbox: Arc<dyn SandboxProvider>) -> Self {
        Self {
            pool,
            sandbox,
            data_root: DataRoot::from_env(),
        }
    }

    /// Override the data root (builder-style). Used by tests and embedded
    /// deployments that want to pin workspace paths.
    pub fn with_data_root(mut self, data_root: DataRoot) -> Self {
        self.data_root = data_root;
        self
    }

    /// Validate an agent manifest JSON.
    ///
    /// Checks for required fields: `name`, `template_name`, `image`.
    fn validate_manifest(manifest: &serde_json::Value) -> Result<(), OrchestratorError> {
        if !manifest.is_object() {
            return Err(OrchestratorError::ManifestValidation(
                "Manifest must be a JSON object".to_string(),
            ));
        }

        let obj = manifest
            .as_object()
            .ok_or_else(|| OrchestratorError::ManifestValidation("Invalid manifest".to_string()))?;

        if !obj.contains_key("name") {
            return Err(OrchestratorError::ManifestValidation(
                "Missing required field: name".to_string(),
            ));
        }

        if !obj.contains_key("template_name") {
            return Err(OrchestratorError::ManifestValidation(
                "Missing required field: template_name".to_string(),
            ));
        }

        if !obj.contains_key("image") {
            return Err(OrchestratorError::ManifestValidation(
                "Missing required field: image".to_string(),
            ));
        }

        Ok(())
    }

    /// Create a new agent from a manifest.
    ///
    /// Validates the manifest, generates a new instance ID, and inserts into the database.
    /// Does not start the container — use `start_agent()` for that.
    ///
    /// # Arguments
    /// * `manifest` — JSON manifest with required fields: name, template_name, image
    ///
    /// # Returns
    /// The instance ID (UUID) of the newly created agent
    pub async fn create_agent(
        &self,
        manifest: serde_json::Value,
    ) -> Result<String, OrchestratorError> {
        Self::validate_manifest(&manifest)?;

        let obj = manifest.as_object().ok_or_else(|| {
            OrchestratorError::ManifestValidation("Invalid manifest".to_string())
        })?;

        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                OrchestratorError::ManifestValidation("'name' must be a string".to_string())
            })?;

        let template_name = obj
            .get("template_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                OrchestratorError::ManifestValidation(
                    "'template_name' must be a string".to_string(),
                )
            })?;

        // Check if instance name already exists
        if AgentRepository::instance_name_exists(&self.pool, name)
            .await
            .map_err(OrchestratorError::Db)?
        {
            return Err(OrchestratorError::ManifestValidation(format!(
                "Agent instance with name '{}' already exists",
                name
            )));
        }

        let instance_id = Uuid::new_v4().to_string();
        let workspace_path = self
            .data_root
            .agent_workspace(&instance_id)
            .to_string_lossy()
            .into_owned();
        let template_ref = obj
            .get("template_ref")
            .and_then(|v| v.as_str())
            .unwrap_or(template_name);
        let display_name = obj.get("display_name").and_then(|v| v.as_str());
        let circle = obj.get("circle").and_then(|v| v.as_str());
        let lifecycle_mode = obj.get("lifecycle_mode").and_then(|v| v.as_str());

        AgentRepository::create_instance(
            &self.pool,
            sera_db::agents::CreateInstanceInput {
                id: &instance_id,
                name,
                template_name,
                template_ref,
                workspace_path: &workspace_path,
                display_name,
                circle,
                lifecycle_mode,
            },
        )
        .await
        .map_err(OrchestratorError::Db)?;

        tracing::info!(instance_id, name, "Created agent instance");

        Ok(instance_id)
    }

    /// Start an agent container.
    ///
    /// Looks up the agent by ID, extracts Docker configuration from the manifest,
    /// creates and starts a container, then updates the agent status to "running".
    ///
    /// # Arguments
    /// * `agent_id` — The agent instance ID (UUID)
    ///
    /// # Returns
    /// The container ID of the started container
    pub async fn start_agent(&self, agent_id: &str) -> Result<String, OrchestratorError> {
        let agent = AgentRepository::get_instance(&self.pool, agent_id)
            .await
            .map_err(OrchestratorError::Db)?;

        // Extract image from manifest — fall back to template name if not specified
        let image = format!("sera-agent-{}", agent.template_name.to_lowercase());

        let mut env_vars = HashMap::new();
        env_vars.insert("SERA_INSTANCE_ID".to_string(), agent_id.to_string());
        env_vars.insert(
            "SERA_AGENT_NAME".to_string(),
            agent.name.clone().to_string(),
        );
        if let Some(circle) = &agent.circle {
            env_vars.insert("SERA_CIRCLE".to_string(), circle.clone());
        }

        let config = SandboxConfig {
            image: Some(image),
            env: env_vars,
            labels: HashMap::from([
                ("sera.instance".to_string(), agent_id.to_string()),
                ("sera.agent".to_string(), agent.name.clone()),
                ("sera.template".to_string(), agent.template_name.clone()),
            ]),
            ..Default::default()
        };

        let handle = self
            .sandbox
            .create(&config)
            .await
            .map_err(OrchestratorError::Sandbox)?;

        let container_id = handle.0;

        // Update agent status to running and store container ID
        AgentRepository::update_status(&self.pool, agent_id, "running")
            .await
            .map_err(OrchestratorError::Db)?;

        tracing::info!(
            agent_id,
            container_id,
            "Started agent container"
        );

        Ok(container_id)
    }

    /// Stop an agent container and update status.
    ///
    /// Looks up the agent, stops its container via the container manager,
    /// and updates the agent status to "stopped".
    ///
    /// # Arguments
    /// * `agent_id` — The agent instance ID (UUID)
    pub async fn stop_agent(&self, agent_id: &str) -> Result<(), OrchestratorError> {
        let agent = AgentRepository::get_instance(&self.pool, agent_id)
            .await
            .map_err(OrchestratorError::Db)?;

        if let Some(container_id) = &agent.container_id {
            self.sandbox
                .destroy(&SandboxHandle(container_id.clone()))
                .await
                .map_err(OrchestratorError::Sandbox)?;
        } else {
            tracing::warn!(agent_id, "Agent has no container_id to stop");
        }

        AgentRepository::update_status(&self.pool, agent_id, "stopped")
            .await
            .map_err(OrchestratorError::Db)?;

        tracing::info!(agent_id, "Stopped agent");

        Ok(())
    }

    /// Retrieve a single agent by ID.
    ///
    /// # Arguments
    /// * `agent_id` — The agent instance ID (UUID)
    ///
    /// # Returns
    /// The agent instance row
    pub async fn get_agent(
        &self,
        agent_id: &str,
    ) -> Result<sera_db::agents::InstanceRow, OrchestratorError> {
        AgentRepository::get_instance(&self.pool, agent_id)
            .await
            .map_err(OrchestratorError::Db)
    }

    /// List agents with pagination.
    ///
    /// # Arguments
    /// * `limit` — maximum number of agents to return
    /// * `offset` — number of agents to skip
    ///
    /// # Returns
    /// A vector of agent instance rows
    pub async fn list_agents(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<sera_db::agents::InstanceRow>, OrchestratorError> {
        // For now, fetch all agents and apply limit/offset in memory
        // (The repository doesn't have paginated query yet, so we'll implement at the service layer)
        let all_agents = AgentRepository::list_instances(&self.pool, None)
            .await
            .map_err(OrchestratorError::Db)?;

        let offset_usize = offset as usize;
        let limit_usize = limit as usize;

        let paginated = all_agents
            .into_iter()
            .skip(offset_usize)
            .take(limit_usize)
            .collect();

        Ok(paginated)
    }
}

/// Error type for orchestrator operations.
#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    /// Database error (from sera-db).
    #[error("database error: {0}")]
    Db(#[from] DbError),

    /// Sandbox/container management error.
    #[error("sandbox error: {0}")]
    Sandbox(#[from] sera_tools::sandbox::SandboxError),

    /// Manifest validation error.
    #[error("invalid manifest: {0}")]
    ManifestValidation(String),

    /// Agent not found.
    #[error("agent not found: {0}")]
    NotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_manifest_missing_name() {
        let manifest = serde_json::json!({
            "template_name": "example",
            "image": "sera-agent-example"
        });

        let result = Orchestrator::validate_manifest(&manifest);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Missing required field: name"));
    }

    #[test]
    fn test_validate_manifest_missing_template_name() {
        let manifest = serde_json::json!({
            "name": "my-agent",
            "image": "sera-agent-example"
        });

        let result = Orchestrator::validate_manifest(&manifest);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Missing required field: template_name"));
    }

    #[test]
    fn test_validate_manifest_missing_image() {
        let manifest = serde_json::json!({
            "name": "my-agent",
            "template_name": "example"
        });

        let result = Orchestrator::validate_manifest(&manifest);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Missing required field: image"));
    }

    #[test]
    fn test_validate_manifest_valid() {
        let manifest = serde_json::json!({
            "name": "my-agent",
            "template_name": "example",
            "image": "sera-agent-example"
        });

        let result = Orchestrator::validate_manifest(&manifest);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_manifest_with_optional_fields() {
        let manifest = serde_json::json!({
            "name": "my-agent",
            "template_name": "example",
            "image": "sera-agent-example",
            "display_name": "My Agent",
            "circle": "my-circle",
            "lifecycle_mode": "persistent"
        });

        let result = Orchestrator::validate_manifest(&manifest);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_manifest_not_object() {
        let manifest = serde_json::json!("not an object");

        let result = Orchestrator::validate_manifest(&manifest);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must be a JSON object"));
    }
}
