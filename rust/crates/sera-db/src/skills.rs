//! Skills repository — read access to the skills table.

use sqlx::PgPool;
use crate::error::DbError;

/// Row type for skills table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SkillRow {
    pub id: uuid::Uuid,
    pub skill_id: Option<String>,
    pub name: String,
    pub version: String,
    pub description: String,
    pub triggers: serde_json::Value,
    pub requires: Option<serde_json::Value>,
    pub conflicts: Option<serde_json::Value>,
    pub max_tokens: Option<i32>,
    pub source: String,
    pub category: Option<String>,
    pub tags: Option<serde_json::Value>,
    pub applies_to: Option<serde_json::Value>,
    pub created_at: Option<time::OffsetDateTime>,
    pub updated_at: Option<time::OffsetDateTime>,
}

pub struct SkillRepository;

impl SkillRepository {
    pub async fn list_skills(pool: &PgPool) -> Result<Vec<SkillRow>, DbError> {
        let rows = sqlx::query_as::<_, SkillRow>(
            "SELECT id, skill_id, name, version, description, triggers, requires, conflicts,
                    max_tokens, source, category, tags, applies_to, created_at, updated_at
             FROM skills ORDER BY name, version"
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Create a new skill.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_skill(
        pool: &PgPool,
        name: &str,
        version: &str,
        description: &str,
        triggers: &serde_json::Value,
        content: &str,
        category: Option<&str>,
        tags: Option<&serde_json::Value>,
        max_tokens: Option<i32>,
    ) -> Result<SkillRow, DbError> {
        let id = uuid::Uuid::new_v4();
        sqlx::query(
            "INSERT INTO skills (id, name, version, description, triggers, content, source, category, tags, max_tokens)
             VALUES ($1, $2, $3, $4, $5, $6, 'external', $7, COALESCE($8, '[]'::jsonb), $9)"
        )
        .bind(id)
        .bind(name)
        .bind(version)
        .bind(description)
        .bind(triggers)
        .bind(content)
        .bind(category)
        .bind(tags)
        .bind(max_tokens)
        .execute(pool)
        .await?;

        // Fetch and return the created row
        let row = sqlx::query_as::<_, SkillRow>(
            "SELECT id, skill_id, name, version, description, triggers, requires, conflicts,
                    max_tokens, source, category, tags, applies_to, created_at, updated_at
             FROM skills WHERE id = $1"
        )
        .bind(id)
        .fetch_one(pool)
        .await?;
        Ok(row)
    }

    /// Get a skill by name.
    pub async fn get_by_name(pool: &PgPool, name: &str) -> Result<SkillRow, DbError> {
        sqlx::query_as::<_, SkillRow>(
            "SELECT id, skill_id, name, version, description, triggers, requires, conflicts,
                    max_tokens, source, category, tags, applies_to, created_at, updated_at
             FROM skills WHERE name = $1"
        )
        .bind(name)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DbError::NotFound {
            entity: "skill",
            key: "name",
            value: name.to_string(),
        })
    }

    /// Delete a skill by name.
    pub async fn delete_skill(pool: &PgPool, name: &str) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM skills WHERE name = $1")
            .bind(name)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
