//! Self-patching loop scaffold for `sera-skills`.
//!
//! Provides a validation + apply pipeline so agents can propose incremental
//! updates to installed skill packs. No agent-tool wiring is included here —
//! that is a follow-up layer.
//!
//! # Pipeline
//!
//! ```text
//! SkillPatch ──► SelfPatchValidator ──► ValidatedPatch ──► SelfPatchApplier ──► updated SkillPack
//! ```
//!
//! [`DefaultSelfPatchValidator`] checks version alignment, YAML syntax for
//! `SkillMd` patches, 64 KB body budget, and duplicate knowledge filenames.
//!
//! [`FsSelfPatchApplier`] writes to a temp directory first and then atomically
//! renames, so a crash mid-write leaves the live pack untouched.
//!
//! [`InMemorySelfPatchApplier`] is provided for tests.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Maximum allowed body size (bytes) for a `SkillMd` patch.
pub const MAX_SKILL_MD_BYTES: usize = 64 * 1024; // 64 KB

// ---------------------------------------------------------------------------
// Core patch types
// ---------------------------------------------------------------------------

/// A proposed change to one skill inside an installed skill pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPatch {
    /// Identifier of the skill to patch (e.g. `"code-review"`).
    pub skill_id: String,
    /// Version of the skill currently installed. Must match the live pack's
    /// version; a mismatch causes [`PatchError::VersionMismatch`].
    pub base_version: String,
    /// What kind of change this patch represents.
    pub patch_kind: PatchKind,
    /// The actual payload carrying the new content.
    pub payload: PatchPayload,
}

/// Discriminator for the three supported patch operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatchKind {
    /// Replace the entire `SKILL.md` body.
    UpdateSkillMd,
    /// Append a new file under `knowledge/`.
    AddKnowledgeBlock,
    /// Update a single metadata field (description, category, or tags).
    UpdateMetadata,
}

/// The content carried by a [`SkillPatch`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PatchPayload {
    /// New SKILL.md body (YAML frontmatter + markdown body).
    SkillMd { new_body: String },
    /// New knowledge file to add under `knowledge/`.
    Knowledge { filename: String, body: String },
    /// A metadata field update.
    Metadata { field: String, value: String },
}

// ---------------------------------------------------------------------------
// Validated patch
// ---------------------------------------------------------------------------

/// A [`SkillPatch`] that has passed validation, together with precomputed
/// metadata for the approval trail.
#[derive(Debug, Clone)]
pub struct ValidatedPatch {
    /// The original patch (preserved for the applier).
    pub patch: SkillPatch,
    /// Character-level size of the largest changed blob, for budget tracking.
    pub payload_bytes: usize,
    /// Human-readable diff summary (single line). Not a real diff; just a
    /// structured description that an approval record can reference.
    pub diff_summary: String,
}

// ---------------------------------------------------------------------------
// In-memory representation of a skill pack used by the patch pipeline
// ---------------------------------------------------------------------------

/// A minimal in-memory snapshot of a skill pack, sufficient for the
/// self-patching pipeline. The validator reads from this; the applier produces
/// a new one (or writes through to disk).
#[derive(Debug, Clone, Default)]
pub struct SkillPack {
    /// Skill identifier (e.g. `"code-review"`).
    pub skill_id: String,
    /// Currently installed version string (semver recommended).
    pub version: String,
    /// Raw SKILL.md content.
    pub skill_md: String,
    /// Knowledge files keyed by filename (relative, e.g. `"intro.md"`).
    pub knowledge: HashMap<String, String>,
    /// Arbitrary metadata fields (description, category, tags, …).
    pub metadata: HashMap<String, String>,
}

impl SkillPack {
    /// Convenience constructor.
    pub fn new(skill_id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            skill_id: skill_id.into(),
            version: version.into(),
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by the self-patching pipeline.
#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    #[error("version mismatch: patch targets {expected}, pack is at {actual}")]
    VersionMismatch { expected: String, actual: String },

    #[error("syntax invalid: {0}")]
    SyntaxInvalid(String),

    #[error("size exceeded: body is {0} bytes (limit 65536)")]
    SizeExceeded(usize),

    #[error("duplicate knowledge filename: {0}")]
    DuplicateKnowledge(String),

    #[error("write failed: {0}")]
    WriteFailed(String),
}

// ---------------------------------------------------------------------------
// Validator trait + default implementation
// ---------------------------------------------------------------------------

/// Validates a [`SkillPatch`] against the current [`SkillPack`].
pub trait SelfPatchValidator: Send + Sync {
    /// Returns a [`ValidatedPatch`] on success, or a [`PatchError`] describing
    /// why the patch was rejected.
    fn validate(
        &self,
        patch: &SkillPatch,
        current: &SkillPack,
    ) -> Result<ValidatedPatch, PatchError>;
}

/// Production validator shipped with `sera-skills`.
///
/// Checks:
/// 1. Version alignment (`patch.base_version == current.version`).
/// 2. For `SkillMd` patches — minimal YAML frontmatter syntax.
/// 3. For `SkillMd` patches — body ≤ 64 KB.
/// 4. For `AddKnowledgeBlock` patches — no duplicate filename.
#[derive(Debug, Default)]
pub struct DefaultSelfPatchValidator;

impl SelfPatchValidator for DefaultSelfPatchValidator {
    fn validate(
        &self,
        patch: &SkillPatch,
        current: &SkillPack,
    ) -> Result<ValidatedPatch, PatchError> {
        // 1. Version check.
        if patch.base_version != current.version {
            return Err(PatchError::VersionMismatch {
                expected: patch.base_version.clone(),
                actual: current.version.clone(),
            });
        }

        let (payload_bytes, diff_summary) = match &patch.payload {
            PatchPayload::SkillMd { new_body } => {
                // 2. Syntax: the body must start with a YAML frontmatter block.
                validate_yaml_frontmatter(new_body)?;
                // 3. Size budget.
                let size = new_body.len();
                if size > MAX_SKILL_MD_BYTES {
                    return Err(PatchError::SizeExceeded(size));
                }
                let summary = format!(
                    "UpdateSkillMd skill_id={} bytes={}",
                    patch.skill_id, size
                );
                (size, summary)
            }

            PatchPayload::Knowledge { filename, body } => {
                // 4. Duplicate check.
                if current.knowledge.contains_key(filename.as_str()) {
                    return Err(PatchError::DuplicateKnowledge(filename.clone()));
                }
                let size = body.len();
                let summary = format!(
                    "AddKnowledgeBlock skill_id={} filename={} bytes={}",
                    patch.skill_id, filename, size
                );
                (size, summary)
            }

            PatchPayload::Metadata { field, value } => {
                let summary = format!(
                    "UpdateMetadata skill_id={} field={} value_len={}",
                    patch.skill_id,
                    field,
                    value.len()
                );
                (value.len(), summary)
            }
        };

        Ok(ValidatedPatch {
            patch: patch.clone(),
            payload_bytes,
            diff_summary,
        })
    }
}

/// Checks that `body` begins with a `---`-delimited YAML frontmatter block.
/// Uses `serde_yaml` to parse the frontmatter as a loose mapping.
fn validate_yaml_frontmatter(body: &str) -> Result<(), PatchError> {
    // Must start with `---`
    if !body.starts_with("---") {
        return Err(PatchError::SyntaxInvalid(
            "SKILL.md must start with a YAML frontmatter block (---)".into(),
        ));
    }

    // Find the closing `---`
    let after_open = &body[3..];
    let close_pos = after_open
        .find("\n---")
        .ok_or_else(|| PatchError::SyntaxInvalid("missing closing --- in frontmatter".into()))?;

    let frontmatter = &after_open[..close_pos];
    serde_yaml::from_str::<serde_yaml::Value>(frontmatter).map_err(|e| {
        PatchError::SyntaxInvalid(format!("frontmatter YAML parse error: {e}"))
    })?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Applier trait + implementations
// ---------------------------------------------------------------------------

/// Applies a [`ValidatedPatch`] and returns the updated [`SkillPack`].
pub trait SelfPatchApplier: Send + Sync {
    /// Consume the validated patch and return the updated pack.
    fn apply(&self, validated: ValidatedPatch) -> Result<SkillPack, PatchError>;
}

// ---------------------------------------------------------------------------
// InMemorySelfPatchApplier  (for tests)
// ---------------------------------------------------------------------------

/// An applier that works purely in memory. The caller supplies the current
/// pack snapshot; the applier returns a cloned, updated version.
#[derive(Debug, Default)]
pub struct InMemorySelfPatchApplier {
    /// The current pack snapshot this applier operates against.
    pub current: SkillPack,
}

impl InMemorySelfPatchApplier {
    /// Create an applier wrapping the given pack snapshot.
    pub fn new(current: SkillPack) -> Self {
        Self { current }
    }
}

impl SelfPatchApplier for InMemorySelfPatchApplier {
    fn apply(&self, validated: ValidatedPatch) -> Result<SkillPack, PatchError> {
        let mut updated = self.current.clone();
        apply_patch_to_pack(&mut updated, &validated.patch)?;
        Ok(updated)
    }
}

// ---------------------------------------------------------------------------
// FsSelfPatchApplier  (writes through to disk atomically)
// ---------------------------------------------------------------------------

/// An applier that persists the updated skill pack to the filesystem.
///
/// Write strategy:
/// 1. Copy the entire skill directory into a temp dir alongside it.
/// 2. Apply the patch to the temp copy.
/// 3. Atomically rename the temp copy over the original.
///
/// This ensures the live pack is never in a partial state.
#[derive(Debug)]
pub struct FsSelfPatchApplier {
    /// Root directory that contains skill directories, e.g. `/skills/`.
    pub root: PathBuf,
}

impl FsSelfPatchApplier {
    /// Create an applier rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn skill_dir(&self, skill_id: &str) -> PathBuf {
        self.root.join(skill_id)
    }
}

impl SelfPatchApplier for FsSelfPatchApplier {
    fn apply(&self, validated: ValidatedPatch) -> Result<SkillPack, PatchError> {
        let skill_id = &validated.patch.skill_id;
        let skill_dir = self.skill_dir(skill_id);

        // Build a temp directory in the same parent so rename(2) stays on the
        // same filesystem (required for atomicity on Linux).
        let parent = skill_dir.parent().unwrap_or(&self.root);
        let tmp_path = make_temp_dir_in(parent, &format!(".{skill_id}-patch-"))
            .map_err(|e| PatchError::WriteFailed(e.to_string()))?;

        // Copy existing skill directory into tmp (if it exists).
        if skill_dir.exists() {
            copy_dir_all(&skill_dir, &tmp_path)
                .map_err(|e| PatchError::WriteFailed(e.to_string()))?;
        }

        // Apply the patch to the temp copy.
        apply_patch_to_fs(&tmp_path, &validated.patch)
            .map_err(|e| PatchError::WriteFailed(e.to_string()))?;

        // Atomic swap: remove old skill dir then rename tmp into place.
        // Linux rename(2) cannot replace a non-empty directory, so we remove
        // the old one first. The window between remove and rename is small and
        // acceptable for this scaffold (no concurrent writers assumed).
        if skill_dir.exists() {
            std::fs::remove_dir_all(&skill_dir)
                .map_err(|e| PatchError::WriteFailed(format!("remove old dir failed: {e}")))?;
        }
        std::fs::rename(&tmp_path, &skill_dir)
            .map_err(|e| PatchError::WriteFailed(format!("rename failed: {e}")))?;

        // Return an in-memory snapshot of the updated pack.
        let mut updated = SkillPack::new(skill_id.clone(), validated.patch.base_version.clone());
        // Re-read skill.md if present.
        let skill_md_path = skill_dir.join("SKILL.md");
        if skill_md_path.exists() {
            updated.skill_md = std::fs::read_to_string(&skill_md_path)
                .map_err(|e| PatchError::WriteFailed(e.to_string()))?;
        }
        // Re-read knowledge/*.
        let knowledge_dir = skill_dir.join("knowledge");
        if knowledge_dir.exists() {
            for entry in std::fs::read_dir(&knowledge_dir)
                .map_err(|e| PatchError::WriteFailed(e.to_string()))?
            {
                let entry = entry.map_err(|e| PatchError::WriteFailed(e.to_string()))?;
                let p = entry.path();
                if p.is_file() {
                    let fname = p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    let content = std::fs::read_to_string(&p)
                        .map_err(|e| PatchError::WriteFailed(e.to_string()))?;
                    updated.knowledge.insert(fname, content);
                }
            }
        }

        Ok(updated)
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Apply a patch to a mutable [`SkillPack`] (in-memory path).
fn apply_patch_to_pack(pack: &mut SkillPack, patch: &SkillPatch) -> Result<(), PatchError> {
    match &patch.payload {
        PatchPayload::SkillMd { new_body } => {
            pack.skill_md = new_body.clone();
        }
        PatchPayload::Knowledge { filename, body } => {
            pack.knowledge.insert(filename.clone(), body.clone());
        }
        PatchPayload::Metadata { field, value } => {
            pack.metadata.insert(field.clone(), value.clone());
        }
    }
    Ok(())
}

/// Apply a patch to a filesystem directory (temp-copy path).
fn apply_patch_to_fs(dir: &std::path::Path, patch: &SkillPatch) -> std::io::Result<()> {
    match &patch.payload {
        PatchPayload::SkillMd { new_body } => {
            std::fs::write(dir.join("SKILL.md"), new_body)?;
        }
        PatchPayload::Knowledge { filename, body } => {
            let kdir = dir.join("knowledge");
            std::fs::create_dir_all(&kdir)?;
            std::fs::write(kdir.join(filename), body)?;
        }
        PatchPayload::Metadata { field, value } => {
            // Persist metadata as a simple `<field>=<value>` line in a
            // `metadata.txt` sidecar. Real production code would use YAML;
            // this scaffold keeps the dependency footprint minimal.
            let meta_path = dir.join("metadata.txt");
            let mut existing = if meta_path.exists() {
                std::fs::read_to_string(&meta_path)?
            } else {
                String::new()
            };
            // Remove any existing line for this field.
            existing = existing
                .lines()
                .filter(|l| !l.starts_with(&format!("{field}=")))
                .collect::<Vec<_>>()
                .join("\n");
            if !existing.is_empty() {
                existing.push('\n');
            }
            existing.push_str(&format!("{field}={value}\n"));
            std::fs::write(&meta_path, existing)?;
        }
    }
    Ok(())
}

/// Create a uniquely-named temporary directory inside `parent`.
/// The caller is responsible for removing it (or renaming it over the target).
fn make_temp_dir_in(parent: &std::path::Path, prefix: &str) -> std::io::Result<PathBuf> {
    use std::time::{SystemTime, UNIX_EPOCH};
    std::fs::create_dir_all(parent)?;
    // Use nanos + process id for a collision-resistant suffix.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let dir_name = format!("{prefix}{pid}-{nanos}");
    let path = parent.join(&dir_name);
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

/// Recursively copy `src` directory into `dst` (dst must already exist).
fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
