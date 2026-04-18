//! Skill pack lock file — the human-grep-friendly record of what a
//! [`SkillResolver`][crate::resolver::SkillResolver] actually resolved.
//!
//! The lock file lives next to the project's skill pack manifest. It is a
//! TOML document so operators can diff revisions easily. Schema version 1:
//!
//! ```toml
//! version = 1
//!
//! [[skills]]
//! name          = "triage"
//! version       = "1.0.0"
//! source        = "fs"
//! source_detail = "/abs/path/to/pack"
//! content_hash  = "sha256:…"
//! ```

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::SkillsError;
use crate::resolver::ResolvedSkillBundle;
use crate::skill_ref::SkillSourceKind;

/// Current lock file schema version.
pub const LOCKFILE_SCHEMA_VERSION: u32 = 1;

/// One entry per resolved skill.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillLockEntry {
    pub name: String,
    pub version: String,
    pub source: SkillSourceKind,
    pub source_detail: String,
    pub content_hash: String,
}

/// TOML-serializable lock file document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillLockFile {
    pub version: u32,
    #[serde(default)]
    pub skills: Vec<SkillLockEntry>,
}

impl SkillLockFile {
    /// Create an empty lock file at schema v1.
    pub fn empty() -> Self {
        Self {
            version: LOCKFILE_SCHEMA_VERSION,
            skills: Vec::new(),
        }
    }

    /// Build a lock file from a resolved bundle.
    pub fn from_bundle(bundle: &ResolvedSkillBundle) -> Self {
        let mut skills: Vec<SkillLockEntry> = bundle
            .skills
            .iter()
            .map(|r| SkillLockEntry {
                name: r.definition.name.clone(),
                version: r
                    .definition
                    .version
                    .clone()
                    .unwrap_or_else(|| "0.0.0".into()),
                source: r.source,
                source_detail: r.pack_name.clone(),
                content_hash: content_hash(r.definition.body.as_deref().unwrap_or("")),
            })
            .collect();
        // Deterministic ordering keeps diffs minimal across runs.
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Self {
            version: LOCKFILE_SCHEMA_VERSION,
            skills,
        }
    }

    /// Read a lock file from disk.
    pub fn read(path: &Path) -> Result<Self, SkillsError> {
        let raw = std::fs::read_to_string(path)?;
        let parsed: Self = toml::from_str(&raw)?;
        if parsed.version != LOCKFILE_SCHEMA_VERSION {
            return Err(SkillsError::InvalidFormat(format!(
                "lockfile schema version {} is not supported (want {LOCKFILE_SCHEMA_VERSION})",
                parsed.version
            )));
        }
        Ok(parsed)
    }

    /// Write a lock file to disk.
    pub fn write(&self, path: &Path) -> Result<(), SkillsError> {
        let raw = toml::to_string_pretty(self)?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, raw)?;
        Ok(())
    }

    /// Compare this (existing) lockfile against a freshly resolved bundle
    /// to classify the delta.
    pub fn reconcile(&self, bundle: &ResolvedSkillBundle) -> LockReconciliation {
        let fresh = Self::from_bundle(bundle);

        // Index by name.
        let existing: std::collections::HashMap<&str, &SkillLockEntry> =
            self.skills.iter().map(|s| (s.name.as_str(), s)).collect();
        let incoming: std::collections::HashMap<&str, &SkillLockEntry> =
            fresh.skills.iter().map(|s| (s.name.as_str(), s)).collect();

        let mut adds: Vec<String> = Vec::new();
        let mut removes: Vec<String> = Vec::new();
        let mut changes: Vec<(String, String)> = Vec::new();

        for (name, new_entry) in &incoming {
            match existing.get(name) {
                None => adds.push((*name).to_string()),
                Some(old_entry) => {
                    if old_entry.content_hash != new_entry.content_hash
                        || old_entry.version != new_entry.version
                        || old_entry.source != new_entry.source
                        || old_entry.source_detail != new_entry.source_detail
                    {
                        changes.push((
                            (*name).to_string(),
                            format!("{} -> {}", old_entry.version, new_entry.version),
                        ));
                    }
                }
            }
        }
        for name in existing.keys() {
            if !incoming.contains_key(name) {
                removes.push((*name).to_string());
            }
        }

        if adds.is_empty() && removes.is_empty() && changes.is_empty() {
            LockReconciliation::InSync
        } else {
            adds.sort();
            removes.sort();
            changes.sort();
            LockReconciliation::Drifted {
                adds,
                removes,
                changes,
            }
        }
    }
}

/// Output of [`SkillLockFile::reconcile`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockReconciliation {
    InSync,
    Drifted {
        adds: Vec<String>,
        removes: Vec<String>,
        changes: Vec<(String, String)>,
    },
}

fn content_hash(body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_ref::SkillRef;
    use crate::source::ResolvedSkill;
    use sera_types::skill::SkillDefinition;
    use tempfile::tempdir;

    fn resolved(name: &str, version: &str, source: SkillSourceKind, pack: &str, body: &str) -> ResolvedSkill {
        ResolvedSkill {
            reference: SkillRef::parse(name).unwrap(),
            definition: SkillDefinition {
                name: name.into(),
                description: None,
                version: Some(version.into()),
                parameters: None,
                source: None,
                body: Some(body.into()),
                triggers: vec![],
                model_override: None,
                context_budget_tokens: None,
                tool_bindings: vec![],
                mcp_servers: vec![],
            },
            pack_name: pack.into(),
            source,
        }
    }

    fn bundle(skills: Vec<ResolvedSkill>) -> ResolvedSkillBundle {
        ResolvedSkillBundle { skills, misses: Vec::new() }
    }

    #[test]
    fn from_bundle_populates_entries_sorted() {
        let b = bundle(vec![
            resolved("z", "1.0.0", SkillSourceKind::Fs, "p1", "body-z"),
            resolved("a", "2.0.0", SkillSourceKind::Registry, "p2", "body-a"),
        ]);
        let lock = SkillLockFile::from_bundle(&b);
        assert_eq!(lock.version, LOCKFILE_SCHEMA_VERSION);
        assert_eq!(lock.skills.len(), 2);
        assert_eq!(lock.skills[0].name, "a");
        assert_eq!(lock.skills[1].name, "z");
        assert!(lock.skills[0].content_hash.starts_with("sha256:"));
    }

    #[test]
    fn toml_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("skills.lock");
        let original = SkillLockFile::from_bundle(&bundle(vec![resolved(
            "triage",
            "1.0.0",
            SkillSourceKind::Fs,
            "/packs/main",
            "body",
        )]));
        original.write(&path).unwrap();
        let loaded = SkillLockFile::read(&path).unwrap();
        assert_eq!(original, loaded);
    }

    #[test]
    fn read_rejects_unknown_schema_version() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("skills.lock");
        std::fs::write(&path, "version = 99\nskills = []\n").unwrap();
        let err = SkillLockFile::read(&path).unwrap_err();
        assert!(matches!(err, SkillsError::InvalidFormat(_)));
    }

    #[test]
    fn reconcile_in_sync() {
        let b = bundle(vec![resolved(
            "triage", "1.0.0", SkillSourceKind::Fs, "p", "body",
        )]);
        let lock = SkillLockFile::from_bundle(&b);
        assert_eq!(lock.reconcile(&b), LockReconciliation::InSync);
    }

    #[test]
    fn reconcile_detects_add_remove_change() {
        let first = bundle(vec![
            resolved("triage", "1.0.0", SkillSourceKind::Fs, "p", "body"),
            resolved("deploy", "0.1.0", SkillSourceKind::Fs, "p", "body"),
        ]);
        let lock = SkillLockFile::from_bundle(&first);

        let second = bundle(vec![
            // triage version bumped and body changed
            resolved("triage", "1.0.1", SkillSourceKind::Fs, "p", "body-v2"),
            // deploy removed, onboard added
            resolved("onboard", "0.1.0", SkillSourceKind::Fs, "p", "body"),
        ]);

        match lock.reconcile(&second) {
            LockReconciliation::Drifted { adds, removes, changes } => {
                assert_eq!(adds, vec!["onboard".to_string()]);
                assert_eq!(removes, vec!["deploy".to_string()]);
                assert_eq!(changes.len(), 1);
                assert_eq!(changes[0].0, "triage");
            }
            other => panic!("expected Drifted, got {other:?}"),
        }
    }

    #[test]
    fn empty_lock_file_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("skills.lock");
        let lock = SkillLockFile::empty();
        lock.write(&path).unwrap();
        let loaded = SkillLockFile::read(&path).unwrap();
        assert_eq!(loaded, lock);
    }

    #[test]
    fn content_hash_is_deterministic() {
        assert_eq!(content_hash("abc"), content_hash("abc"));
        assert_ne!(content_hash("abc"), content_hash("abd"));
    }
}
