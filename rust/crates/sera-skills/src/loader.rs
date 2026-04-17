//! Skill pack loader for discovering and loading skill packs.
//!
//! The [`SkillLoader`] provides a way to discover and load skill packs from
//! various sources (filesystem, embedded, etc.).
//!
//! # Multi-path discovery
//!
//! `SkillLoader::new(paths)` takes a priority-ordered list of directories.
//! When a pack name collides across paths, the **first path wins** and a
//! `DEBUG` log is emitted for the loser.
//!
//! A `with_legacy_fallback` constructor exposes both markdown and legacy
//! JSON+YAML packs so new- and old-format skills coexist during migration.

use std::path::PathBuf;
use async_trait::async_trait;
use tokio::fs;
use tracing::{debug, warn};

use crate::error::SkillsError;
use crate::markdown_pack::MarkdownSkillPack;
use crate::skill_pack::{SkillPack, SkillPackMetadata};
use crate::bundle::SkillBundle;
use sera_types::skill::{SkillDefinition, SkillConfig, SkillState, SkillMode};

/// A skill loader that loads packs from the filesystem.
///
/// The loader holds a priority-ordered list of **markdown pack roots** and an
/// optional list of **legacy pack roots** (two-file JSON + YAML format).
/// Resolution: markdown paths are searched first in order, then legacy paths.
#[derive(Debug, Clone)]
pub struct SkillLoader {
    markdown_paths: Vec<PathBuf>,
    legacy_paths: Vec<PathBuf>,
}

impl SkillLoader {
    /// Create a new skill loader with a priority-ordered list of markdown
    /// pack directories. First path wins on name collision.
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            markdown_paths: paths,
            legacy_paths: Vec::new(),
        }
    }

    /// Create a loader that also searches legacy two-file packs after the
    /// markdown paths have been exhausted. Useful during migration.
    pub fn with_legacy_fallback(paths: Vec<PathBuf>, legacy_paths: Vec<PathBuf>) -> Self {
        Self {
            markdown_paths: paths,
            legacy_paths,
        }
    }

    /// Load a markdown skill pack by name. Searches markdown paths in
    /// priority order; returns `NotFound` if no path contains it.
    pub async fn load(&self, name: &str) -> Result<MarkdownSkillPack, SkillsError> {
        let mut seen_elsewhere: Option<PathBuf> = None;
        for base in &self.markdown_paths {
            let pack_path = base.join(name);
            if pack_path.exists() {
                if let Some(loser) = seen_elsewhere {
                    debug!(
                        winner = %pack_path.display(),
                        loser = %loser.display(),
                        "skill pack name collision; earlier path wins"
                    );
                }
                debug!(path = %pack_path.display(), "loading markdown skill pack");
                // Double-check for later colliders so we can log them too.
                for other in self.markdown_paths.iter().skip_while(|p| *p != base).skip(1) {
                    let alt = other.join(name);
                    if alt.exists() {
                        debug!(
                            winner = %pack_path.display(),
                            loser = %alt.display(),
                            "skill pack name collision; earlier path wins"
                        );
                    }
                }
                return Ok(MarkdownSkillPack::new(name.to_string(), pack_path));
            }
            seen_elsewhere = Some(pack_path);
        }
        Err(SkillsError::NotFound(name.to_string()))
    }

    /// Load a legacy (two-file) skill pack by name. Only searches the
    /// legacy path list provided via [`SkillLoader::with_legacy_fallback`].
    pub async fn load_legacy(&self, name: &str) -> Result<FileSystemSkillPack, SkillsError> {
        for base in &self.legacy_paths {
            let pack_path = base.join(name);
            if pack_path.exists() {
                debug!(path = %pack_path.display(), "loading legacy skill pack from filesystem");
                return Ok(FileSystemSkillPack::new(name.to_string(), pack_path));
            }
        }
        Err(SkillsError::NotFound(name.to_string()))
    }

    /// List all available markdown skill packs across every configured path.
    /// Deduplicates by name using first-wins semantics.
    pub async fn list_packs(&self) -> Result<Vec<String>, SkillsError> {
        let mut packs = Vec::<String>::new();
        for base in &self.markdown_paths {
            if !base.exists() {
                continue;
            }
            let mut entries = fs::read_dir(base).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_dir()
                    && let Some(name) = path.file_name().and_then(|n| n.to_str())
                {
                    if packs.iter().any(|p| p == name) {
                        debug!(
                            pack = name,
                            shadowed_by_path = %path.display(),
                            "skill pack name collision; earlier path wins"
                        );
                        continue;
                    }
                    packs.push(name.to_string());
                }
            }
        }
        Ok(packs)
    }

    /// Markdown pack search paths in priority order.
    pub fn markdown_paths(&self) -> &[PathBuf] {
        &self.markdown_paths
    }

    /// Legacy pack search paths (after markdown).
    pub fn legacy_paths(&self) -> &[PathBuf] {
        &self.legacy_paths
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs;

    const MARKDOWN_SKILL: &str = "---\nname: review-code\nversion: 1.0.0\n---\n\nbody\n";

    #[tokio::test]
    async fn multi_path_first_wins_on_collision() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();

        // Both dirs contain a "core" pack with a review-code.md file.
        for dir in [&dir_a, &dir_b] {
            let pack = dir.path().join("core");
            fs::create_dir_all(&pack).await.unwrap();
            fs::write(pack.join("review-code.md"), MARKDOWN_SKILL)
                .await
                .unwrap();
        }

        let loader = SkillLoader::new(vec![
            dir_a.path().to_path_buf(),
            dir_b.path().to_path_buf(),
        ]);
        let pack = loader.load("core").await.unwrap();
        assert_eq!(pack.path(), dir_a.path().join("core"));
    }

    #[tokio::test]
    async fn load_returns_not_found_when_absent() {
        let dir = tempdir().unwrap();
        let loader = SkillLoader::new(vec![dir.path().to_path_buf()]);
        let err = loader.load("nonexistent").await.unwrap_err();
        assert!(matches!(err, SkillsError::NotFound(_)));
    }

    #[tokio::test]
    async fn list_packs_deduplicates_across_paths() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();

        fs::create_dir_all(dir_a.path().join("core")).await.unwrap();
        fs::create_dir_all(dir_b.path().join("core")).await.unwrap();
        fs::create_dir_all(dir_b.path().join("extras")).await.unwrap();

        let loader = SkillLoader::new(vec![
            dir_a.path().to_path_buf(),
            dir_b.path().to_path_buf(),
        ]);
        let mut packs = loader.list_packs().await.unwrap();
        packs.sort();
        assert_eq!(packs, vec!["core".to_string(), "extras".to_string()]);
    }

    #[tokio::test]
    async fn legacy_fallback_still_reachable() {
        let md_dir = tempdir().unwrap();
        let legacy_dir = tempdir().unwrap();
        fs::create_dir_all(legacy_dir.path().join("old-pack")).await.unwrap();

        let loader = SkillLoader::with_legacy_fallback(
            vec![md_dir.path().to_path_buf()],
            vec![legacy_dir.path().to_path_buf()],
        );
        assert!(loader.load("old-pack").await.is_err()); // markdown path has nothing
        let legacy = loader.load_legacy("old-pack").await.unwrap();
        assert_eq!(legacy.name(), "old-pack");
    }

    // --- additional gap-filling tests ---

    #[tokio::test]
    async fn load_legacy_missing_returns_not_found() {
        let md_dir = tempdir().unwrap();
        let legacy_dir = tempdir().unwrap();
        let loader = SkillLoader::with_legacy_fallback(
            vec![md_dir.path().to_path_buf()],
            vec![legacy_dir.path().to_path_buf()],
        );
        let err = loader.load_legacy("ghost").await.unwrap_err();
        assert!(matches!(err, SkillsError::NotFound(_)));
    }

    #[tokio::test]
    async fn list_packs_nonexistent_paths_skipped() {
        // Paths that don't exist should not cause an error — they are silently skipped.
        let loader = SkillLoader::new(vec![
            PathBuf::from("/tmp/__sera_nonexistent_dir_12345__"),
        ]);
        let packs = loader.list_packs().await.unwrap();
        assert!(packs.is_empty());
    }

    #[tokio::test]
    async fn list_packs_returns_empty_for_empty_dir() {
        let dir = tempdir().unwrap();
        let loader = SkillLoader::new(vec![dir.path().to_path_buf()]);
        let packs = loader.list_packs().await.unwrap();
        assert!(packs.is_empty());
    }

    #[tokio::test]
    async fn markdown_paths_accessor_returns_configured_paths() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let loader = SkillLoader::new(vec![path.clone()]);
        assert_eq!(loader.markdown_paths(), &[path]);
        assert!(loader.legacy_paths().is_empty());
    }

    #[tokio::test]
    async fn legacy_paths_accessor_returns_configured_paths() {
        let md_dir = tempdir().unwrap();
        let legacy_dir = tempdir().unwrap();
        let loader = SkillLoader::with_legacy_fallback(
            vec![md_dir.path().to_path_buf()],
            vec![legacy_dir.path().to_path_buf()],
        );
        assert_eq!(loader.legacy_paths(), &[legacy_dir.path().to_path_buf()]);
    }

    #[tokio::test]
    async fn list_packs_ignores_non_directory_entries() {
        let dir = tempdir().unwrap();
        // Write a plain file — it should NOT appear as a pack.
        fs::write(dir.path().join("not-a-pack.txt"), "content").await.unwrap();
        // Write a directory — it SHOULD appear.
        fs::create_dir_all(dir.path().join("real-pack")).await.unwrap();
        let loader = SkillLoader::new(vec![dir.path().to_path_buf()]);
        let packs = loader.list_packs().await.unwrap();
        assert_eq!(packs, vec!["real-pack".to_string()]);
    }
}
