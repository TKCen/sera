//! Skill bundle — a loaded collection of skills from a pack.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use sera_types::skill::{SkillDefinition, SkillConfig, SkillState};
use crate::skill_pack::SkillPackMetadata;

/// A bundle of skills loaded from a skill pack.
///
/// Contains all the skills, their configurations, and current states.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillBundle {
    /// Metadata about the source pack.
    pub metadata: SkillPackMetadata,
    /// All skill definitions in the pack.
    pub skills: HashMap<String, SkillDefinition>,
    /// Runtime configurations for each skill.
    pub configs: HashMap<String, SkillConfig>,
    /// Current states of skills (empty if not yet activated).
    #[serde(default)]
    pub states: HashMap<String, SkillState>,
}

impl SkillBundle {
    /// Create a new skill bundle from components.
    pub fn new(
        metadata: SkillPackMetadata,
        skills: HashMap<String, SkillDefinition>,
        configs: HashMap<String, SkillConfig>,
    ) -> Self {
        Self {
            metadata,
            skills,
            configs,
            states: HashMap::new(),
        }
    }

    /// Get a skill definition by name.
    pub fn get(&self, name: &str) -> Option<&SkillDefinition> {
        self.skills.get(name)
    }

    /// Get all skill names.
    pub fn skill_names(&self) -> Vec<&str> {
        self.skills.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a skill exists in this bundle.
    pub fn contains(&self, name: &str) -> bool {
        self.skills.contains_key(name)
    }

    /// Get the number of skills in the bundle.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Check if the bundle is empty.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}
