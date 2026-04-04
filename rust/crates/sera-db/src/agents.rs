//! Agent repository — CRUD for agent_templates and agent_instances.

use sqlx::PgPool;

use sera_domain::agent::{AgentInstance, AgentStatus};
use crate::error::DbError;

/// Row type for agent_templates table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TemplateRow {
    pub name: String,
    pub display_name: Option<String>,
    pub builtin: bool,
    pub category: Option<String>,
    pub spec: serde_json::Value,
    pub description: Option<String>,
    pub updated_at: Option<time::OffsetDateTime>,
}

/// Row type for agent_instances table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InstanceRow {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub template_ref: String,
    pub circle: Option<String>,
    pub status: String,
    pub lifecycle_mode: Option<String>,
    pub parent_instance_id: Option<String>,
    pub workspace_path: Option<String>,
    pub container_id: Option<String>,
    pub last_heartbeat_at: Option<time::OffsetDateTime>,
    pub updated_at: Option<time::OffsetDateTime>,
    pub created_at: Option<time::OffsetDateTime>,
}

impl InstanceRow {
    /// Convert database row to domain type.
    pub fn into_domain(self) -> AgentInstance {
        let status = match self.status.as_str() {
            "created" => AgentStatus::Created,
            "running" => AgentStatus::Running,
            "stopped" => AgentStatus::Stopped,
            "error" => AgentStatus::Error,
            "unresponsive" => AgentStatus::Unresponsive,
            "throttled" => AgentStatus::Throttled,
            "active" => AgentStatus::Active,
            "inactive" => AgentStatus::Inactive,
            _ => AgentStatus::Created,
        };

        AgentInstance {
            id: self.id,
            name: self.name,
            display_name: self.display_name,
            template_ref: self.template_ref,
            circle: self.circle,
            status,
            overrides: None,
            lifecycle_mode: self.lifecycle_mode.and_then(|m| serde_json::from_str(&format!("\"{m}\"")).ok()),
            parent_instance_id: self.parent_instance_id,
            resolved_config: None,
            resolved_capabilities: None,
            workspace_path: self.workspace_path,
            workspace_used_gb: None,
            container_id: self.container_id,
            circle_id: None,
            last_heartbeat_at: self.last_heartbeat_at.map(|t| t.to_string()),
            updated_at: self.updated_at.map(|t| t.to_string()).unwrap_or_default(),
            created_at: self.created_at.map(|t| t.to_string()).unwrap_or_default(),
        }
    }
}

/// Agent repository for database operations.
pub struct AgentRepository;

impl AgentRepository {
    /// List all agent templates.
    pub async fn list_templates(pool: &PgPool) -> Result<Vec<TemplateRow>, DbError> {
        let rows = sqlx::query_as::<_, TemplateRow>(
            "SELECT name, display_name, builtin, category, spec, description, updated_at
             FROM agent_templates
             ORDER BY name"
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Get a single template by name.
    pub async fn get_template(pool: &PgPool, name: &str) -> Result<TemplateRow, DbError> {
        sqlx::query_as::<_, TemplateRow>(
            "SELECT name, display_name, builtin, category, spec, description, updated_at
             FROM agent_templates WHERE name = $1"
        )
        .bind(name)
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound {
            entity: "agent_template",
            key: "name",
            value: name.to_string(),
        })
    }

    /// List all agent instances, optionally filtered by status.
    pub async fn list_instances(
        pool: &PgPool,
        status_filter: Option<&str>,
    ) -> Result<Vec<InstanceRow>, DbError> {
        let rows = if let Some(status) = status_filter {
            sqlx::query_as::<_, InstanceRow>(
                "SELECT id, name, display_name, template_ref, circle, status, lifecycle_mode,
                        parent_instance_id, workspace_path, container_id,
                        last_heartbeat_at, updated_at, created_at
                 FROM agent_instances WHERE status = $1
                 ORDER BY created_at DESC"
            )
            .bind(status)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, InstanceRow>(
                "SELECT id, name, display_name, template_ref, circle, status, lifecycle_mode,
                        parent_instance_id, workspace_path, container_id,
                        last_heartbeat_at, updated_at, created_at
                 FROM agent_instances
                 ORDER BY created_at DESC"
            )
            .fetch_all(pool)
            .await?
        };
        Ok(rows)
    }

    /// Get a single instance by ID.
    pub async fn get_instance(pool: &PgPool, id: &str) -> Result<InstanceRow, DbError> {
        sqlx::query_as::<_, InstanceRow>(
            "SELECT id, name, display_name, template_ref, circle, status, lifecycle_mode,
                    parent_instance_id, workspace_path, container_id,
                    last_heartbeat_at, updated_at, created_at
             FROM agent_instances WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound {
            entity: "agent_instance",
            key: "id",
            value: id.to_string(),
        })
    }

    /// Create a new agent instance.
    pub async fn create_instance(
        pool: &PgPool,
        id: &str,
        name: &str,
        template_ref: &str,
        lifecycle_mode: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO agent_instances (id, name, template_ref, status, lifecycle_mode, created_at, updated_at)
             VALUES ($1, $2, $3, 'created', $4, NOW(), NOW())"
        )
        .bind(id)
        .bind(name)
        .bind(template_ref)
        .bind(lifecycle_mode)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update instance status.
    pub async fn update_status(
        pool: &PgPool,
        id: &str,
        status: &str,
    ) -> Result<(), DbError> {
        let result = sqlx::query(
            "UPDATE agent_instances SET status = $1, updated_at = NOW() WHERE id = $2"
        )
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity: "agent_instance",
                key: "id",
                value: id.to_string(),
            });
        }
        Ok(())
    }
}
