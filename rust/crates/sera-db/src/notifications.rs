//! Notification channels repository.

use sqlx::PgPool;

use crate::error::DbError;

/// Row type for notification_channels table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NotificationChannelRow {
    pub id: uuid::Uuid,
    pub name: String,
    pub r#type: String,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub created_at: Option<time::OffsetDateTime>,
    pub description: Option<String>,
}

pub struct NotificationRepository;

impl NotificationRepository {
    /// List all notification channels.
    pub async fn list(pool: &PgPool) -> Result<Vec<NotificationChannelRow>, DbError> {
        let rows = sqlx::query_as::<_, NotificationChannelRow>(
            "SELECT id, name, type, config, enabled, created_at, description
             FROM notification_channels ORDER BY name",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Create a notification channel.
    pub async fn create(
        pool: &PgPool,
        id: &str,
        name: &str,
        channel_type: &str,
        config: &serde_json::Value,
        description: Option<&str>,
    ) -> Result<NotificationChannelRow, DbError> {
        sqlx::query(
            "INSERT INTO notification_channels (id, name, type, config, description)
             VALUES ($1::uuid, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(name)
        .bind(channel_type)
        .bind(config)
        .bind(description)
        .execute(pool)
        .await?;

        sqlx::query_as::<_, NotificationChannelRow>(
            "SELECT id, name, type, config, enabled, created_at, description
             FROM notification_channels WHERE id = $1::uuid",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(DbError::from)
    }

    /// Delete a notification channel.
    pub async fn delete(pool: &PgPool, id: &str) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM notification_channels WHERE id = $1::uuid")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
