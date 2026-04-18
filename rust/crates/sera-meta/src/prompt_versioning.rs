//! Versioned system prompt section editing with rollback support.
//!
//! Provides a per-agent, per-section version history for system prompt
//! content, with activation modes and history-preserving rollback.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Sections of an agent's system prompt that can be independently versioned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSection {
    Role,
    Principles,
    CommunicationStyle,
    ToolGuidelines,
    CustomInstructions,
}

/// Activation mode for prompt changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationMode {
    /// Activate immediately upon proposal.
    Auto,
    /// Store but require explicit activation.
    Review,
}

/// A versioned snapshot of a prompt section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptVersion {
    pub agent_id: String,
    pub section: PromptSection,
    pub version: u32,
    pub content: String,
    pub rationale: String,
    pub activation: ActivationMode,
    pub active: bool,
    pub created_at: SystemTime,
}

/// Maximum allowed byte length for a prompt section's content.
pub const MAX_SECTION_LENGTH: usize = 4000;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum PromptVersionError {
    #[error("version {version} not found for {agent_id}/{section:?}")]
    VersionNotFound {
        agent_id: String,
        section: PromptSection,
        version: u32,
    },
    #[error("content exceeds maximum length of {max} characters")]
    ContentTooLong { max: usize },
    #[error("rationale is required")]
    RationaleRequired,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Storage backend for versioned prompt sections.
pub trait PromptVersionStore: Send + Sync {
    /// Return the currently active version for the given agent and section.
    fn get_active(&self, agent_id: &str, section: PromptSection) -> Option<&PromptVersion>;

    /// Return a specific version.
    fn get_version(
        &self,
        agent_id: &str,
        section: PromptSection,
        version: u32,
    ) -> Option<&PromptVersion>;

    /// List all versions in order for the given agent and section.
    fn list_versions(&self, agent_id: &str, section: PromptSection) -> Vec<&PromptVersion>;

    /// Propose a new version. Returns the new version number.
    ///
    /// With `ActivationMode::Auto` the new version becomes active immediately
    /// and any previous active version is deactivated.
    fn propose(
        &mut self,
        agent_id: &str,
        section: PromptSection,
        content: String,
        rationale: String,
        activation: ActivationMode,
    ) -> Result<u32, PromptVersionError>;

    /// Explicitly activate a stored version (required for `Review` mode).
    fn activate(
        &mut self,
        agent_id: &str,
        section: PromptSection,
        version: u32,
    ) -> Result<(), PromptVersionError>;

    /// Create a new version that copies the content of `target_version` and
    /// activates it. History is never rewritten.
    fn rollback(
        &mut self,
        agent_id: &str,
        section: PromptSection,
        target_version: u32,
    ) -> Result<(), PromptVersionError>;

    /// Return a map of section → content for all currently active versions of
    /// the given agent.
    fn get_overrides(&self, agent_id: &str) -> HashMap<PromptSection, String>;
}

// ---------------------------------------------------------------------------
// In-memory implementation
// ---------------------------------------------------------------------------

/// Key into the internal map: `(agent_id, section)`.
type StoreKey = (String, PromptSection);

/// Simple in-memory store. Not persistent across process restarts.
#[derive(Debug, Default)]
pub struct InMemoryPromptVersionStore {
    versions: HashMap<StoreKey, Vec<PromptVersion>>,
}

impl InMemoryPromptVersionStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn key(agent_id: &str, section: PromptSection) -> StoreKey {
        (agent_id.to_owned(), section)
    }

    /// Deactivate all versions for the given key.
    fn deactivate_all(versions: &mut [PromptVersion]) {
        for v in versions.iter_mut() {
            v.active = false;
        }
    }
}

impl PromptVersionStore for InMemoryPromptVersionStore {
    fn get_active(&self, agent_id: &str, section: PromptSection) -> Option<&PromptVersion> {
        self.versions
            .get(&Self::key(agent_id, section))
            .and_then(|vs| vs.iter().find(|v| v.active))
    }

    fn get_version(
        &self,
        agent_id: &str,
        section: PromptSection,
        version: u32,
    ) -> Option<&PromptVersion> {
        self.versions
            .get(&Self::key(agent_id, section))
            .and_then(|vs| vs.iter().find(|v| v.version == version))
    }

    fn list_versions(&self, agent_id: &str, section: PromptSection) -> Vec<&PromptVersion> {
        self.versions
            .get(&Self::key(agent_id, section))
            .map(|vs| vs.iter().collect())
            .unwrap_or_default()
    }

    fn propose(
        &mut self,
        agent_id: &str,
        section: PromptSection,
        content: String,
        rationale: String,
        activation: ActivationMode,
    ) -> Result<u32, PromptVersionError> {
        if content.len() > MAX_SECTION_LENGTH {
            return Err(PromptVersionError::ContentTooLong {
                max: MAX_SECTION_LENGTH,
            });
        }
        if rationale.trim().is_empty() {
            return Err(PromptVersionError::RationaleRequired);
        }

        let key = Self::key(agent_id, section);
        let versions = self.versions.entry(key).or_default();

        let next_version = versions
            .last()
            .map(|v| v.version + 1)
            .unwrap_or(1);

        let active = matches!(activation, ActivationMode::Auto);

        if active {
            Self::deactivate_all(versions);
        }

        versions.push(PromptVersion {
            agent_id: agent_id.to_owned(),
            section,
            version: next_version,
            content,
            rationale,
            activation,
            active,
            created_at: SystemTime::now(),
        });

        Ok(next_version)
    }

    fn activate(
        &mut self,
        agent_id: &str,
        section: PromptSection,
        version: u32,
    ) -> Result<(), PromptVersionError> {
        let key = Self::key(agent_id, section);
        let versions = self
            .versions
            .get_mut(&key)
            .ok_or_else(|| PromptVersionError::VersionNotFound {
                agent_id: agent_id.to_owned(),
                section,
                version,
            })?;

        // Validate the target exists before mutating.
        if !versions.iter().any(|v| v.version == version) {
            return Err(PromptVersionError::VersionNotFound {
                agent_id: agent_id.to_owned(),
                section,
                version,
            });
        }

        Self::deactivate_all(versions);
        if let Some(v) = versions.iter_mut().find(|v| v.version == version) {
            v.active = true;
        }
        Ok(())
    }

    fn rollback(
        &mut self,
        agent_id: &str,
        section: PromptSection,
        target_version: u32,
    ) -> Result<(), PromptVersionError> {
        let key = Self::key(agent_id, section);

        // Borrow to extract the content we want to restore, then drop borrow.
        let (content, rationale) = {
            let versions = self
                .versions
                .get(&key)
                .ok_or_else(|| PromptVersionError::VersionNotFound {
                    agent_id: agent_id.to_owned(),
                    section,
                    version: target_version,
                })?;

            let target = versions
                .iter()
                .find(|v| v.version == target_version)
                .ok_or_else(|| PromptVersionError::VersionNotFound {
                    agent_id: agent_id.to_owned(),
                    section,
                    version: target_version,
                })?;

            (
                target.content.clone(),
                format!("rollback to v{target_version}"),
            )
        };

        // propose() enforces validation and appends the new version.
        self.propose(agent_id, section, content, rationale, ActivationMode::Auto)?;
        Ok(())
    }

    fn get_overrides(&self, agent_id: &str) -> HashMap<PromptSection, String> {
        let mut result = HashMap::new();
        for ((aid, section), versions) in &self.versions {
            if aid == agent_id
                && let Some(active) = versions.iter().find(|v| v.active)
            {
                result.insert(*section, active.content.clone());
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> InMemoryPromptVersionStore {
        InMemoryPromptVersionStore::new()
    }

    #[test]
    fn propose_auto_becomes_active_immediately() {
        let mut s = store();
        let v = s
            .propose("agent-1", PromptSection::Role, "Hello".into(), "init".into(), ActivationMode::Auto)
            .unwrap();
        assert_eq!(v, 1);
        let active = s.get_active("agent-1", PromptSection::Role).unwrap();
        assert!(active.active);
        assert_eq!(active.content, "Hello");
    }

    #[test]
    fn propose_review_not_active() {
        let mut s = store();
        let v = s
            .propose("agent-1", PromptSection::Role, "Draft".into(), "wip".into(), ActivationMode::Review)
            .unwrap();
        assert_eq!(v, 1);
        assert!(s.get_active("agent-1", PromptSection::Role).is_none());
    }

    #[test]
    fn activate_makes_review_version_active() {
        let mut s = store();
        s.propose("agent-1", PromptSection::Role, "Draft".into(), "wip".into(), ActivationMode::Review)
            .unwrap();
        s.activate("agent-1", PromptSection::Role, 1).unwrap();
        let active = s.get_active("agent-1", PromptSection::Role).unwrap();
        assert_eq!(active.version, 1);
        assert!(active.active);
    }

    #[test]
    fn rollback_creates_new_version_with_old_content() {
        let mut s = store();
        s.propose("agent-1", PromptSection::Role, "v1 content".into(), "init".into(), ActivationMode::Auto)
            .unwrap();
        s.propose("agent-1", PromptSection::Role, "v2 content".into(), "update".into(), ActivationMode::Auto)
            .unwrap();

        s.rollback("agent-1", PromptSection::Role, 1).unwrap();

        let versions = s.list_versions("agent-1", PromptSection::Role);
        assert_eq!(versions.len(), 3, "rollback should create a third version");

        let active = s.get_active("agent-1", PromptSection::Role).unwrap();
        assert_eq!(active.version, 3);
        assert_eq!(active.content, "v1 content");

        // History is intact
        assert_eq!(versions[0].content, "v1 content");
        assert_eq!(versions[1].content, "v2 content");
    }

    #[test]
    fn get_overrides_returns_only_active_sections() {
        let mut s = store();
        s.propose("agent-1", PromptSection::Role, "My role".into(), "init".into(), ActivationMode::Auto)
            .unwrap();
        s.propose("agent-1", PromptSection::Principles, "Draft principles".into(), "wip".into(), ActivationMode::Review)
            .unwrap();

        let overrides = s.get_overrides("agent-1");
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[&PromptSection::Role], "My role");
        assert!(!overrides.contains_key(&PromptSection::Principles));
    }

    #[test]
    fn content_too_long_rejected() {
        let mut s = store();
        let long = "x".repeat(MAX_SECTION_LENGTH + 1);
        let err = s
            .propose("agent-1", PromptSection::Role, long, "reason".into(), ActivationMode::Auto)
            .unwrap_err();
        assert!(matches!(err, PromptVersionError::ContentTooLong { .. }));
    }

    #[test]
    fn empty_rationale_rejected() {
        let mut s = store();
        let err = s
            .propose("agent-1", PromptSection::Role, "content".into(), "   ".into(), ActivationMode::Auto)
            .unwrap_err();
        assert!(matches!(err, PromptVersionError::RationaleRequired));
    }

    #[test]
    fn version_numbers_are_sequential_per_agent_section() {
        let mut s = store();
        for i in 1..=5u32 {
            let v = s
                .propose("agent-1", PromptSection::Role, format!("v{i}"), "r".into(), ActivationMode::Review)
                .unwrap();
            assert_eq!(v, i);
        }
    }

    #[test]
    fn multiple_agents_do_not_interfere() {
        let mut s = store();
        s.propose("agent-a", PromptSection::Role, "A role".into(), "init".into(), ActivationMode::Auto)
            .unwrap();
        s.propose("agent-b", PromptSection::Role, "B role".into(), "init".into(), ActivationMode::Auto)
            .unwrap();

        let a = s.get_active("agent-a", PromptSection::Role).unwrap();
        let b = s.get_active("agent-b", PromptSection::Role).unwrap();
        assert_eq!(a.content, "A role");
        assert_eq!(b.content, "B role");
        assert_eq!(a.version, 1);
        assert_eq!(b.version, 1);
    }

    #[test]
    fn activate_nonexistent_version_returns_error() {
        let mut s = store();
        s.propose("agent-1", PromptSection::Role, "content".into(), "r".into(), ActivationMode::Review)
            .unwrap();
        let err = s.activate("agent-1", PromptSection::Role, 99).unwrap_err();
        assert!(matches!(err, PromptVersionError::VersionNotFound { version: 99, .. }));
    }
}
