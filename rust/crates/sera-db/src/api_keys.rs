//! API keys repository.

use sqlx::PgPool;

use crate::error::DbError;

/// Row type for api_keys table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ApiKeyRow {
    pub id: uuid::Uuid,
    pub name: String,
    pub key_hash: String,
    pub owner_sub: String,
    pub roles: Vec<String>,
    pub created_at: Option<time::OffsetDateTime>,
    pub expires_at: Option<time::OffsetDateTime>,
    pub last_used_at: Option<time::OffsetDateTime>,
    pub revoked_at: Option<time::OffsetDateTime>,
}

pub struct ApiKeyRepository;

impl ApiKeyRepository {
    /// List API keys (no hash exposed).
    pub async fn list(pool: &PgPool, owner_sub: Option<&str>) -> Result<Vec<ApiKeyRow>, DbError> {
        let rows = if let Some(owner) = owner_sub {
            sqlx::query_as::<_, ApiKeyRow>(
                "SELECT id, name, key_hash, owner_sub, roles, created_at, expires_at, last_used_at, revoked_at
                 FROM api_keys WHERE owner_sub = $1 AND revoked_at IS NULL
                 ORDER BY created_at DESC",
            )
            .bind(owner)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, ApiKeyRow>(
                "SELECT id, name, key_hash, owner_sub, roles, created_at, expires_at, last_used_at, revoked_at
                 FROM api_keys WHERE revoked_at IS NULL
                 ORDER BY created_at DESC",
            )
            .fetch_all(pool)
            .await?
        };
        Ok(rows)
    }

    /// Create a new API key.
    pub async fn create(
        pool: &PgPool,
        name: &str,
        key_hash: &str,
        owner_sub: &str,
        roles: &[String],
    ) -> Result<ApiKeyRow, DbError> {
        let id = uuid::Uuid::new_v4();
        sqlx::query(
            "INSERT INTO api_keys (id, name, key_hash, owner_sub, roles)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(name)
        .bind(key_hash)
        .bind(owner_sub)
        .bind(roles)
        .execute(pool)
        .await?;

        sqlx::query_as::<_, ApiKeyRow>(
            "SELECT id, name, key_hash, owner_sub, roles, created_at, expires_at, last_used_at, revoked_at
             FROM api_keys WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(DbError::from)
    }

    /// Revoke (soft-delete) an API key.
    pub async fn revoke(pool: &PgPool, id: &str) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE api_keys SET revoked_at = NOW() WHERE id = $1::uuid AND revoked_at IS NULL",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}
