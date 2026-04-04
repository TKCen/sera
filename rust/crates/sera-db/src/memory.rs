//! Memory repository — core memory blocks for agent instances.

use sqlx::PgPool;

use crate::error::DbError;

/// Row type for core_memory_blocks table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MemoryBlockRow {
    pub id: uuid::Uuid,
    pub agent_instance_id: uuid::Uuid,
    pub name: String,
    pub content: String,
    pub character_limit: i32,
    pub is_read_only: bool,
    pub created_at: time::OffsetDateTime,
    pub updated_at: time::OffsetDateTime,
}

pub struct MemoryRepository;

impl MemoryRepository {
    /// List all memory blocks, optionally filtered by agent.
    pub async fn list_blocks(
        pool: &PgPool,
        agent_instance_id: Option<&str>,
    ) -> Result<Vec<MemoryBlockRow>, DbError> {
        let rows = if let Some(aid) = agent_instance_id {
            sqlx::query_as::<_, MemoryBlockRow>(
                "SELECT id, agent_instance_id, name, content, character_limit, is_read_only, created_at, updated_at
                 FROM core_memory_blocks WHERE agent_instance_id = $1::uuid
                 ORDER BY name",
            )
            .bind(aid)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, MemoryBlockRow>(
                "SELECT id, agent_instance_id, name, content, character_limit, is_read_only, created_at, updated_at
                 FROM core_memory_blocks ORDER BY agent_instance_id, name",
            )
            .fetch_all(pool)
            .await?
        };
        Ok(rows)
    }

    /// Get a single memory block by ID.
    pub async fn get_block(pool: &PgPool, id: &str) -> Result<MemoryBlockRow, DbError> {
        sqlx::query_as::<_, MemoryBlockRow>(
            "SELECT id, agent_instance_id, name, content, character_limit, is_read_only, created_at, updated_at
             FROM core_memory_blocks WHERE id = $1::uuid",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DbError::NotFound {
            entity: "memory_block",
            key: "id",
            value: id.to_string(),
        })
    }

    /// Create a new memory block.
    pub async fn create_block(
        pool: &PgPool,
        agent_instance_id: &str,
        name: &str,
        content: &str,
        character_limit: Option<i32>,
        is_read_only: bool,
    ) -> Result<MemoryBlockRow, DbError> {
        let id = uuid::Uuid::new_v4();
        let limit = character_limit.unwrap_or(2000);

        sqlx::query(
            "INSERT INTO core_memory_blocks (id, agent_instance_id, name, content, character_limit, is_read_only)
             VALUES ($1, $2::uuid, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(agent_instance_id)
        .bind(name)
        .bind(content)
        .bind(limit)
        .bind(is_read_only)
        .execute(pool)
        .await?;

        Self::get_block(pool, &id.to_string()).await
    }

    /// Update a memory block's content.
    pub async fn update_block(
        pool: &PgPool,
        id: &str,
        content: &str,
    ) -> Result<MemoryBlockRow, DbError> {
        let result = sqlx::query(
            "UPDATE core_memory_blocks SET content = $1, updated_at = NOW() WHERE id = $2::uuid AND is_read_only = false",
        )
        .bind(content)
        .bind(id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity: "memory_block",
                key: "id",
                value: id.to_string(),
            });
        }

        Self::get_block(pool, id).await
    }

    /// Delete a memory block.
    pub async fn delete_block(pool: &PgPool, id: &str) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM core_memory_blocks WHERE id = $1::uuid")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
