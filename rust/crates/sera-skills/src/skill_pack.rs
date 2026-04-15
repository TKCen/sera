//! Skill pack trait and implementations.
//!
//! A [`SkillPack`] is a collection of skills that can be loaded and discovered
//! by agents. The trait allows for different storage backends (filesystem,
//! database, remote API, etc.).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::SkillsError;
use crate::bundle::SkillBundle;
use sera_types::skill::{SkillDefinition, SkillConfig, SkillState, SkillMode};

/// A skill pack that provides skill definitions and configurations.
///
/// Implement this trait to create a skill pack backed by any storage system.
/// The pack provides:
///
/// - Skill definitions (metadata about each skill)
/// - Skill configurations (runtime settings)
/// - Skill states (current state of active skills)
#[async_trait]
pub trait SkillPack: Send + Sync {
    /// Get the name of this skill pack.
    fn name(&self) -> &str;

    /// List all skill names available in this pack.
    async fn list_skills(&self) -> Result<Vec<String>, SkillsError>;

    /// Get a specific skill's definition.
    async fn get_skill(&self, name: &str) -> Result<Option<SkillDefinition>, SkillsError>;

    /// Get a specific skill's configuration.
    async fn get_config(&self, name: &str) -> Result<Option<SkillConfig>, SkillsError>;

    /// Get the current state of a skill.
    async fn get_state(&self, name: &str) -> Result<Option<SkillState>, SkillsError>;

    /// Update a skill's mode.
    async fn set_mode(&self, name: &str, mode: SkillMode) -> Result<(), SkillsError>;

    /// Load the entire pack as a bundle.
    async fn load_bundle(&self) -> Result<SkillBundle, SkillsError>;
}

/// Metadata about a skill pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPackMetadata {
    /// Unique name of the skill pack.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Version of the skill pack format.
    pub version: String,
    /// Number of skills in the pack.
    pub skill_count: usize,
}
