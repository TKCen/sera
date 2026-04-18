//! Filesystem-backed [`SkillSource`].
//!
//! A [`FileSystemSource`] searches a priority-ordered list of directories for
//! markdown skill packs. Each directory is treated as a root containing
//! either top-level `*.md` skill files or nested pack subdirectories; we
//! support both shapes.
//!
//! Resolution uses first-wins semantics across the configured paths and
//! delegates the heavy lifting to [`MarkdownSkillPack`] — we only list
//! metadata (via `MarkdownSkillPack::list`) to locate the right pack before
//! loading the body.

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::fs;
use tracing::debug;

use crate::error::SkillsError;
use crate::markdown_pack::MarkdownSkillPack;
use crate::skill_pack::SkillPack;
use crate::skill_ref::{SkillRef, SkillSourceKind};
use crate::source::{ResolvedSkill, SkillSearchHit, SkillSource};

/// A `SkillSource` that reads markdown packs from a list of root directories.
///
/// Construction is cheap — no I/O is performed until `resolve` or `search`.
#[derive(Debug, Clone)]
pub struct FileSystemSource {
    paths: Vec<PathBuf>,
}

impl FileSystemSource {
    /// Create a source over the given priority-ordered paths. First match
    /// wins on name/version collisions.
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self { paths }
    }

    /// Borrow the configured paths.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Enumerate every markdown pack rooted under each configured path.
    ///
    /// A pack is either:
    ///   * the path itself (when it directly contains `*.md` files), or
    ///   * a subdirectory containing `*.md` files.
    async fn discover_packs(&self) -> Vec<MarkdownSkillPack> {
        let mut packs = Vec::new();
        for root in &self.paths {
            if !root.exists() {
                continue;
            }

            // Treat the root as a pack if it contains any `*.md` files directly.
            if has_markdown_child(root).await {
                let name = root
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                packs.push(MarkdownSkillPack::new(name, root.clone()));
            }

            // Also recurse one level: each subdirectory is a candidate pack.
            let Ok(mut entries) = fs::read_dir(root).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let p = entry.path();
                if p.is_dir() && has_markdown_child(&p).await {
                    let name = p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    packs.push(MarkdownSkillPack::new(name, p));
                }
            }
        }
        packs
    }
}

async fn has_markdown_child(dir: &std::path::Path) -> bool {
    let Ok(mut entries) = fs::read_dir(dir).await else {
        return false;
    };
    while let Ok(Some(e)) = entries.next_entry().await {
        let p = e.path();
        if p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("md") {
            return true;
        }
    }
    false
}

#[async_trait]
impl SkillSource for FileSystemSource {
    fn kind(&self) -> SkillSourceKind {
        SkillSourceKind::Fs
    }

    async fn resolve(&self, skill_ref: &SkillRef) -> Result<ResolvedSkill, SkillsError> {
        let packs = self.discover_packs().await;
        for pack in &packs {
            // Check the lightweight metadata first (progressive disclosure).
            let list = pack.list().await?;
            let Some(entry) = list.iter().find(|e| e.name == skill_ref.name) else {
                continue;
            };
            // If the caller constrained the version, the index entry MUST
            // carry a version that satisfies it.
            if let Some(ver) = entry.version.as_deref() {
                if !skill_ref.satisfied_by(ver) {
                    debug!(
                        name = %entry.name,
                        version = ver,
                        want = %skill_ref,
                        "skipping version-mismatched skill"
                    );
                    continue;
                }
            } else if skill_ref.version.is_some() {
                // No version in frontmatter but caller pinned — not a match.
                continue;
            }

            let Some(definition) = pack.get_skill(&skill_ref.name).await? else {
                continue;
            };

            return Ok(ResolvedSkill {
                reference: skill_ref.clone(),
                definition,
                pack_name: pack.name().to_string(),
                source: SkillSourceKind::Fs,
            });
        }

        Err(SkillsError::SkillNotFound(skill_ref.name.clone()))
    }

    async fn search(&self, query: &str) -> Result<Vec<SkillSearchHit>, SkillsError> {
        let q = query.to_lowercase();
        let mut out = Vec::new();
        for pack in self.discover_packs().await {
            let list = pack.list().await?;
            for entry in list {
                let desc = entry.description.clone().unwrap_or_default();
                let hay_name = entry.name.to_lowercase();
                let hay_desc = desc.to_lowercase();
                if q.is_empty() || hay_name.contains(&q) || hay_desc.contains(&q) {
                    out.push(SkillSearchHit {
                        name: entry.name,
                        version: entry.version.unwrap_or_default(),
                        description: desc,
                        source: SkillSourceKind::Fs,
                        pack_name: pack.name().to_string(),
                    });
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs as tfs;

    const SKILL_A: &str = "---\nname: triage\nversion: 1.0.0\ndescription: Route incident tickets\n---\nBody A.\n";
    const SKILL_A_V2: &str = "---\nname: triage\nversion: 2.0.0\ndescription: Route incident tickets v2\n---\nBody A v2.\n";
    const SKILL_B: &str = "---\nname: deploy\nversion: 0.3.0\ndescription: Deploy a service\n---\nBody B.\n";

    #[tokio::test]
    async fn resolve_matches_by_name() {
        let dir = tempdir().unwrap();
        tfs::write(dir.path().join("triage.md"), SKILL_A).await.unwrap();
        let src = FileSystemSource::new(vec![dir.path().to_path_buf()]);

        let r = SkillRef::parse("triage").unwrap();
        let resolved = src.resolve(&r).await.unwrap();
        assert_eq!(resolved.definition.name, "triage");
        assert_eq!(resolved.source, SkillSourceKind::Fs);
    }

    #[tokio::test]
    async fn resolve_respects_version_constraint() {
        let dir = tempdir().unwrap();
        tfs::write(dir.path().join("triage.md"), SKILL_A).await.unwrap();
        let src = FileSystemSource::new(vec![dir.path().to_path_buf()]);

        // ^1 matches 1.0.0
        let ok = SkillRef::parse("triage@^1").unwrap();
        assert!(src.resolve(&ok).await.is_ok());

        // ^2 does not
        let bad = SkillRef::parse("triage@^2").unwrap();
        assert!(matches!(
            src.resolve(&bad).await.unwrap_err(),
            SkillsError::SkillNotFound(_)
        ));
    }

    #[tokio::test]
    async fn resolve_searches_multiple_paths_in_order() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        tfs::write(dir_a.path().join("triage.md"), SKILL_A).await.unwrap();
        tfs::write(dir_b.path().join("triage.md"), SKILL_A_V2).await.unwrap();

        let src = FileSystemSource::new(vec![
            dir_a.path().to_path_buf(),
            dir_b.path().to_path_buf(),
        ]);
        let r = SkillRef::parse("triage").unwrap();
        let resolved = src.resolve(&r).await.unwrap();
        // First path wins — should see v1 body.
        assert!(resolved.definition.body.as_deref().unwrap().contains("Body A."));
    }

    #[tokio::test]
    async fn search_by_description_substring() {
        let dir = tempdir().unwrap();
        tfs::write(dir.path().join("triage.md"), SKILL_A).await.unwrap();
        tfs::write(dir.path().join("deploy.md"), SKILL_B).await.unwrap();
        let src = FileSystemSource::new(vec![dir.path().to_path_buf()]);

        let hits = src.search("incident").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "triage");

        let hits = src.search("deploy").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "deploy");

        let hits = src.search("").await.unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[tokio::test]
    async fn search_is_case_insensitive() {
        let dir = tempdir().unwrap();
        tfs::write(dir.path().join("triage.md"), SKILL_A).await.unwrap();
        let src = FileSystemSource::new(vec![dir.path().to_path_buf()]);
        let hits = src.search("INCIDENT").await.unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[tokio::test]
    async fn resolve_missing_skill_returns_not_found() {
        let dir = tempdir().unwrap();
        let src = FileSystemSource::new(vec![dir.path().to_path_buf()]);
        let r = SkillRef::parse("ghost").unwrap();
        assert!(matches!(
            src.resolve(&r).await.unwrap_err(),
            SkillsError::SkillNotFound(_)
        ));
    }

    // --- additional gap-filling tests ---

    #[tokio::test]
    async fn resolve_skill_in_subdirectory_pack() {
        // FileSystemSource also discovers skills nested one level deep as packs.
        let root = tempdir().unwrap();
        let pack_dir = root.path().join("infra-pack");
        tfs::create_dir_all(&pack_dir).await.unwrap();
        tfs::write(pack_dir.join("triage.md"), SKILL_A).await.unwrap();

        let src = FileSystemSource::new(vec![root.path().to_path_buf()]);
        let r = SkillRef::parse("triage").unwrap();
        let resolved = src.resolve(&r).await.unwrap();
        assert_eq!(resolved.definition.name, "triage");
    }

    #[tokio::test]
    async fn search_empty_query_returns_all_skills() {
        let dir = tempdir().unwrap();
        tfs::write(dir.path().join("triage.md"), SKILL_A).await.unwrap();
        tfs::write(dir.path().join("deploy.md"), SKILL_B).await.unwrap();
        let src = FileSystemSource::new(vec![dir.path().to_path_buf()]);
        let hits = src.search("").await.unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[tokio::test]
    async fn source_kind_is_fs() {
        let src = FileSystemSource::new(vec![]);
        assert_eq!(src.kind(), SkillSourceKind::Fs);
    }

    #[tokio::test]
    async fn paths_accessor_returns_configured_paths() {
        let dir = tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let src = FileSystemSource::new(vec![p.clone()]);
        assert_eq!(src.paths(), &[p]);
    }

    #[tokio::test]
    async fn resolve_exact_version_pin_matches() {
        let dir = tempdir().unwrap();
        tfs::write(dir.path().join("triage.md"), SKILL_A).await.unwrap();
        let src = FileSystemSource::new(vec![dir.path().to_path_buf()]);
        // =1.0.0 is an exact pin — must match 1.0.0.
        let r = SkillRef::parse("triage@=1.0.0").unwrap();
        let resolved = src.resolve(&r).await.unwrap();
        assert_eq!(resolved.definition.name, "triage");
    }

    #[tokio::test]
    async fn resolve_skill_without_frontmatter_version_fails_version_constraint() {
        // A skill file with no version field cannot satisfy any version constraint.
        let no_version_skill = "---\nname: triage\ndescription: no version\n---\nBody.\n";
        let dir = tempdir().unwrap();
        tfs::write(dir.path().join("triage.md"), no_version_skill).await.unwrap();
        let src = FileSystemSource::new(vec![dir.path().to_path_buf()]);
        let r = SkillRef::parse("triage@^1").unwrap();
        assert!(matches!(
            src.resolve(&r).await.unwrap_err(),
            SkillsError::SkillNotFound(_)
        ));
    }

    #[tokio::test]
    async fn search_no_match_returns_empty() {
        let dir = tempdir().unwrap();
        tfs::write(dir.path().join("triage.md"), SKILL_A).await.unwrap();
        let src = FileSystemSource::new(vec![dir.path().to_path_buf()]);
        let hits = src.search("zzzzzz-no-match").await.unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn search_hit_includes_pack_name_and_source() {
        let dir = tempdir().unwrap();
        tfs::write(dir.path().join("triage.md"), SKILL_A).await.unwrap();
        let src = FileSystemSource::new(vec![dir.path().to_path_buf()]);
        let hits = src.search("triage").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, SkillSourceKind::Fs);
        assert!(!hits[0].pack_name.is_empty());
    }
}
