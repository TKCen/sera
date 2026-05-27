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
        body_bytes: usize,
    ) -> Result<(), PatchRejection> {
        let root_ok = self.allowed_roots.iter().any(|allowed| {
            skill_root.starts_with(allowed) || skill_root == allowed
        });
        if !root_ok {
            return Err(PatchRejection {
                reason: format!(
                    "skill root '{}' is not in the allowed roots for Tier 1 patches",
                    skill_root.display()
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

/// Permissive policy that approves every patch (for tests or open sandboxes).
pub struct AllowAllPolicy;

impl SkillPatchPolicy for AllowAllPolicy {
    fn check_patch(&self, _: &str, _: &Path, _: usize) -> Result<(), PatchRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier1_allows_patch_within_root_and_budget() {
        let policy = Tier1SkillPatchPolicy::new(
            vec![PathBuf::from("/skills")],
            64 * 1024,
        );
        assert!(policy.check_patch("my-skill", Path::new("/skills"), 100).is_ok());
        assert!(policy.check_patch("my-skill", Path::new("/skills/sub"), 100).is_ok());
    }

    #[test]
    fn tier1_rejects_outside_allowed_root() {
        let policy = Tier1SkillPatchPolicy::new(
            vec![PathBuf::from("/skills")],
            64 * 1024,
        );
        let err = policy
            .check_patch("evil", Path::new("/etc"), 10)
            .unwrap_err();
        assert!(err.reason.contains("not in the allowed roots"));
    }

    #[test]
    fn tier1_rejects_over_budget() {
        let policy = Tier1SkillPatchPolicy::new(
            vec![PathBuf::from("/skills")],
            1024,
        );
        let err = policy
            .check_patch("big", Path::new("/skills"), 2048)
            .unwrap_err();
        assert!(err.reason.contains("exceeds Tier 1 budget"));
    }

    #[test]
    fn tier1_multiple_roots() {
        let policy = Tier1SkillPatchPolicy::new(
            vec![PathBuf::from("/a"), PathBuf::from("/b")],
            64 * 1024,
        );
        assert!(policy.check_patch("s", Path::new("/a"), 10).is_ok());
        assert!(policy.check_patch("s", Path::new("/b"), 10).is_ok());
        assert!(policy.check_patch("s", Path::new("/c"), 10).is_err());
    }

    #[test]
    fn allow_all_always_permits() {
        let policy = AllowAllPolicy;
        assert!(policy.check_patch("x", Path::new("/anywhere"), 999_999).is_ok());
    }

    #[test]
    fn rejection_display() {
        let r = PatchRejection {
            reason: "too big".into(),
        };
        assert_eq!(r.to_string(), "too big");
    }
}
