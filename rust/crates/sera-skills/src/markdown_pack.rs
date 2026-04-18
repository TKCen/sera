//! `MarkdownSkillPack` — AgentSkills-compatible markdown skill pack.
//!
//! Reads single-file `.md` skill definitions from a directory. Implements
//! the [`SkillPack`] trait, so it is interchangeable with the legacy
//! [`FileSystemSkillPack`][crate::loader::FileSystemSkillPack] (two-file
//! JSON + YAML format).
//!
//! # Progressive disclosure
//!
//! `list()` / `list_skills()` return metadata (name, description, version)
//! without parsing skill bodies. `get_skill()` lazy-loads the full body for
//! a named skill. A `_index.yaml` fast-path, when present, lets the pack
//! answer metadata queries without touching any `*.md` files at all;
//! [`MarkdownSkillPack::regenerate_index`] writes a fresh index file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::debug;

use sera_types::skill::{SkillConfig, SkillDefinition, SkillMode, SkillState};

use crate::bundle::SkillBundle;
use crate::error::SkillsError;
use crate::markdown::{parse_skill_markdown_file, parse_skill_markdown_str};
use crate::skill_pack::{SkillPack, SkillPackMetadata};

/// Index file name inside a markdown skill pack directory.
pub const INDEX_FILE_NAME: &str = "_index.yaml";

/// Lightweight per-skill record stored in `_index.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexEntry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Full `_index.yaml` document schema.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackIndex {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub skills: Vec<IndexEntry>,
}

/// A markdown-backed skill pack. Cheap to construct — all I/O is deferred
/// until `list_skills`, `get_skill`, or `load_bundle` is called.
#[derive(Debug, Clone)]
pub struct MarkdownSkillPack {
    name: String,
    path: PathBuf,
    /// Incremented every time a `*.md` file is read from disk. Used by tests
    /// to verify the progressive-disclosure contract (metadata listing must
    /// NOT read skill bodies).
    read_counter: Arc<AtomicU64>,
}

impl MarkdownSkillPack {
    /// Create a markdown skill pack rooted at `path`. No I/O is performed.
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            read_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Root directory of this pack.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Test-only counter tracking how many `*.md` files have been read.
    /// Exposed publicly so integration tests can assert the progressive
    /// disclosure contract.
    pub fn read_count(&self) -> u64 {
        self.read_counter.load(Ordering::SeqCst)
    }

    fn skill_path(&self, name: &str) -> PathBuf {
        self.path.join(format!("{name}.md"))
    }

    fn index_path(&self) -> PathBuf {
        self.path.join(INDEX_FILE_NAME)
    }

    async fn read_index(&self) -> Result<Option<PackIndex>, SkillsError> {
        let idx_path = self.index_path();
        if !idx_path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&idx_path).await?;
        let idx: PackIndex = serde_yaml::from_str(&raw).map_err(|e| {
            SkillsError::Format(format!(
                "invalid _index.yaml at {}: {e}",
                idx_path.display()
            ))
        })?;
        Ok(Some(idx))
    }

    /// List skills as lightweight index entries. Uses `_index.yaml` when
    /// present, otherwise parses just the frontmatter of every `*.md` file.
    ///
    /// This is the **progressive disclosure** entry point — it MUST NOT
    /// materialise skill bodies.
    pub async fn list(&self) -> Result<Vec<IndexEntry>, SkillsError> {
        if let Some(idx) = self.read_index().await? {
            return Ok(idx.skills);
        }

        let mut out = Vec::new();
        if !self.path.exists() {
            return Ok(out);
        }
        let mut entries = fs::read_dir(&self.path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let p = entry.path();
            if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("md") {
                // Parse frontmatter + body. We do this even for "list" when no
                // index is present because we need the description/version.
                // Bodies are discarded; they are not returned to the caller.
                self.read_counter.fetch_add(1, Ordering::SeqCst);
                let raw = fs::read_to_string(&p).await?;
                let parsed = parse_skill_markdown_str(&raw, p.clone())?;
                out.push(IndexEntry {
                    name: parsed.definition.name,
                    description: parsed.definition.description,
                    version: parsed.definition.version,
                });
            }
        }
        Ok(out)
    }

    /// Regenerate `_index.yaml` by parsing every `*.md` file's frontmatter.
    pub async fn regenerate_index(&self) -> Result<(), SkillsError> {
        if !self.path.exists() {
            fs::create_dir_all(&self.path).await?;
        }

        let mut entries = fs::read_dir(&self.path).await?;
        let mut skills: Vec<IndexEntry> = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let p = entry.path();
            if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("md") {
                self.read_counter.fetch_add(1, Ordering::SeqCst);
                let raw = fs::read_to_string(&p).await?;
                let parsed = parse_skill_markdown_str(&raw, p.clone())?;
                skills.push(IndexEntry {
                    name: parsed.definition.name,
                    description: parsed.definition.description,
                    version: parsed.definition.version,
                });
            }
        }
        skills.sort_by(|a, b| a.name.cmp(&b.name));

        let index = PackIndex {
            name: Some(self.name.clone()),
            description: Some(format!("Markdown skill pack at {}", self.path.display())),
            version: Some("1.0.0".to_string()),
            skills,
        };
        let yaml = serde_yaml::to_string(&index).map_err(SkillsError::YamlParsing)?;
        fs::write(self.index_path(), yaml).await?;
        debug!(path = %self.index_path().display(), "regenerated markdown skill pack index");
        Ok(())
    }
}

#[async_trait]
impl SkillPack for MarkdownSkillPack {
    fn name(&self) -> &str {
        &self.name
    }

    async fn list_skills(&self) -> Result<Vec<String>, SkillsError> {
        let list = self.list().await?;
        Ok(list.into_iter().map(|e| e.name).collect())
    }

    async fn get_skill(&self, name: &str) -> Result<Option<SkillDefinition>, SkillsError> {
        let path = self.skill_path(name);
        if !path.exists() {
            return Ok(None);
        }
        self.read_counter.fetch_add(1, Ordering::SeqCst);
        let parsed = parse_skill_markdown_file(&path).await?;
        Ok(Some(parsed.definition))
    }

    async fn get_config(&self, name: &str) -> Result<Option<SkillConfig>, SkillsError> {
        let path = self.skill_path(name);
        if !path.exists() {
            return Ok(None);
        }
        self.read_counter.fetch_add(1, Ordering::SeqCst);
        let parsed = parse_skill_markdown_file(&path).await?;
        Ok(Some(parsed.config))
    }

    async fn get_state(&self, _name: &str) -> Result<Option<SkillState>, SkillsError> {
        Ok(None)
    }

    async fn set_mode(&self, _name: &str, _mode: SkillMode) -> Result<(), SkillsError> {
        // State lives in the runtime registry, not in the markdown pack.
        Ok(())
    }

    async fn load_bundle(&self) -> Result<SkillBundle, SkillsError> {
        let names = self.list_skills().await?;
        let mut skills = HashMap::new();
        let mut configs = HashMap::new();
        for name in &names {
            if let Some(def) = self.get_skill(name).await? {
                skills.insert(name.clone(), def);
            }
            if let Some(cfg) = self.get_config(name).await? {
                configs.insert(name.clone(), cfg);
            }
        }
        let metadata = SkillPackMetadata {
            name: self.name.clone(),
            description: format!("Markdown skill pack at {}", self.path.display()),
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

    const SAMPLE_SKILL: &str = r#"---
name: review-code
version: 1.0.0
description: Sample skill for tests
triggers: [review]
tools: [read_file]
---

Body contents for review-code.
"#;

    const SECOND_SKILL: &str = r#"---
name: deploy-app
version: 0.2.0
description: Second sample
---

Deploy body.
"#;

    #[tokio::test]
    async fn list_returns_metadata_without_index() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("review-code.md"), SAMPLE_SKILL)
            .await
            .unwrap();
        fs::write(dir.path().join("deploy-app.md"), SECOND_SKILL)
            .await
            .unwrap();

        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        let list = pack.list().await.unwrap();
        assert_eq!(list.len(), 2);
        let names: Vec<_> = list.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"review-code"));
        assert!(names.contains(&"deploy-app"));
    }

    #[tokio::test]
    async fn get_skill_returns_full_body() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("review-code.md"), SAMPLE_SKILL)
            .await
            .unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        let def = pack.get_skill("review-code").await.unwrap().unwrap();
        assert!(def.body.as_deref().unwrap().contains("Body contents"));
        assert_eq!(def.tool_bindings, vec!["read_file"]);
    }

    #[tokio::test]
    async fn get_skill_missing_returns_none() {
        let dir = tempdir().unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        assert!(pack.get_skill("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn index_roundtrip_skips_body_reads() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("review-code.md"), SAMPLE_SKILL)
            .await
            .unwrap();
        fs::write(dir.path().join("deploy-app.md"), SECOND_SKILL)
            .await
            .unwrap();

        // First pack: regenerate the index (this reads all bodies).
        let writer = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        writer.regenerate_index().await.unwrap();
        let writer_reads = writer.read_count();
        assert_eq!(writer_reads, 2, "regenerate_index should read every .md");

        // Second pack reads from the index only — no body reads.
        let reader = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        let list = reader.list().await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(
            reader.read_count(),
            0,
            "list() with _index.yaml must not read any .md bodies"
        );

        // Verify the index file is present and parseable.
        let idx_path = dir.path().join(INDEX_FILE_NAME);
        assert!(idx_path.exists());
        let raw = fs::read_to_string(&idx_path).await.unwrap();
        let idx: PackIndex = serde_yaml::from_str(&raw).unwrap();
        assert_eq!(idx.skills.len(), 2);
    }

    #[tokio::test]
    async fn load_bundle_populates_skills_and_configs() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("review-code.md"), SAMPLE_SKILL)
            .await
            .unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        let bundle = pack.load_bundle().await.unwrap();
        assert_eq!(bundle.metadata.skill_count, 1);
        assert!(bundle.get("review-code").is_some());
        assert!(bundle.configs.contains_key("review-code"));
    }

    #[tokio::test]
    async fn fixture_code_review_parses_end_to_end() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("skills")
            .join("markdown_pack");
        let pack = MarkdownSkillPack::new("fixture", fixtures);
        let list = pack.list().await.unwrap();
        assert!(list.iter().any(|e| e.name == "code-review"));
        let def = pack.get_skill("code-review").await.unwrap().unwrap();
        assert_eq!(def.version.as_deref(), Some("1.0.0"));
        assert_eq!(def.mcp_servers.len(), 1);
        assert_eq!(def.mcp_servers[0].name, "github");
        assert!(def.body.as_deref().unwrap().contains("senior code reviewer"));
    }

    #[tokio::test]
    async fn list_without_index_reads_all_md_files() {
        // Counter should reflect body reads when no index is present.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("review-code.md"), SAMPLE_SKILL)
            .await
            .unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        let _ = pack.list().await.unwrap();
        assert_eq!(pack.read_count(), 1);
    }

    // --- additional gap-filling tests ---

    #[tokio::test]
    async fn list_empty_dir_returns_empty_vec() {
        let dir = tempdir().unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        let list = pack.list().await.unwrap();
        assert!(list.is_empty());
        assert_eq!(pack.read_count(), 0);
    }

    #[tokio::test]
    async fn list_skips_non_md_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("readme.txt"), "not a skill").await.unwrap();
        fs::write(dir.path().join("review-code.md"), SAMPLE_SKILL).await.unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        let list = pack.list().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "review-code");
    }

    #[tokio::test]
    async fn corrupted_md_returns_error_not_panic() {
        let dir = tempdir().unwrap();
        // Malformed: no frontmatter fence at all.
        fs::write(dir.path().join("bad.md"), "this is not a skill file").await.unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        let result = pack.list().await;
        assert!(result.is_err(), "corrupted .md should yield an error");
    }

    #[tokio::test]
    async fn get_config_returns_none_for_missing_skill() {
        let dir = tempdir().unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        assert!(pack.get_config("nonexistent").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn get_config_returns_config_for_existing_skill() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("review-code.md"), SAMPLE_SKILL).await.unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        let config = pack.get_config("review-code").await.unwrap();
        assert!(config.is_some());
        let cfg = config.unwrap();
        assert_eq!(cfg.name, "review-code");
        assert_eq!(cfg.version, "1.0.0");
    }

    #[tokio::test]
    async fn get_state_always_returns_none() {
        let dir = tempdir().unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        assert!(pack.get_state("anything").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn set_mode_is_noop_and_succeeds() {
        use sera_types::skill::SkillMode;
        let dir = tempdir().unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        // set_mode on a markdown pack must not error.
        pack.set_mode("anything", SkillMode::OnDemand).await.unwrap();
    }

    #[tokio::test]
    async fn index_entry_preserves_version_and_description() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("review-code.md"), SAMPLE_SKILL).await.unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        let list = pack.list().await.unwrap();
        let entry = &list[0];
        assert_eq!(entry.version.as_deref(), Some("1.0.0"));
        assert_eq!(entry.description.as_deref(), Some("Sample skill for tests"));
    }

    #[tokio::test]
    async fn list_skills_returns_only_names() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("review-code.md"), SAMPLE_SKILL).await.unwrap();
        fs::write(dir.path().join("deploy-app.md"), SECOND_SKILL).await.unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        let mut names = pack.list_skills().await.unwrap();
        names.sort();
        assert_eq!(names, vec!["deploy-app", "review-code"]);
    }

    #[tokio::test]
    async fn name_method_returns_pack_name() {
        let dir = tempdir().unwrap();
        let pack = MarkdownSkillPack::new("my-pack", dir.path().to_path_buf());
        assert_eq!(pack.name(), "my-pack");
    }

    #[tokio::test]
    async fn path_method_returns_root() {
        let dir = tempdir().unwrap();
        let pack = MarkdownSkillPack::new("unit", dir.path().to_path_buf());
        assert_eq!(pack.path(), dir.path());
    }
}
