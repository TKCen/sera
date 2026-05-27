//! Narrow policy interface for governing skill patch operations.
//!
//! [`SkillPatchPolicy`] is the Tier 1 boundary check that runs before
//! `SelfPatchValidator`. Full `sera-meta::PolicyEngine` integration is a
//! follow-up; this trait is the seam for that wiring.

use std::path::{Path, PathBuf};

/// Why a patch was rejected by policy.
#[derive(Debug, Clone)]
pub struct PatchRejection {
    pub reason: String,
}

impl std::fmt::Display for PatchRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

/// Policy gate for skill patch operations.
pub trait SkillPatchPolicy: Send + Sync {
    fn check_patch(
        &self,
        skill_name: &str,
        skill_root: &Path,
        skill_path: &Path,
        body_bytes: usize,
    ) -> Result<(), PatchRejection>;
}

/// Default Tier 1 policy: restricts patches to allowed skill roots and
/// enforces a per-patch body budget.
pub struct Tier1SkillPatchPolicy {
    allowed_roots: Vec<PathBuf>,
    max_body_bytes: usize,
}

impl Tier1SkillPatchPolicy {
    pub fn new(allowed_roots: Vec<PathBuf>, max_body_bytes: usize) -> Self {
        Self {
            allowed_roots,
            max_body_bytes,
        }
    }
}

impl SkillPatchPolicy for Tier1SkillPatchPolicy {
    fn check_patch(
        &self,
        _skill_name: &str,
        skill_root: &Path,
        skill_path: &Path,
        body_bytes: usize,
    ) -> Result<(), PatchRejection> {
        let skill_root = canonical_or_original(skill_root);
        let skill_path = canonical_or_original(skill_path);
        let root_ok = self.allowed_roots.iter().any(|allowed| {
            let allowed = canonical_or_original(allowed);
            skill_root.starts_with(&allowed) && skill_path.starts_with(&allowed)
        });
        if !root_ok {
            return Err(PatchRejection {
                reason: format!(
                    "skill path '{}' is not contained by an allowed Tier 1 root",
                    skill_path.display()
                ),
            });
        }
        if body_bytes > self.max_body_bytes {
            return Err(PatchRejection {
                reason: format!(
                    "patch body {} bytes exceeds Tier 1 budget of {} bytes",
                    body_bytes, self.max_body_bytes
                ),
            });
        }
        Ok(())
    }
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Permissive policy that approves every patch (for tests or open sandboxes).
pub struct AllowAllPolicy;

impl SkillPatchPolicy for AllowAllPolicy {
    fn check_patch(&self, _: &str, _: &Path, _: &Path, _: usize) -> Result<(), PatchRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier1_allows_patch_within_root_and_budget() {
        let policy = Tier1SkillPatchPolicy::new(vec![PathBuf::from("/skills")], 64 * 1024);
        assert!(
            policy
                .check_patch(
                    "my-skill",
                    Path::new("/skills"),
                    Path::new("/skills/my-skill"),
                    100
                )
                .is_ok()
        );
        assert!(
            policy
                .check_patch(
                    "my-skill",
                    Path::new("/skills/sub"),
                    Path::new("/skills/sub/my-skill"),
                    100
                )
                .is_ok()
        );
    }

    #[test]
    fn tier1_rejects_outside_allowed_root() {
        let policy = Tier1SkillPatchPolicy::new(vec![PathBuf::from("/skills")], 64 * 1024);
        let err = policy
            .check_patch("evil", Path::new("/etc"), Path::new("/etc/evil"), 10)
            .unwrap_err();
        assert!(
            err.reason
                .contains("not contained by an allowed Tier 1 root")
        );
    }

    #[test]
    fn tier1_rejects_over_budget() {
        let policy = Tier1SkillPatchPolicy::new(vec![PathBuf::from("/skills")], 1024);
        let err = policy
            .check_patch("big", Path::new("/skills"), Path::new("/skills/big"), 2048)
            .unwrap_err();
        assert!(err.reason.contains("exceeds Tier 1 budget"));
    }

    #[test]
    fn tier1_multiple_roots() {
        let policy =
            Tier1SkillPatchPolicy::new(vec![PathBuf::from("/a"), PathBuf::from("/b")], 64 * 1024);
        assert!(
            policy
                .check_patch("s", Path::new("/a"), Path::new("/a/s"), 10)
                .is_ok()
        );
        assert!(
            policy
                .check_patch("s", Path::new("/b"), Path::new("/b/s"), 10)
                .is_ok()
        );
        assert!(
            policy
                .check_patch("s", Path::new("/c"), Path::new("/c/s"), 10)
                .is_err()
        );
    }

    #[test]
    fn allow_all_always_permits() {
        let policy = AllowAllPolicy;
        assert!(
            policy
                .check_patch(
                    "x",
                    Path::new("/anywhere"),
                    Path::new("/anywhere/x"),
                    999_999
                )
                .is_ok()
        );
    }

    #[test]
    #[cfg(unix)]
    fn tier1_rejects_symlink_escape_from_allowed_root() {
        let tmp = tempfile::tempdir().unwrap();
        let allowed = tmp.path().join("allowed");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let outside_skill = outside.join("evil.md");
        std::fs::write(&outside_skill, "---\nname: evil\nversion: 1.0.0\n---\n").unwrap();
        let linked_skill = allowed.join("evil.md");
        std::os::unix::fs::symlink(&outside_skill, &linked_skill).unwrap();

        let policy = Tier1SkillPatchPolicy::new(vec![allowed.clone()], 64 * 1024);
        let err = policy
            .check_patch("evil", &allowed, &linked_skill, 10)
            .unwrap_err();
        assert!(
            err.reason
                .contains("not contained by an allowed Tier 1 root")
        );
    }

    #[test]
    fn rejection_display() {
        let r = PatchRejection {
            reason: "too big".into(),
        };
        assert_eq!(r.to_string(), "too big");
    }
}
