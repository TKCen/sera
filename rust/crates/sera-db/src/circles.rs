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
}
