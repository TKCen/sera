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
}
