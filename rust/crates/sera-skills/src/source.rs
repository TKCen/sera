//! [`SkillSource`] — a trait for anything that can resolve or search skills.
//!
//! Implementations live under [`crate::sources`] (`FileSystemSource`,
//! `PluginSource`, `RegistrySource`). A [`SkillResolver`][crate::resolver::SkillResolver]
//! fans out across a list of `SkillSource`s in priority order.

use async_trait::async_trait;

use sera_types::skill::SkillDefinition;

use crate::error::SkillsError;
use crate::skill_ref::{SkillRef, SkillSourceKind};

/// A source that can resolve a `SkillRef` to a full `SkillDefinition` and
/// perform free-text search over its catalog.
#[async_trait]
pub trait SkillSource: Send + Sync + 'static {
    /// What kind of source this is (used for logging and result tagging).
    fn kind(&self) -> SkillSourceKind;

    /// Resolve a concrete skill. On success, returns the skill definition
    /// plus provenance metadata.
    async fn resolve(&self, skill_ref: &SkillRef) -> Result<ResolvedSkill, SkillsError>;

    /// Search for skills matching the free-text query. Must never panic;
    /// return `Ok(vec![])` when nothing matches.
    async fn search(&self, query: &str) -> Result<Vec<SkillSearchHit>, SkillsError>;
}

/// A skill resolved by a source — ready to be loaded into an agent's
/// context.
#[derive(Debug, Clone)]
pub struct ResolvedSkill {
    /// The reference that triggered the resolution.
    pub reference: SkillRef,
    /// Full skill definition (with `body` populated).
    pub definition: SkillDefinition,
    /// Human-facing pack name (filesystem dir, plugin name, OCI path).
    pub pack_name: String,
    /// Which source resolved this skill.
    pub source: SkillSourceKind,
}

/// Lightweight record returned by [`SkillSource::search`].
#[derive(Debug, Clone)]
pub struct SkillSearchHit {
    pub name: String,
    pub version: String,
    pub description: String,
    pub source: SkillSourceKind,
    pub pack_name: String,
}
