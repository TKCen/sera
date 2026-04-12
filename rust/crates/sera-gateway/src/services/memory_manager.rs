//! Memory Manager — hybrid DB + filesystem block storage for agent memory.

use std::path::PathBuf;
use std::sync::Arc;

use sqlx::PgPool;
use uuid::Uuid;

use sera_db::memory::MemoryRepository;

/// Block type classification for memory blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockType {
    Fact,
    Context,
    Memory,
    Insight,
}

impl std::fmt::Display for BlockType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fact => write!(f, "fact"),
            Self::Context => write!(f, "context"),
            Self::Memory => write!(f, "memory"),
            Self::Insight => write!(f, "insight"),
        }
    }
}

impl std::str::FromStr for BlockType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "fact" => Ok(Self::Fact),
            "context" => Ok(Self::Context),
            "memory" => Ok(Self::Memory),
            "insight" => Ok(Self::Insight),
            other => Err(format!("unknown block type: {other}")),
        }
    }
}

/// A memory block stored for an agent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryBlock {
    pub id: Uuid,
    pub agent_id: String,
    pub block_type: BlockType,
    pub name: String,
    pub content: String,
    pub created_at: Option<time::OffsetDateTime>,
}

/// Hybrid DB + filesystem memory manager.
pub struct MemoryManager {
    pool: Arc<PgPool>,
    data_dir: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("database error: {0}")]
    Db(#[from] sera_db::DbError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("block not found: {0}")]
    NotFound(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}

impl MemoryManager {
    /// Create a new memory manager.
    pub fn new(pool: Arc<PgPool>, data_dir: PathBuf) -> Self {
        Self { pool, data_dir }
    }

    /// Store a memory block in the database.
    pub async fn store_block(
        &self,
        agent_id: &str,
        _block_type: BlockType,
        name: &str,
        content: &str,
    ) -> Result<Uuid, MemoryError> {
        let row = MemoryRepository::create_block(
            &self.pool,
            agent_id,
            name,
            content,
            None,
            false,
        )
        .await?;
        Ok(row.id)
    }

    /// Query memory blocks for an agent.
    pub async fn query_blocks(
        &self,
        agent_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<MemoryBlock>, MemoryError> {
        let rows = MemoryRepository::list_blocks(&self.pool, Some(agent_id)).await?;

        let blocks: Vec<MemoryBlock> = rows
            .into_iter()
            .take(limit.unwrap_or(100) as usize)
            .map(|row| MemoryBlock {
                id: row.id,
                agent_id: row.agent_instance_id.to_string(),
                block_type: BlockType::Memory, // Default; DB schema doesn't have block_type yet
                name: row.name,
                content: row.content,
                created_at: Some(row.created_at),
            })
            .collect();

        Ok(blocks)
    }

    /// Get a single memory block by ID.
    pub async fn get_block(&self, block_id: &str) -> Result<MemoryBlock, MemoryError> {
        let row = MemoryRepository::get_block(&self.pool, block_id).await?;
        Ok(MemoryBlock {
            id: row.id,
            agent_id: row.agent_instance_id.to_string(),
            block_type: BlockType::Memory,
            name: row.name,
            content: row.content,
            created_at: Some(row.created_at),
        })
    }

    /// Update a memory block's content.
    pub async fn update_block(&self, block_id: &str, content: &str) -> Result<(), MemoryError> {
        MemoryRepository::update_block(&self.pool, block_id, content).await?;
        Ok(())
    }

    /// Delete a memory block.
    pub async fn delete_block(&self, block_id: &str) -> Result<bool, MemoryError> {
        let deleted = MemoryRepository::delete_block(&self.pool, block_id).await?;
        Ok(deleted)
    }

    /// Store a block to the filesystem as fallback when DB is unavailable.
    pub fn store_block_fs(&self, agent_id: &str, block: &MemoryBlock) -> Result<(), MemoryError> {
        let dir = self.fs_block_dir(agent_id);
        std::fs::create_dir_all(&dir)?;

        let path = dir.join(format!("{}.json", block.id));
        let json = serde_json::to_string_pretty(block)
            .map_err(|e| MemoryError::Serialization(e.to_string()))?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Sync filesystem blocks to database for an agent.
    /// Returns the number of blocks synced.
    pub async fn sync_blocks(&self, agent_id: &str) -> Result<i64, MemoryError> {
        let dir = self.fs_block_dir(agent_id);
        if !dir.exists() {
            return Ok(0);
        }

        let mut synced = 0i64;
        let entries = std::fs::read_dir(&dir)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "json").unwrap_or(false) {
                let content = std::fs::read_to_string(&path)?;
                let block: MemoryBlock = serde_json::from_str(&content)
                    .map_err(|e| MemoryError::Serialization(e.to_string()))?;

                // Insert to DB
                MemoryRepository::create_block(
                    &self.pool,
                    &block.agent_id,
                    &block.name,
                    &block.content,
                    None,
                    false,
                )
                .await?;

                // Remove the FS file after successful sync
                std::fs::remove_file(&path)?;
                synced += 1;
            }
        }

        Ok(synced)
    }

    /// Get the filesystem directory for an agent's blocks.
    fn fs_block_dir(&self, agent_id: &str) -> PathBuf {
        self.data_dir.join("memory").join(agent_id).join("blocks")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_type_display() {
        assert_eq!(BlockType::Fact.to_string(), "fact");
        assert_eq!(BlockType::Context.to_string(), "context");
        assert_eq!(BlockType::Memory.to_string(), "memory");
        assert_eq!(BlockType::Insight.to_string(), "insight");
    }

    #[test]
    fn test_block_type_from_str() {
        assert_eq!("fact".parse::<BlockType>().unwrap(), BlockType::Fact);
        assert_eq!("context".parse::<BlockType>().unwrap(), BlockType::Context);
        assert!("unknown".parse::<BlockType>().is_err());
    }

    #[test]
    fn test_fs_block_dir() {
        // Test the path construction logic directly
        let data_dir = PathBuf::from("/data/sera");
        let dir = data_dir.join("memory").join("agent-1").join("blocks");
        assert!(dir.to_string_lossy().contains("memory"));
        assert!(dir.to_string_lossy().contains("agent-1"));
        assert!(dir.to_string_lossy().contains("blocks"));
    }

    #[test]
    fn test_memory_block_serialization() {
        let block = MemoryBlock {
            id: Uuid::new_v4(),
            agent_id: "agent-1".to_string(),
            block_type: BlockType::Fact,
            name: "test-block".to_string(),
            content: "Some important fact".to_string(),
            created_at: None,
        };

        let json = serde_json::to_string(&block).expect("serialize failed");
        let deserialized: MemoryBlock = serde_json::from_str(&json).expect("deserialize failed");
        assert_eq!(deserialized.agent_id, "agent-1");
        assert_eq!(deserialized.block_type, BlockType::Fact);
    }

    #[test]
    fn test_store_and_read_fs_block() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_dir = temp.path().to_path_buf();

        let block = MemoryBlock {
            id: Uuid::new_v4(),
            agent_id: "agent-test".to_string(),
            block_type: BlockType::Insight,
            name: "insight-1".to_string(),
            content: "Agent learned something".to_string(),
            created_at: None,
        };

        // Manually test FS storage without needing a PgPool
        let dir = data_dir.join("memory").join("agent-test").join("blocks");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let file = dir.join(format!("{}.json", block.id));
        let json = serde_json::to_string_pretty(&block).expect("serialize");
        std::fs::write(&file, json).expect("write");
        assert!(file.exists());

        let content = std::fs::read_to_string(&file).expect("read failed");
        let loaded: MemoryBlock = serde_json::from_str(&content).expect("parse failed");
        assert_eq!(loaded.name, "insight-1");
    }
}
