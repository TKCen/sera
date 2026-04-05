//! Circles repository — read access to the circles table.

use sqlx::PgPool;
use crate::error::DbError;

/// Row type for circles table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CircleRow {
    pub id: uuid::Uuid,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub constitution: Option<String>,
    pub created_at: Option<time::OffsetDateTime>,
    pub updated_at: Option<time::OffsetDateTime>,
}

pub struct CircleRepository;

impl CircleRepository {
    pub async fn list_circles(pool: &PgPool) -> Result<Vec<CircleRow>, DbError> {
        let rows = sqlx::query_as::<_, CircleRow>(
            "SELECT id, name, display_name, description, constitution, created_at, updated_at
             FROM circles ORDER BY name"
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn create_circle(
        pool: &PgPool,
        id: &str,
        name: &str,
        display_name: &str,
        description: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO circles (id, name, display_name, description, created_at, updated_at)
             VALUES ($1::uuid, $2, $3, $4, NOW(), NOW())"
        )
        .bind(id)
        .bind(name)
        .bind(display_name)
        .bind(description)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Get a circle by name (or id).
    pub async fn get_by_name(pool: &PgPool, name: &str) -> Result<CircleRow, DbError> {
        sqlx::query_as::<_, CircleRow>(
            "SELECT id, name, display_name, description, constitution, created_at, updated_at
             FROM circles WHERE name = $1 OR id::text = $1"
        )
        .bind(name)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DbError::NotFound {
            entity: "circle",
            key: "name",
            value: name.to_string(),
        })
    }

    pub async fn delete_circle(pool: &PgPool, id: &str) -> Result<(), DbError> {
        let result = sqlx::query("DELETE FROM circles WHERE id::text = $1")
            .bind(id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity: "circle",
                key: "id",
                value: id.to_string(),
            });
        }
        Ok(())
    }
}
