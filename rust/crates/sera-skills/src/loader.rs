//! Skill pack loader for discovering and loading skill packs.
//!
//! The [`SkillLoader`] provides a way to discover and load skill packs from
//! various sources (filesystem, embedded, etc.).

use std::path::PathBuf;
use async_trait::async_trait;
use tokio::fs;
use tracing::{debug, warn};

use crate::error::SkillsError;
use crate::skill_pack::{SkillPack, SkillPackMetadata};
use crate::bundle::SkillBundle;
use sera_types::skill::{SkillDefinition, SkillConfig, SkillState, SkillMode};

/// A skill loader that loads packs from the filesystem.
#[derive(Debug, Clone)]
pub struct SkillLoader {
    base_path: PathBuf,
}

impl SkillLoader {
    /// Create a new skill loader with the given base path.
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Load a skill pack by name from the filesystem.
    pub async fn load(&self, name: &str) -> Result<FileSystemSkillPack, SkillsError> {
        let pack_path = self.base_path.join(name);
        
        if !pack_path.exists() {
            return Err(SkillsError::NotFound(name.to_string()));
        }

        debug!(path = %pack_path.display(), "loading skill pack from filesystem");
        Ok(FileSystemSkillPack::new(name.to_string(), pack_path))
    }

    /// List all available skill packs in the base path.
    pub async fn list_packs(&self) -> Result<Vec<String>, SkillsError> {
        let mut entries = fs::read_dir(&self.base_path).await?;
        let mut packs = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir()
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
            {
                packs.push(name.to_string());
            }
        }

        Ok(packs)
    }
}

/// A skill pack loaded from the filesystem.
#[derive(Debug)]
pub struct FileSystemSkillPack {
    name: String,
    path: PathBuf,
}

impl FileSystemSkillPack {
    /// Create a new filesystem-backed skill pack.
    pub fn new(name: String, path: PathBuf) -> Self {
        Self { name, path }
    }

    /// Get the path to a skill file within this pack.
    fn skill_path(&self, skill_name: &str) -> PathBuf {
        self.path.join(skill_name).with_extension("json")
    }

    /// Get the path to a skill config file within this pack.
    fn config_path(&self, skill_name: &str) -> PathBuf {
        self.path.join(skill_name).with_extension("yaml")
    }
}

#[async_trait]
impl SkillPack for FileSystemSkillPack {
    fn name(&self) -> &str {
        &self.name
    }

    async fn list_skills(&self) -> Result<Vec<String>, SkillsError> {
        let mut skills = Vec::new();
        let mut entries = fs::read_dir(&self.path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file()
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && path.extension().and_then(|e| e.to_str()) == Some("json")
            {
                skills.push(stem.to_string());
            }
        }

        Ok(skills)
    }

    async fn get_skill(&self, name: &str) -> Result<Option<SkillDefinition>, SkillsError> {
        let skill_path = self.skill_path(name);
        
        if !skill_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&skill_path).await?;
        let skill: SkillDefinition = serde_json::from_str(&content)?;
        
        Ok(Some(skill))
    }

    async fn get_config(&self, name: &str) -> Result<Option<SkillConfig>, SkillsError> {
        let config_path = self.config_path(name);
        
        if !config_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&config_path).await?;
        let config: SkillConfig = serde_yaml::from_str(&content)?;
        
        Ok(Some(config))
    }

    async fn get_state(&self, _name: &str) -> Result<Option<SkillState>, SkillsError> {
        // State is managed at runtime, not persisted in the pack
        Ok(None)
    }

    async fn set_mode(&self, _name: &str, _mode: SkillMode) -> Result<(), SkillsError> {
        // State changes would need to be persisted somewhere
        warn!("set_mode called on FileSystemSkillPack - state not persisted");
        Ok(())
    }

    async fn load_bundle(&self) -> Result<SkillBundle, SkillsError> {
        let skill_names = self.list_skills().await?;
        
        let mut skills = std::collections::HashMap::new();
        let mut configs = std::collections::HashMap::new();

        for name in &skill_names {
            if let Some(skill) = self.get_skill(name).await? {
                skills.insert(name.clone(), skill);
            }
            if let Some(config) = self.get_config(name).await? {
                configs.insert(name.clone(), config);
            }
        }

        let metadata = SkillPackMetadata {
            name: self.name.clone(),
            description: format!("Skill pack loaded from filesystem: {}", self.path.display()),
            version: "1.0.0".to_string(),
            skill_count: skills.len(),
        };

        Ok(SkillBundle::new(metadata, skills, configs))
    }
}
