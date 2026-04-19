//! Circles repository — read access to the circles table.

use sqlx::PgPool;
use crate::error::DbError;

/// Row type for `circle_constitution_versions` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ConstitutionVersionRow {
    pub circle_id: String,
    pub version: i32,
    pub text_hash: String,
    pub changed_by: String,
    pub changed_at: time::OffsetDateTime,
}

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

    /// Update the `constitution` column on a circle.
    pub async fn update_constitution(
        pool: &PgPool,
        circle_id: &str,
        constitution_text: Option<&str>,
    ) -> Result<(), DbError> {
        let affected = sqlx::query(
            "UPDATE circles SET constitution = $1, updated_at = NOW()
             WHERE id::text = $2 OR name = $2"
        )
        .bind(constitution_text)
        .bind(circle_id)
        .execute(pool)
        .await?
        .rows_affected();
        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "circle",
                key: "id",
                value: circle_id.to_string(),
            });
        }
        Ok(())
    }

    /// Append a constitution audit entry for `circle_id`, returning the new version number.
    ///
    /// The version is `MAX(version) + 1` for that circle, or `1` if none exist.
    pub async fn record_constitution_update(
        pool: &PgPool,
        circle_id: &str,
        text_hash: &str,
        changed_by: &str,
    ) -> Result<i32, DbError> {
        let row: (i32,) = sqlx::query_as(
            "INSERT INTO circle_constitution_versions (circle_id, version, text_hash, changed_by)
             SELECT $1,
                    COALESCE((SELECT MAX(version) FROM circle_constitution_versions WHERE circle_id = $1), 0) + 1,
                    $2,
                    $3
             RETURNING version"
        )
        .bind(circle_id)
        .bind(text_hash)
        .bind(changed_by)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Retrieve all audit entries for a circle, ordered by version ascending.
    pub async fn get_constitution_versions(
        pool: &PgPool,
        circle_id: &str,
    ) -> Result<Vec<ConstitutionVersionRow>, DbError> {
        let rows = sqlx::query_as::<_, ConstitutionVersionRow>(
            "SELECT circle_id, version, text_hash, changed_by, changed_at
             FROM circle_constitution_versions
             WHERE circle_id = $1
             ORDER BY version ASC"
        )
        .bind(circle_id)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }
}
