//! Webhooks repository.

use sqlx::PgPool;

use crate::error::DbError;

/// Row type for webhooks table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WebhookRow {
    pub id: uuid::Uuid,
    pub name: String,
    pub url_path: String,
    pub secret: String,
    pub event_type: String,
    pub enabled: bool,
    pub created_at: time::OffsetDateTime,
    pub updated_at: time::OffsetDateTime,
}

pub struct WebhookRepository;

impl WebhookRepository {
    /// List all webhooks.
    pub async fn list(pool: &PgPool) -> Result<Vec<WebhookRow>, DbError> {
        let rows = sqlx::query_as::<_, WebhookRow>(
            "SELECT id, name, url_path, secret, event_type, enabled, created_at, updated_at
             FROM webhooks ORDER BY name",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Register a new webhook.
    pub async fn create(
        pool: &PgPool,
        name: &str,
        url_path: &str,
        secret: &str,
        event_type: &str,
    ) -> Result<WebhookRow, DbError> {
        let id = uuid::Uuid::new_v4();
        sqlx::query(
            "INSERT INTO webhooks (id, name, url_path, secret, event_type)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(name)
        .bind(url_path)
        .bind(secret)
        .bind(event_type)
        .execute(pool)
        .await?;

        sqlx::query_as::<_, WebhookRow>(
            "SELECT id, name, url_path, secret, event_type, enabled, created_at, updated_at
             FROM webhooks WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(DbError::from)
    }
}
