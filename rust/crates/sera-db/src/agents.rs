//! Agent repository — CRUD for agent_templates and agent_instances.
//!
//! Dual-backend (sera-mwb4):
//! * [`AgentRepository`] — Postgres (sqlx) repository, enterprise path.
//! * [`SqliteAgentStore`] — rusqlite store for local-first boot.
//! * [`AgentStore`] — trait shared by both.

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};
use sqlx::PgPool;
use tokio::sync::Mutex;

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

// ---------------------------------------------------------------------------
// Dual-backend trait (sera-mwb4)
// ---------------------------------------------------------------------------

/// Common agent store surface shared by Postgres and SQLite backends.
#[async_trait]
pub trait AgentStore: Send + Sync + std::fmt::Debug {
    async fn list_instances(
        &self,
        status_filter: Option<&str>,
    ) -> Result<Vec<InstanceRow>, DbError>;

    async fn get_instance(&self, id: &str) -> Result<InstanceRow, DbError>;

    async fn instance_name_exists(&self, name: &str) -> Result<bool, DbError>;

    async fn create_instance(&self, input: CreateInstanceInput<'_>) -> Result<(), DbError>;

    async fn update_instance(
        &self,
        id: &str,
        name: Option<&str>,
        display_name: Option<&str>,
        circle: Option<&str>,
        lifecycle_mode: Option<&str>,
    ) -> Result<(), DbError>;

    async fn update_status(&self, id: &str, status: &str) -> Result<(), DbError>;

    async fn delete_instance(&self, id: &str) -> Result<String, DbError>;
}

#[derive(Debug, Clone)]
pub struct PgAgentStore {
    pool: PgPool,
}

impl PgAgentStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AgentStore for PgAgentStore {
    async fn list_instances(
        &self,
        status_filter: Option<&str>,
    ) -> Result<Vec<InstanceRow>, DbError> {
        AgentRepository::list_instances(&self.pool, status_filter).await
    }

    async fn get_instance(&self, id: &str) -> Result<InstanceRow, DbError> {
        AgentRepository::get_instance(&self.pool, id).await
    }

    async fn instance_name_exists(&self, name: &str) -> Result<bool, DbError> {
        AgentRepository::instance_name_exists(&self.pool, name).await
    }

    async fn create_instance(&self, input: CreateInstanceInput<'_>) -> Result<(), DbError> {
        AgentRepository::create_instance(&self.pool, input).await
    }

    async fn update_instance(
        &self,
        id: &str,
        name: Option<&str>,
        display_name: Option<&str>,
        circle: Option<&str>,
        lifecycle_mode: Option<&str>,
    ) -> Result<(), DbError> {
        AgentRepository::update_instance(&self.pool, id, name, display_name, circle, lifecycle_mode)
            .await
    }

    async fn update_status(&self, id: &str, status: &str) -> Result<(), DbError> {
        AgentRepository::update_status(&self.pool, id, status).await
    }

    async fn delete_instance(&self, id: &str) -> Result<String, DbError> {
        AgentRepository::delete_instance(&self.pool, id).await
    }
}

// ---------------------------------------------------------------------------
// SQLite implementation (sera-mwb4)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SqliteAgentStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteAgentStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_instances (
                id                  TEXT PRIMARY KEY,
                name                TEXT NOT NULL UNIQUE,
                display_name        TEXT,
                template_name       TEXT NOT NULL,
                template_ref        TEXT,
                circle              TEXT,
                status              TEXT NOT NULL DEFAULT 'created',
                lifecycle_mode      TEXT,
                parent_instance_id  TEXT,
                workspace_path      TEXT NOT NULL DEFAULT '',
                container_id        TEXT,
                sandbox_boundary    TEXT,
                overrides           TEXT,
                resolved_config     TEXT,
                resolved_capabilities TEXT,
                last_heartbeat_at   TEXT,
                created_at          TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at          TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_agent_instances_status ON agent_instances(status);

            CREATE TABLE IF NOT EXISTS agent_templates (
                id            TEXT PRIMARY KEY,
                name          TEXT NOT NULL UNIQUE,
                display_name  TEXT,
                builtin       INTEGER NOT NULL DEFAULT 0,
                category      TEXT,
                spec          TEXT NOT NULL DEFAULT '{}',
                created_at    TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
    }
}

fn parse_uuid_opt(s: Option<String>) -> Option<uuid::Uuid> {
    s.and_then(|v| uuid::Uuid::parse_str(&v).ok())
}

fn parse_datetime_opt(s: Option<String>) -> Option<time::OffsetDateTime> {
    s.and_then(|v| {
        if let Ok(dt) = time::OffsetDateTime::parse(&v, &time::format_description::well_known::Rfc3339) {
            return Some(dt);
        }
        let fmt = time::macros::format_description!(
            "[year]-[month]-[day] [hour]:[minute]:[second]"
        );
        time::PrimitiveDateTime::parse(&v, &fmt)
            .ok()
            .map(|p| p.assume_utc())
    })
}

fn row_to_instance(row: &rusqlite::Row<'_>) -> rusqlite::Result<InstanceRow> {
    let id_str: String = row.get("id")?;
    Ok(InstanceRow {
        id: uuid::Uuid::parse_str(&id_str).unwrap_or(uuid::Uuid::nil()),
        name: row.get("name")?,
        display_name: row.get("display_name")?,
        template_name: row.get("template_name")?,
        template_ref: row.get("template_ref")?,
        circle: row.get("circle")?,
        status: row.get("status")?,
        lifecycle_mode: row.get("lifecycle_mode")?,
        parent_instance_id: parse_uuid_opt(row.get("parent_instance_id")?),
        workspace_path: row.get("workspace_path")?,
        container_id: row.get("container_id")?,
        sandbox_boundary: row.get("sandbox_boundary")?,
        overrides: row
            .get::<_, Option<String>>("overrides")?
            .and_then(|s| serde_json::from_str(&s).ok()),
        resolved_config: row
            .get::<_, Option<String>>("resolved_config")?
            .and_then(|s| serde_json::from_str(&s).ok()),
        resolved_capabilities: row
            .get::<_, Option<String>>("resolved_capabilities")?
            .and_then(|s| serde_json::from_str(&s).ok()),
        last_heartbeat_at: parse_datetime_opt(row.get("last_heartbeat_at")?),
        updated_at: parse_datetime_opt(row.get("updated_at")?),
        created_at: parse_datetime_opt(row.get("created_at")?),
    })
}

const SELECT_INSTANCE_COLUMNS: &str = "id, name, display_name, template_name, template_ref, \
    circle, status, lifecycle_mode, parent_instance_id, workspace_path, container_id, \
    sandbox_boundary, overrides, resolved_config, resolved_capabilities, last_heartbeat_at, \
    updated_at, created_at";

#[async_trait]
impl AgentStore for SqliteAgentStore {
    async fn list_instances(
        &self,
        status_filter: Option<&str>,
    ) -> Result<Vec<InstanceRow>, DbError> {
        let conn = self.conn.lock().await;
        let rows = match status_filter {
            Some(status) => {
                let sql = format!(
                    "SELECT {SELECT_INSTANCE_COLUMNS} FROM agent_instances WHERE status = ?1 ORDER BY created_at DESC"
                );
                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| DbError::Integrity(format!("sqlite prepare: {e}")))?;
                let rows = stmt
                    .query_map(params![status], row_to_instance)
                    .map_err(|e| DbError::Integrity(format!("sqlite query: {e}")))?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.map_err(|e| DbError::Integrity(format!("sqlite row: {e}")))?);
                }
                out
            }
            None => {
                let sql = format!(
                    "SELECT {SELECT_INSTANCE_COLUMNS} FROM agent_instances ORDER BY created_at DESC"
                );
                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| DbError::Integrity(format!("sqlite prepare: {e}")))?;
                let rows = stmt
                    .query_map([], row_to_instance)
                    .map_err(|e| DbError::Integrity(format!("sqlite query: {e}")))?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.map_err(|e| DbError::Integrity(format!("sqlite row: {e}")))?);
                }
                out
            }
        };
        Ok(rows)
    }

    async fn get_instance(&self, id: &str) -> Result<InstanceRow, DbError> {
        let conn = self.conn.lock().await;
        let sql = format!(
            "SELECT {SELECT_INSTANCE_COLUMNS} FROM agent_instances WHERE id = ?1"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| DbError::Integrity(format!("sqlite prepare: {e}")))?;
        let row = stmt
            .query_row(params![id], row_to_instance)
            .optional()
            .map_err(|e| DbError::Integrity(format!("sqlite query: {e}")))?;
        row.ok_or(DbError::NotFound {
            entity: "agent_instance",
            key: "id",
            value: id.to_string(),
        })
    }

    async fn instance_name_exists(&self, name: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().await;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_instances WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .map_err(|e| DbError::Integrity(format!("sqlite count: {e}")))?;
        Ok(count > 0)
    }

    async fn create_instance(&self, input: CreateInstanceInput<'_>) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO agent_instances (id, name, template_name, template_ref, workspace_path,
                                           display_name, circle, lifecycle_mode, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'created')",
            params![
                input.id,
                input.name,
                input.template_name,
                input.template_ref,
                input.workspace_path,
                input.display_name,
                input.circle,
                input.lifecycle_mode
            ],
        )
        .map_err(|e| {
            // SQLite raises a UNIQUE constraint error via ErrorCode::ConstraintViolation.
            if let rusqlite::Error::SqliteFailure(f, _) = &e
                && f.code == rusqlite::ErrorCode::ConstraintViolation
            {
                return DbError::Conflict(format!("agent_instance name collision: {e}"));
            }
            DbError::Integrity(format!("sqlite insert agent: {e}"))
        })?;
        Ok(())
    }

    async fn update_instance(
        &self,
        id: &str,
        name: Option<&str>,
        display_name: Option<&str>,
        circle: Option<&str>,
        lifecycle_mode: Option<&str>,
    ) -> Result<(), DbError> {
        let mut sql = String::from("UPDATE agent_instances SET updated_at = datetime('now')");
        let mut args: Vec<String> = Vec::new();
        if let Some(v) = name {
            sql.push_str(", name = ?");
            args.push(v.to_string());
        }
        if let Some(v) = display_name {
            sql.push_str(", display_name = ?");
            args.push(v.to_string());
        }
        if let Some(v) = circle {
            sql.push_str(", circle = ?");
            args.push(v.to_string());
        }
        if let Some(v) = lifecycle_mode {
            sql.push_str(", lifecycle_mode = ?");
            args.push(v.to_string());
        }
        sql.push_str(" WHERE id = ?");
        args.push(id.to_string());

        let conn = self.conn.lock().await;
        let param_refs: Vec<&dyn rusqlite::ToSql> =
            args.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        let n = conn
            .execute(&sql, param_refs.as_slice())
            .map_err(|e| DbError::Integrity(format!("sqlite update agent: {e}")))?;
        if n == 0 {
            return Err(DbError::NotFound {
                entity: "agent_instance",
                key: "id",
                value: id.to_string(),
            });
        }
        Ok(())
    }

    async fn update_status(&self, id: &str, status: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        let n = conn
            .execute(
                "UPDATE agent_instances SET status = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![status, id],
            )
            .map_err(|e| DbError::Integrity(format!("sqlite update status: {e}")))?;
        if n == 0 {
            return Err(DbError::NotFound {
                entity: "agent_instance",
                key: "id",
                value: id.to_string(),
            });
        }
        Ok(())
    }

    async fn delete_instance(&self, id: &str) -> Result<String, DbError> {
        let conn = self.conn.lock().await;
        let name: Option<String> = conn
            .query_row(
                "SELECT name FROM agent_instances WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| DbError::Integrity(format!("sqlite delete get: {e}")))?;
        match name {
            Some(name) => {
                conn.execute("DELETE FROM agent_instances WHERE id = ?1", params![id])
                    .map_err(|e| DbError::Integrity(format!("sqlite delete agent: {e}")))?;
                Ok(name)
            }
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

    // --- SQLite backend tests (sera-mwb4) ---------------------------------

    fn new_store() -> SqliteAgentStore {
        let conn = Connection::open_in_memory().unwrap();
        SqliteAgentStore::init_schema(&conn).unwrap();
        SqliteAgentStore::new(Arc::new(Mutex::new(conn)))
    }

    #[tokio::test]
    async fn sqlite_create_get_roundtrip() {
        let store = new_store();
        let id = uuid::Uuid::new_v4().to_string();
        store
            .create_instance(CreateInstanceInput {
                id: &id,
                name: "alice",
                template_name: "base",
                template_ref: "base@v1",
                workspace_path: "/ws/alice",
                display_name: Some("Alice"),
                circle: Some("default"),
                lifecycle_mode: None,
            })
            .await
            .unwrap();
        let row = store.get_instance(&id).await.unwrap();
        assert_eq!(row.name, "alice");
        assert_eq!(row.template_ref.as_deref(), Some("base@v1"));
        assert_eq!(row.status.as_deref(), Some("created"));
    }

    #[tokio::test]
    async fn sqlite_list_instances_filtered_by_status() {
        let store = new_store();
        for (i, status) in ["running", "created", "running"].iter().enumerate() {
            let id = uuid::Uuid::new_v4().to_string();
            store
                .create_instance(CreateInstanceInput {
                    id: &id,
                    name: &format!("agent-{i}"),
                    template_name: "base",
                    template_ref: "base@v1",
                    workspace_path: "/ws",
                    display_name: None,
                    circle: None,
                    lifecycle_mode: None,
                })
                .await
                .unwrap();
            store.update_status(&id, status).await.unwrap();
        }
        let running = store.list_instances(Some("running")).await.unwrap();
        assert_eq!(running.len(), 2);
        let all = store.list_instances(None).await.unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn sqlite_unique_name_conflict() {
        let store = new_store();
        let id1 = uuid::Uuid::new_v4().to_string();
        let id2 = uuid::Uuid::new_v4().to_string();
        store
            .create_instance(CreateInstanceInput {
                id: &id1,
                name: "same",
                template_name: "base",
                template_ref: "base@v1",
                workspace_path: "/ws",
                display_name: None,
                circle: None,
                lifecycle_mode: None,
            })
            .await
            .unwrap();
        let err = store
            .create_instance(CreateInstanceInput {
                id: &id2,
                name: "same",
                template_name: "base",
                template_ref: "base@v1",
                workspace_path: "/ws",
                display_name: None,
                circle: None,
                lifecycle_mode: None,
            })
            .await
            .unwrap_err();
        matches!(err, DbError::Conflict(_));
        assert!(store.instance_name_exists("same").await.unwrap());
    }

    #[tokio::test]
    async fn sqlite_delete_returns_name() {
        let store = new_store();
        let id = uuid::Uuid::new_v4().to_string();
        store
            .create_instance(CreateInstanceInput {
                id: &id,
                name: "bob",
                template_name: "base",
                template_ref: "base@v1",
                workspace_path: "/ws",
                display_name: None,
                circle: None,
                lifecycle_mode: None,
            })
            .await
            .unwrap();
        let name = store.delete_instance(&id).await.unwrap();
        assert_eq!(name, "bob");
        assert!(store.get_instance(&id).await.is_err());
    }

    #[tokio::test]
    async fn sqlite_update_fields_and_tenant_isolation() {
        let store = new_store();
        let id_a = uuid::Uuid::new_v4().to_string();
        let id_b = uuid::Uuid::new_v4().to_string();
        for (id, circle) in [(&id_a, "tenant-a"), (&id_b, "tenant-b")] {
            store
                .create_instance(CreateInstanceInput {
                    id,
                    name: &format!("agent-{circle}"),
                    template_name: "base",
                    template_ref: "base@v1",
                    workspace_path: "/ws",
                    display_name: None,
                    circle: Some(circle),
                    lifecycle_mode: None,
                })
                .await
                .unwrap();
        }
        store
            .update_instance(&id_a, None, Some("Renamed"), None, None)
            .await
            .unwrap();
        let a = store.get_instance(&id_a).await.unwrap();
        let b = store.get_instance(&id_b).await.unwrap();
        assert_eq!(a.display_name.as_deref(), Some("Renamed"));
        assert!(b.display_name.is_none());
        // Tenant isolation check: each has its own circle.
        assert_eq!(a.circle.as_deref(), Some("tenant-a"));
        assert_eq!(b.circle.as_deref(), Some("tenant-b"));
    }
}
