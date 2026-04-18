//! Schedules repository — cron and one-shot task scheduling.
//!
//! Dual-backend (sera-mwb4):
//! * [`ScheduleRepository`] — Postgres (sqlx) with agent-join via
//!   `agent_instances` (enterprise path).
//! * [`SqliteScheduleStore`] — rusqlite equivalent for local-first boot.
//! * [`ScheduleStore`] — shared trait.

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::error::DbError;

/// Row type for schedules table (with agent name resolved via JOIN).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ScheduleRow {
    pub id: uuid::Uuid,
    pub agent_id: Option<uuid::Uuid>,
    pub agent_instance_id: Option<uuid::Uuid>,
    pub agent_name: Option<String>,
    pub name: String,
    pub cron: Option<String>,
    pub expression: Option<String>,
    pub r#type: Option<String>,
    pub task: serde_json::Value,
    pub source: String,
    pub status: Option<String>,
    pub last_run_at: Option<time::OffsetDateTime>,
    pub last_run_status: Option<String>,
    pub next_run_at: Option<time::OffsetDateTime>,
    pub category: Option<String>,
    pub description: Option<String>,
    pub created_at: Option<time::OffsetDateTime>,
    pub updated_at: Option<time::OffsetDateTime>,
}

pub struct ScheduleRepository;

impl ScheduleRepository {
    /// List all schedules with agent name resolved via JOIN.
    pub async fn list_schedules(pool: &PgPool) -> Result<Vec<ScheduleRow>, DbError> {
        let rows = sqlx::query_as::<_, ScheduleRow>(
            "SELECT s.id, s.agent_id, s.agent_instance_id,
                    COALESCE(s.agent_name, ai.name) as agent_name,
                    s.name, s.cron, s.expression, s.type, s.task, s.source, s.status,
                    s.last_run_at, s.last_run_status, s.next_run_at,
                    s.category, s.description, s.created_at, s.updated_at
             FROM schedules s
             LEFT JOIN agent_instances ai ON ai.id = s.agent_instance_id
             ORDER BY s.created_at DESC"
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Create a new schedule.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_schedule(
        pool: &PgPool,
        id: &str,
        agent_instance_id: Option<&str>,
        agent_name: &str,
        name: &str,
        schedule_type: &str,
        expression: &str,
        task: &serde_json::Value,
        source: &str,
        status: &str,
        category: Option<&str>,
        description: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO schedules (id, agent_instance_id, agent_name, name, type, expression, task,
                                    source, status, category, description, created_at, updated_at)
             VALUES ($1::uuid, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10, $11, NOW(), NOW())"
        )
        .bind(id)
        .bind(agent_instance_id)
        .bind(agent_name)
        .bind(name)
        .bind(schedule_type)
        .bind(expression)
        .bind(task)
        .bind(source)
        .bind(status)
        .bind(category)
        .bind(description)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update a schedule's mutable fields. Returns number of rows affected.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_schedule(
        pool: &PgPool,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
        expression: Option<&str>,
        task: Option<&serde_json::Value>,
        status: Option<&str>,
        category: Option<&str>,
    ) -> Result<u64, DbError> {
        let mut qb = sqlx::QueryBuilder::new("UPDATE schedules SET updated_at = NOW()");

        if let Some(v) = name {
            qb.push(", name = ").push_bind(v);
        }
        if let Some(v) = description {
            qb.push(", description = ").push_bind(v);
        }
        if let Some(v) = expression {
            qb.push(", expression = ").push_bind(v);
        }
        if let Some(v) = task {
            qb.push(", task = ").push_bind(v.clone());
        }
        if let Some(v) = status {
            qb.push(", status = ").push_bind(v);
        }
        if let Some(v) = category {
            qb.push(", category = ").push_bind(v);
        }

        qb.push(" WHERE id::text = ").push_bind(id);

        let result = qb.build().execute(pool).await?;
        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity: "schedule",
                key: "id",
                value: id.to_string(),
            });
        }
        Ok(result.rows_affected())
    }

    /// Get a schedule's source field (to check if manifest-sourced).
    pub async fn get_source(pool: &PgPool, id: &str) -> Result<String, DbError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT source FROM schedules WHERE id::text = $1"
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some((source,)) => Ok(source),
            None => Err(DbError::NotFound {
                entity: "schedule",
                key: "id",
                value: id.to_string(),
            }),
        }
    }

    /// List active schedules that are due to run (next_run_at <= NOW() or never run).
    pub async fn list_due(pool: &PgPool) -> Result<Vec<ScheduleRow>, DbError> {
        let rows = sqlx::query_as::<_, ScheduleRow>(
            "SELECT s.id, s.agent_id, s.agent_instance_id,
                    COALESCE(s.agent_name, ai.name) as agent_name,
                    s.name, s.cron, s.expression, s.type, s.task, s.source, s.status,
                    s.last_run_at, s.last_run_status, s.next_run_at,
                    s.category, s.description, s.created_at, s.updated_at
             FROM schedules s
             LEFT JOIN agent_instances ai ON ai.id = s.agent_instance_id
             WHERE s.status = 'active'
               AND (s.next_run_at IS NULL OR s.next_run_at <= NOW())"
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Update last_run_at and next_run_at after a schedule fires.
    pub async fn update_run_times(
        pool: &PgPool,
        id: uuid::Uuid,
        last_run_at: time::OffsetDateTime,
        next_run_at: Option<time::OffsetDateTime>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE schedules SET last_run_at = $2, next_run_at = $3, updated_at = NOW()
             WHERE id = $1"
        )
        .bind(id)
        .bind(last_run_at)
        .bind(next_run_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Delete a schedule. Returns error if not found.
    pub async fn delete_schedule(pool: &PgPool, id: &str) -> Result<(), DbError> {
        let result = sqlx::query(
            "DELETE FROM schedules WHERE id::text = $1"
        )
        .bind(id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity: "schedule",
                key: "id",
                value: id.to_string(),
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Dual-backend trait (sera-mwb4)
// ---------------------------------------------------------------------------

/// Schedule surface shared by Postgres and SQLite backends.
///
/// Agent name resolution via JOIN is Postgres-only; the SQLite backend stores
/// `agent_name` directly on the `schedules` row and uses it verbatim.
#[async_trait]
pub trait ScheduleStore: Send + Sync + std::fmt::Debug {
    async fn list_schedules(&self) -> Result<Vec<ScheduleRow>, DbError>;

    #[allow(clippy::too_many_arguments)]
    async fn create_schedule(
        &self,
        id: &str,
        agent_instance_id: Option<&str>,
        agent_name: &str,
        name: &str,
        schedule_type: &str,
        expression: &str,
        task: &serde_json::Value,
        source: &str,
        status: &str,
        category: Option<&str>,
        description: Option<&str>,
    ) -> Result<(), DbError>;

    #[allow(clippy::too_many_arguments)]
    async fn update_schedule(
        &self,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
        expression: Option<&str>,
        task: Option<&serde_json::Value>,
        status: Option<&str>,
        category: Option<&str>,
    ) -> Result<u64, DbError>;

    async fn delete_schedule(&self, id: &str) -> Result<(), DbError>;

    async fn list_due(&self) -> Result<Vec<ScheduleRow>, DbError>;

    async fn update_run_times(
        &self,
        id: uuid::Uuid,
        last_run_at: time::OffsetDateTime,
        next_run_at: Option<time::OffsetDateTime>,
    ) -> Result<(), DbError>;
}

/// Postgres implementation — delegates to [`ScheduleRepository`].
#[derive(Debug, Clone)]
pub struct PgScheduleStore {
    pool: PgPool,
}

impl PgScheduleStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ScheduleStore for PgScheduleStore {
    async fn list_schedules(&self) -> Result<Vec<ScheduleRow>, DbError> {
        ScheduleRepository::list_schedules(&self.pool).await
    }

    async fn create_schedule(
        &self,
        id: &str,
        agent_instance_id: Option<&str>,
        agent_name: &str,
        name: &str,
        schedule_type: &str,
        expression: &str,
        task: &serde_json::Value,
        source: &str,
        status: &str,
        category: Option<&str>,
        description: Option<&str>,
    ) -> Result<(), DbError> {
        ScheduleRepository::create_schedule(
            &self.pool,
            id,
            agent_instance_id,
            agent_name,
            name,
            schedule_type,
            expression,
            task,
            source,
            status,
            category,
            description,
        )
        .await
    }

    async fn update_schedule(
        &self,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
        expression: Option<&str>,
        task: Option<&serde_json::Value>,
        status: Option<&str>,
        category: Option<&str>,
    ) -> Result<u64, DbError> {
        ScheduleRepository::update_schedule(
            &self.pool,
            id,
            name,
            description,
            expression,
            task,
            status,
            category,
        )
        .await
    }

    async fn delete_schedule(&self, id: &str) -> Result<(), DbError> {
        ScheduleRepository::delete_schedule(&self.pool, id).await
    }

    async fn list_due(&self) -> Result<Vec<ScheduleRow>, DbError> {
        ScheduleRepository::list_due(&self.pool).await
    }

    async fn update_run_times(
        &self,
        id: uuid::Uuid,
        last_run_at: time::OffsetDateTime,
        next_run_at: Option<time::OffsetDateTime>,
    ) -> Result<(), DbError> {
        ScheduleRepository::update_run_times(&self.pool, id, last_run_at, next_run_at).await
    }
}

// ---------------------------------------------------------------------------
// SQLite implementation (sera-mwb4)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SqliteScheduleStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteScheduleStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schedules (
                id                 TEXT PRIMARY KEY,
                agent_id           TEXT,
                agent_instance_id  TEXT,
                agent_name         TEXT,
                name               TEXT NOT NULL,
                cron               TEXT,
                expression         TEXT,
                type               TEXT,
                task               TEXT NOT NULL,
                source             TEXT NOT NULL DEFAULT 'api',
                status             TEXT,
                last_run_at        TEXT,
                last_run_status    TEXT,
                next_run_at        TEXT,
                category           TEXT,
                description        TEXT,
                created_at         TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at         TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_schedules_status ON schedules(status);
            CREATE INDEX IF NOT EXISTS idx_schedules_agent_instance ON schedules(agent_instance_id);",
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

fn row_to_schedule(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduleRow> {
    let id_str: String = row.get("id")?;
    let task_str: String = row.get("task")?;
    Ok(ScheduleRow {
        id: uuid::Uuid::parse_str(&id_str).unwrap_or(uuid::Uuid::nil()),
        agent_id: parse_uuid_opt(row.get("agent_id")?),
        agent_instance_id: parse_uuid_opt(row.get("agent_instance_id")?),
        agent_name: row.get("agent_name")?,
        name: row.get("name")?,
        cron: row.get("cron")?,
        expression: row.get("expression")?,
        r#type: row.get("type")?,
        task: serde_json::from_str(&task_str).unwrap_or(serde_json::Value::Null),
        source: row.get("source")?,
        status: row.get("status")?,
        last_run_at: parse_datetime_opt(row.get("last_run_at")?),
        last_run_status: row.get("last_run_status")?,
        next_run_at: parse_datetime_opt(row.get("next_run_at")?),
        category: row.get("category")?,
        description: row.get("description")?,
        created_at: parse_datetime_opt(row.get("created_at")?),
        updated_at: parse_datetime_opt(row.get("updated_at")?),
    })
}

#[async_trait]
impl ScheduleStore for SqliteScheduleStore {
    async fn list_schedules(&self) -> Result<Vec<ScheduleRow>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, agent_id, agent_instance_id, agent_name, name, cron, expression,
                        type, task, source, status, last_run_at, last_run_status, next_run_at,
                        category, description, created_at, updated_at
                 FROM schedules ORDER BY created_at DESC",
            )
            .map_err(|e| DbError::Integrity(format!("sqlite prepare: {e}")))?;
        let rows = stmt
            .query_map([], row_to_schedule)
            .map_err(|e| DbError::Integrity(format!("sqlite query: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| DbError::Integrity(format!("sqlite row: {e}")))?);
        }
        Ok(out)
    }

    async fn create_schedule(
        &self,
        id: &str,
        agent_instance_id: Option<&str>,
        agent_name: &str,
        name: &str,
        schedule_type: &str,
        expression: &str,
        task: &serde_json::Value,
        source: &str,
        status: &str,
        category: Option<&str>,
        description: Option<&str>,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        let task_str = task.to_string();
        conn.execute(
            "INSERT INTO schedules (id, agent_instance_id, agent_name, name, type, expression, task,
                                    source, status, category, description)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                agent_instance_id,
                agent_name,
                name,
                schedule_type,
                expression,
                task_str,
                source,
                status,
                category,
                description
            ],
        )
        .map_err(|e| DbError::Integrity(format!("sqlite insert schedule: {e}")))?;
        Ok(())
    }

    async fn update_schedule(
        &self,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
        expression: Option<&str>,
        task: Option<&serde_json::Value>,
        status: Option<&str>,
        category: Option<&str>,
    ) -> Result<u64, DbError> {
        // Build SQL + args vector synchronously (pre-lock) and only hold the
        // conn guard across rusqlite calls. `String` is `Send`, so the
        // resulting `Vec<String>` crosses the `.await` boundary cleanly.
        let mut sql = String::from("UPDATE schedules SET updated_at = datetime('now')");
        let mut args: Vec<String> = Vec::new();
        if let Some(v) = name {
            sql.push_str(", name = ?");
            args.push(v.to_string());
        }
        if let Some(v) = description {
            sql.push_str(", description = ?");
            args.push(v.to_string());
        }
        if let Some(v) = expression {
            sql.push_str(", expression = ?");
            args.push(v.to_string());
        }
        if let Some(v) = task {
            sql.push_str(", task = ?");
            args.push(v.to_string());
        }
        if let Some(v) = status {
            sql.push_str(", status = ?");
            args.push(v.to_string());
        }
        if let Some(v) = category {
            sql.push_str(", category = ?");
            args.push(v.to_string());
        }
        sql.push_str(" WHERE id = ?");
        args.push(id.to_string());

        let conn = self.conn.lock().await;
        let param_refs: Vec<&dyn rusqlite::ToSql> =
            args.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        let n = conn
            .execute(&sql, param_refs.as_slice())
            .map_err(|e| DbError::Integrity(format!("sqlite update schedule: {e}")))?;
        if n == 0 {
            return Err(DbError::NotFound {
                entity: "schedule",
                key: "id",
                value: id.to_string(),
            });
        }
        Ok(n as u64)
    }

    async fn delete_schedule(&self, id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        let n = conn
            .execute("DELETE FROM schedules WHERE id = ?1", params![id])
            .map_err(|e| DbError::Integrity(format!("sqlite delete schedule: {e}")))?;
        if n == 0 {
            return Err(DbError::NotFound {
                entity: "schedule",
                key: "id",
                value: id.to_string(),
            });
        }
        Ok(())
    }

    async fn list_due(&self) -> Result<Vec<ScheduleRow>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, agent_id, agent_instance_id, agent_name, name, cron, expression,
                        type, task, source, status, last_run_at, last_run_status, next_run_at,
                        category, description, created_at, updated_at
                 FROM schedules
                 WHERE status = 'active'
                   AND (next_run_at IS NULL OR next_run_at <= datetime('now'))",
            )
            .map_err(|e| DbError::Integrity(format!("sqlite prepare due: {e}")))?;
        let rows = stmt
            .query_map([], row_to_schedule)
            .map_err(|e| DbError::Integrity(format!("sqlite query due: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| DbError::Integrity(format!("sqlite row: {e}")))?);
        }
        Ok(out)
    }

    async fn update_run_times(
        &self,
        id: uuid::Uuid,
        last_run_at: time::OffsetDateTime,
        next_run_at: Option<time::OffsetDateTime>,
    ) -> Result<(), DbError> {
        let last_str = last_run_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let next_str = next_run_at.and_then(|t| {
            t.format(&time::format_description::well_known::Rfc3339).ok()
        });
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE schedules SET last_run_at = ?1, next_run_at = ?2, updated_at = datetime('now')
             WHERE id = ?3",
            params![last_str, next_str, id.to_string()],
        )
        .map_err(|e| DbError::Integrity(format!("sqlite update run times: {e}")))?;
        Ok(())
    }
}

impl SqliteScheduleStore {
    /// Fetch the `source` column for a schedule — parity with the Postgres
    /// [`ScheduleRepository::get_source`]. Only used by routes that guard
    /// manifest-sourced schedules against API edits.
    pub async fn get_source(&self, id: &str) -> Result<String, DbError> {
        let conn = self.conn.lock().await;
        let src: Option<String> = conn
            .query_row(
                "SELECT source FROM schedules WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| DbError::Integrity(format!("sqlite get source: {e}")))?;
        src.ok_or(DbError::NotFound {
            entity: "schedule",
            key: "id",
            value: id.to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn new_store() -> SqliteScheduleStore {
        let conn = Connection::open_in_memory().unwrap();
        SqliteScheduleStore::init_schema(&conn).unwrap();
        SqliteScheduleStore::new(Arc::new(Mutex::new(conn)))
    }

    fn new_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    #[tokio::test]
    async fn create_and_list_roundtrip() {
        let store = new_store();
        let id = new_id();
        store
            .create_schedule(
                &id,
                None,
                "agent-1",
                "nightly",
                "cron",
                "0 0 * * * *",
                &serde_json::json!({"prompt": "hi"}),
                "api",
                "active",
                Some("cron_schedule"),
                Some("nightly task"),
            )
            .await
            .unwrap();
        let rows = store.list_schedules().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "nightly");
        assert_eq!(rows[0].agent_name.as_deref(), Some("agent-1"));
        assert_eq!(rows[0].task["prompt"], "hi");
    }

    #[tokio::test]
    async fn update_schedule_fields() {
        let store = new_store();
        let id = new_id();
        store
            .create_schedule(
                &id,
                None,
                "agent-1",
                "old",
                "cron",
                "* * * * * *",
                &serde_json::json!({}),
                "api",
                "active",
                None,
                None,
            )
            .await
            .unwrap();
        store
            .update_schedule(&id, Some("new"), None, Some("0 * * * * *"), None, None, None)
            .await
            .unwrap();
        let rows = store.list_schedules().await.unwrap();
        assert_eq!(rows[0].name, "new");
        assert_eq!(rows[0].expression.as_deref(), Some("0 * * * * *"));
    }

    #[tokio::test]
    async fn delete_missing_returns_not_found() {
        let store = new_store();
        let err = store.delete_schedule(&new_id()).await.unwrap_err();
        matches!(err, DbError::NotFound { .. });
    }

    #[tokio::test]
    async fn list_due_filters_on_status_and_next_run() {
        let store = new_store();
        for (name, status) in [("a", "active"), ("b", "active"), ("c", "paused")] {
            store
                .create_schedule(
                    &new_id(),
                    None,
                    "agent-1",
                    name,
                    "cron",
                    "0 * * * * *",
                    &serde_json::json!({}),
                    "api",
                    status,
                    None,
                    None,
                )
                .await
                .unwrap();
        }
        let due = store.list_due().await.unwrap();
        // Only 'active' schedules with next_run_at IS NULL are due.
        assert_eq!(due.len(), 2);
        assert!(due.iter().all(|s| s.status.as_deref() == Some("active")));
    }

    #[tokio::test]
    async fn tenant_isolation_by_agent_instance_id() {
        let store = new_store();
        let tenant_a = uuid::Uuid::new_v4().to_string();
        let tenant_b = uuid::Uuid::new_v4().to_string();
        for tid in &[tenant_a.clone(), tenant_b.clone()] {
            store
                .create_schedule(
                    &new_id(),
                    Some(tid),
                    "agent",
                    "s",
                    "cron",
                    "0 * * * * *",
                    &serde_json::json!({}),
                    "api",
                    "active",
                    None,
                    None,
                )
                .await
                .unwrap();
        }
        let rows = store.list_schedules().await.unwrap();
        let tenant_a_rows: Vec<_> = rows
            .iter()
            .filter(|r| {
                r.agent_instance_id
                    .map(|u| u.to_string()) == Some(tenant_a.clone())
            })
            .collect();
        assert_eq!(tenant_a_rows.len(), 1);
    }
}
