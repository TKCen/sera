//! Agent repository — CRUD for agent_templates and agent_instances.

use sqlx::PgPool;

use sera_types::agent::{AgentInstance, AgentStatus};
use crate::error::DbError;

/// Row type for agent_templates table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TemplateRow {
    pub id: uuid::Uuid,
    pub name: String,
    pub display_name: Option<String>,
    pub builtin: bool,
    pub category: Option<String>,
    pub spec: serde_json::Value,
    pub created_at: Option<time::OffsetDateTime>,
    pub updated_at: Option<time::OffsetDateTime>,
}

/// Row type for agent_instances table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InstanceRow {
    pub id: uuid::Uuid,
    pub name: String,
    pub display_name: Option<String>,
    pub template_name: String,
    pub template_ref: Option<String>,
    pub circle: Option<String>,
    pub status: Option<String>,
    pub lifecycle_mode: Option<String>,
    pub parent_instance_id: Option<uuid::Uuid>,
    pub workspace_path: String,
    pub container_id: Option<String>,
    pub sandbox_boundary: Option<String>,
    pub overrides: Option<serde_json::Value>,
    pub resolved_config: Option<serde_json::Value>,
    pub resolved_capabilities: Option<serde_json::Value>,
    pub last_heartbeat_at: Option<time::OffsetDateTime>,
    pub updated_at: Option<time::OffsetDateTime>,
    pub created_at: Option<time::OffsetDateTime>,
}

impl InstanceRow {
    /// Convert database row to domain type.
    pub fn into_domain(self) -> AgentInstance {
        let status_str = self.status.as_deref().unwrap_or("active");
        let status = match status_str {
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
            id: self.id.to_string(),
            name: self.name,
            display_name: self.display_name,
            template_ref: self.template_ref.unwrap_or(self.template_name.clone()),
            circle: self.circle,
            status,
            overrides: self.overrides,
            lifecycle_mode: self.lifecycle_mode.and_then(|m| serde_json::from_str(&format!("\"{m}\"")).ok()),
            parent_instance_id: self.parent_instance_id.map(|id| id.to_string()),
            resolved_config: self.resolved_config,
            resolved_capabilities: self.resolved_capabilities,
            workspace_path: Some(self.workspace_path),
            workspace_used_gb: None,
            container_id: self.container_id,
            circle_id: None,
            last_heartbeat_at: self.last_heartbeat_at.map(|t| t.to_string()),
            updated_at: self.updated_at.map(|t| t.to_string()).unwrap_or_default(),
            created_at: self.created_at.map(|t| t.to_string()).unwrap_or_default(),
        }
    }
}

/// Input for creating a new agent instance.
pub struct CreateInstanceInput<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub template_name: &'a str,
    pub template_ref: &'a str,
    pub workspace_path: &'a str,
    pub display_name: Option<&'a str>,
    pub circle: Option<&'a str>,
    pub lifecycle_mode: Option<&'a str>,
}

/// Agent repository for database operations.
pub struct AgentRepository;

impl AgentRepository {
    /// List all agent templates.
    pub async fn list_templates(pool: &PgPool) -> Result<Vec<TemplateRow>, DbError> {
        let rows = sqlx::query_as::<_, TemplateRow>(
            "SELECT id, name, display_name, builtin, category, spec, created_at, updated_at
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
            "SELECT id, name, display_name, builtin, category, spec, created_at, updated_at
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
                "SELECT id, name, display_name, template_name, template_ref, circle, status,
                        lifecycle_mode, parent_instance_id, workspace_path, container_id,
                        sandbox_boundary, overrides, resolved_config, resolved_capabilities,
                        last_heartbeat_at, updated_at, created_at
                 FROM agent_instances WHERE status = $1
                 ORDER BY created_at DESC"
            )
            .bind(status)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, InstanceRow>(
                "SELECT id, name, display_name, template_name, template_ref, circle, status,
                        lifecycle_mode, parent_instance_id, workspace_path, container_id,
                        sandbox_boundary, overrides, resolved_config, resolved_capabilities,
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
            "SELECT id, name, display_name, template_name, template_ref, circle, status,
                    lifecycle_mode, parent_instance_id, workspace_path, container_id,
                    sandbox_boundary, overrides, resolved_config, resolved_capabilities,
                    last_heartbeat_at, updated_at, created_at
             FROM agent_instances WHERE id::text = $1"
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

    /// Update instance status.
    pub async fn update_status(
        pool: &PgPool,
        id: &str,
        status: &str,
    ) -> Result<(), DbError> {
        let result = sqlx::query(
            "UPDATE agent_instances SET status = $1, updated_at = NOW() WHERE id::text = $2"
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

    /// Check if an instance name already exists.
    pub async fn instance_name_exists(pool: &PgPool, name: &str) -> Result<bool, DbError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_instances WHERE name = $1"
        )
        .bind(name)
        .fetch_one(pool)
        .await?;
        Ok(row.0 > 0)
    }

    /// Create a new agent instance. Returns the new instance ID.
    pub async fn create_instance(
        pool: &PgPool,
        input: CreateInstanceInput<'_>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO agent_instances (id, name, template_name, template_ref, workspace_path,
                                          display_name, circle, lifecycle_mode, status, created_at, updated_at)
             VALUES ($1::uuid, $2, $3, $4, $5, $6, $7, $8, 'created', NOW(), NOW())"
        )
        .bind(input.id)
        .bind(input.name)
        .bind(input.template_name)
        .bind(input.template_ref)
        .bind(input.workspace_path)
        .bind(input.display_name)
        .bind(input.circle)
        .bind(input.lifecycle_mode)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update an agent instance's mutable fields.
    pub async fn update_instance(
        pool: &PgPool,
        id: &str,
        name: Option<&str>,
        display_name: Option<&str>,
        circle: Option<&str>,
        lifecycle_mode: Option<&str>,
    ) -> Result<(), DbError> {
        let mut qb = sqlx::QueryBuilder::new("UPDATE agent_instances SET updated_at = NOW()");

        if let Some(v) = name {
            qb.push(", name = ").push_bind(v);
        }
        if let Some(v) = display_name {
            qb.push(", display_name = ").push_bind(v);
        }
        if let Some(v) = circle {
            qb.push(", circle = ").push_bind(v);
        }
        if let Some(v) = lifecycle_mode {
            qb.push(", lifecycle_mode = ").push_bind(v);
        }

        qb.push(" WHERE id::text = ").push_bind(id);

        let result = qb.build().execute(pool).await?;
        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity: "agent_instance",
                key: "id",
                value: id.to_string(),
            });
        }
        Ok(())
    }

    /// Delete an agent instance. Returns the name of the deleted instance.
    pub async fn delete_instance(pool: &PgPool, id: &str) -> Result<String, DbError> {
        let row: Option<(String,)> = sqlx::query_as(
            "DELETE FROM agent_instances WHERE id::text = $1 RETURNING name"
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some((name,)) => Ok(name),
            None => Err(DbError::NotFound {
                entity: "agent_instance",
                key: "id",
                value: id.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_instance_row(status: Option<&str>) -> InstanceRow {
        InstanceRow {
            id: uuid::Uuid::nil(),
            name: "test-agent".to_string(),
            display_name: Some("Test Agent".to_string()),
            template_name: "base".to_string(),
            template_ref: Some("base@v1".to_string()),
            circle: Some("default".to_string()),
            status: status.map(str::to_string),
            lifecycle_mode: None,
            parent_instance_id: None,
            workspace_path: "/workspace/test".to_string(),
            container_id: Some("cnt-abc".to_string()),
            sandbox_boundary: None,
            overrides: None,
            resolved_config: None,
            resolved_capabilities: None,
            last_heartbeat_at: None,
            updated_at: None,
            created_at: None,
        }
    }

    #[test]
    fn into_domain_maps_name_and_template_ref() {
        let row = make_instance_row(Some("active"));
        let domain = row.into_domain();
        assert_eq!(domain.name, "test-agent");
        assert_eq!(domain.template_ref, "base@v1");
        assert_eq!(domain.workspace_path, Some("/workspace/test".to_string()));
        assert_eq!(domain.container_id, Some("cnt-abc".to_string()));
    }

    #[test]
    fn into_domain_falls_back_to_template_name_when_ref_absent() {
        let mut row = make_instance_row(Some("active"));
        row.template_ref = None;
        let domain = row.into_domain();
        assert_eq!(domain.template_ref, "base");
    }

    #[test]
    fn into_domain_status_all_variants() {
        let cases: &[(&str, AgentStatus)] = &[
            ("created", AgentStatus::Created),
            ("running", AgentStatus::Running),
            ("stopped", AgentStatus::Stopped),
            ("error", AgentStatus::Error),
            ("unresponsive", AgentStatus::Unresponsive),
            ("throttled", AgentStatus::Throttled),
            ("active", AgentStatus::Active),
            ("inactive", AgentStatus::Inactive),
        ];
        for (s, expected) in cases {
            let row = make_instance_row(Some(s));
            let domain = row.into_domain();
            assert_eq!(
                std::mem::discriminant(&domain.status),
                std::mem::discriminant(expected),
                "status string '{}' should map correctly",
                s
            );
        }
    }

    #[test]
    fn into_domain_unknown_status_defaults_to_created() {
        let row = make_instance_row(Some("bogus_status"));
        let domain = row.into_domain();
        assert_eq!(
            std::mem::discriminant(&domain.status),
            std::mem::discriminant(&AgentStatus::Created)
        );
    }

    #[test]
    fn into_domain_none_status_defaults_to_active() {
        let row = make_instance_row(None);
        let domain = row.into_domain();
        assert_eq!(
            std::mem::discriminant(&domain.status),
            std::mem::discriminant(&AgentStatus::Active)
        );
    }

    #[test]
    fn into_domain_id_is_stringified_uuid() {
        let id = uuid::Uuid::new_v4();
        let mut row = make_instance_row(Some("running"));
        row.id = id;
        let domain = row.into_domain();
        assert_eq!(domain.id, id.to_string());
    }

    #[test]
    fn into_domain_circle_propagated() {
        let row = make_instance_row(Some("active"));
        let domain = row.into_domain();
        assert_eq!(domain.circle, Some("default".to_string()));
    }

    #[test]
    fn into_domain_workspace_used_gb_is_none() {
        let row = make_instance_row(Some("active"));
        let domain = row.into_domain();
        assert!(domain.workspace_used_gb.is_none());
    }

    #[test]
    fn into_domain_circle_id_is_none() {
        let row = make_instance_row(Some("active"));
        let domain = row.into_domain();
        assert!(domain.circle_id.is_none());
    }
}
