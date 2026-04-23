//! Schedule service — cron and one-shot task scheduling.

use cron::Schedule;
use std::str::FromStr;
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

use sera_db::{DbPool, schedules::ScheduleRepository, tasks::TaskRepository};

/// Error type for schedule operations.
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("database error: {0}")]
    Db(#[from] sera_db::DbError),
    #[error("invalid cron expression: {0}")]
    InvalidCron(String),
    #[error("schedule not found: {0}")]
    NotFound(String),
}

/// Schedule service for managing cron and one-shot schedules.
pub struct ScheduleService {
    pool: DbPool,
}

impl ScheduleService {
    /// Create a new schedule service.
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    /// Validate a cron expression.
    fn validate_cron(expr: &str) -> Result<(), ScheduleError> {
        Schedule::from_str(expr)
            .map_err(|e| ScheduleError::InvalidCron(format!("Invalid cron: {}", e)))?;
        Ok(())
    }

    /// Create a new cron schedule.
    ///
    /// # Arguments
    /// * `agent_id` — UUID of the agent (optional, for multi-agent scheduling)
    /// * `cron_expr` — valid cron expression (e.g., "0 0 * * *" for daily at midnight)
    /// * `name` — human-readable name for the schedule
    /// * `task` — JSON payload for the task
    ///
    /// # Returns
    /// The ID of the newly created schedule.
    pub async fn create_schedule(
        &self,
        agent_id: Option<Uuid>,
        cron_expr: &str,
        name: &str,
        task: &serde_json::Value,
    ) -> Result<Uuid, ScheduleError> {
        Self::validate_cron(cron_expr)?;

        let id = Uuid::new_v4();
        let agent_instance_id = agent_id.map(|id| id.to_string());
        let agent_name = format!("agent_{}", agent_id.unwrap_or_default());

        ScheduleRepository::create_schedule(
            self.pool.inner(),
            &id.to_string(),
            agent_instance_id.as_deref(),
            &agent_name,
            name,
            "cron",
            cron_expr,
            task,
            "api",
            "active",
            Some("cron_schedule"),
            Some(name),
        )
        .await?;

        Ok(id)
    }

    /// List all schedules.
    pub async fn list_schedules(&self) -> Result<Vec<ScheduleRow>, ScheduleError> {
        let rows = ScheduleRepository::list_schedules(self.pool.inner()).await?;
        Ok(rows
            .into_iter()
            .map(|r| ScheduleRow {
                id: r.id,
                agent_id: r.agent_id,
                agent_instance_id: r.agent_instance_id,
                agent_name: r.agent_name,
                name: r.name,
                cron: r.cron,
                expression: r.expression,
                schedule_type: r.r#type,
                task: r.task,
                source: r.source,
                status: r.status,
                last_run_at: r.last_run_at,
                last_run_status: r.last_run_status,
                next_run_at: r.next_run_at,
                category: r.category,
                description: r.description,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }

    /// Update a schedule.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_schedule(
        &self,
        id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
        expression: Option<&str>,
        task: Option<&serde_json::Value>,
        status: Option<&str>,
        category: Option<&str>,
    ) -> Result<(), ScheduleError> {
        // Validate cron if provided
        if let Some(expr) = expression {
            Self::validate_cron(expr)?;
        }

        ScheduleRepository::update_schedule(
            self.pool.inner(),
            &id.to_string(),
            name,
            description,
            expression,
            task,
            status,
            category,
        )
        .await?;

        Ok(())
    }

    /// Delete a schedule.
    pub async fn delete_schedule(&self, id: Uuid) -> Result<(), ScheduleError> {
        ScheduleRepository::delete_schedule(self.pool.inner(), &id.to_string()).await?;
        Ok(())
    }

    /// Parse a cron expression, accepting both 5-field (standard unix) and 6-field (with seconds).
    fn parse_cron(expr: &str) -> Option<Schedule> {
        Schedule::from_str(expr)
            .or_else(|_| Schedule::from_str(&format!("0 {expr}")))
            .ok()
    }

    /// Compute the next run time for a cron expression.
    fn next_run_at(expr: &str) -> Option<OffsetDateTime> {
        let schedule = Self::parse_cron(expr)?;
        let next = schedule.upcoming(chrono::Utc).next()?;
        OffsetDateTime::from_unix_timestamp(next.timestamp()).ok()
    }

    /// Enqueue a task for the agent owning the given schedule.
    pub async fn trigger_schedule(&self, id: Uuid) -> Result<(), ScheduleError> {
        // Look up the schedule to get agent_instance_id and task prompt
        let rows = ScheduleRepository::list_schedules(self.pool.inner()).await?;
        let schedule = rows
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| ScheduleError::NotFound(id.to_string()))?;

        let agent_instance_id = schedule.agent_instance_id.ok_or_else(|| {
            ScheduleError::NotFound(format!("schedule {id} has no agent_instance_id"))
        })?;

        // Extract prompt text from task JSON
        let task_text = if let Some(p) = schedule.task.get("prompt").and_then(|v| v.as_str()) {
            p.to_string()
        } else {
            schedule.task.to_string()
        };

        let context = serde_json::json!({
            "schedule": {
                "scheduleId": id.to_string(),
                "scheduleName": schedule.name,
                "category": schedule.category,
            }
        });

        TaskRepository::enqueue(
            self.pool.inner(),
            &agent_instance_id.to_string(),
            &task_text,
            Some(&context),
            Some(50), // schedules get higher priority than default 100
        )
        .await
        .map_err(ScheduleError::Db)?;

        tracing::info!(
            "Triggered schedule '{}' (id={}) for agent {}",
            schedule.name,
            id,
            agent_instance_id
        );
        Ok(())
    }

    /// Check and process schedules that are due to run.
    ///
    /// This should be called periodically (e.g., every minute) to process due cron schedules.
    pub async fn process_due_schedules(&self) -> Result<usize, ScheduleError> {
        let due = ScheduleRepository::list_due(self.pool.inner()).await?;
        let count = due.len();
        if count == 0 {
            return Ok(0);
        }

        tracing::info!("Processing {} due schedule(s)", count);
        let now = OffsetDateTime::now_utc();

        for schedule in due {
            let id = schedule.id;
            let name = schedule.name.clone();

            // Enqueue the task
            if let Err(e) = self.trigger_schedule(id).await {
                tracing::error!("Failed to trigger schedule '{}' ({}): {}", name, id, e);
                continue;
            }

            // Calculate next_run_at from expression
            let next = schedule.expression.as_deref().and_then(Self::next_run_at);

            if let Err(e) =
                ScheduleRepository::update_run_times(self.pool.inner(), id, now, next).await
            {
                tracing::error!("Failed to update run times for schedule '{}': {}", name, e);
            }
        }

        Ok(count)
    }

    /// Schedule a one-shot task to run after a delay.
    ///
    /// # Arguments
    /// * `agent_id` — UUID of the agent
    /// * `delay_ms` — milliseconds to wait before execution
    /// * `task_data` — JSON payload for the task
    ///
    /// # Returns
    /// The ID of the scheduled task.
    pub async fn schedule_once(
        &self,
        agent_id: Uuid,
        delay_ms: u64,
        task_data: &serde_json::Value,
    ) -> Result<Uuid, ScheduleError> {
        let id = Uuid::new_v4();
        let agent_instance_id = agent_id.to_string();
        let agent_name = format!("agent_{}", agent_id);
        let name = format!("one-shot-{}", &id.to_string()[..8]);

        // Calculate scheduled time
        let scheduled_at = OffsetDateTime::now_utc() + Duration::from_millis(delay_ms);
        let scheduled_str = scheduled_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        ScheduleRepository::create_schedule(
            self.pool.inner(),
            &id.to_string(),
            Some(&agent_instance_id),
            &agent_name,
            &name,
            "one-shot",
            &scheduled_str,
            task_data,
            "api",
            "active",
            Some("one_shot"),
            Some(&format!("One-shot task scheduled for {}ms", delay_ms)),
        )
        .await?;

        Ok(id)
    }
}

/// Wrapper around ScheduleRow for internal use.
#[derive(Debug, Clone)]
pub struct ScheduleRow {
    pub id: uuid::Uuid,
    pub agent_id: Option<uuid::Uuid>,
    pub agent_instance_id: Option<uuid::Uuid>,
    pub agent_name: Option<String>,
    pub name: String,
    pub cron: Option<String>,
    pub expression: Option<String>,
    pub schedule_type: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_cron_valid() {
        // cron crate requires 6-7 fields (sec min hour dom month dow [year])
        assert!(ScheduleService::validate_cron("0 0 0 * * *").is_ok());
        assert!(ScheduleService::validate_cron("0 */5 * * * *").is_ok());
        assert!(ScheduleService::validate_cron("0 0 9-17 * * 1-5").is_ok());
    }

    #[test]
    fn test_validate_cron_invalid() {
        assert!(ScheduleService::validate_cron("invalid").is_err());
        assert!(ScheduleService::validate_cron("99 99 99 99 99").is_err());
        assert!(ScheduleService::validate_cron("").is_err());
    }
}
