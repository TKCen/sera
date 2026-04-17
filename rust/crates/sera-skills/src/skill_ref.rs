//! [`SkillRef`] — a skill reference with an optional semver constraint and
//! optional source hint.
//!
//! Grammar accepted by [`SkillRef::parse`]:
//!
//! ```text
//! ref          := [source-hint ":"] name [ "@" version-constraint ]
//! source-hint  := "fs" | "plugin" | "registry"
//! name         := [a-z0-9-_./]{1,128}   (lenient; registry refs may include "/")
//! constraint   := any valid `semver::VersionReq` ("^1.2", "=1.0.0", ">=1,<2", …)
//! ```
//!
//! Examples:
//!   * `"triage"`                       — any version, any source
//!   * `"triage@^1.2"`                  — semver caret, any source
//!   * `"triage@=1.0.0"`                — exact version pin
//!   * `"plugin:triage@^1"`             — only resolvable via a plugin source
//!   * `"registry:ghcr.io/org/pack@1.0"` — only resolvable via the OCI registry

use std::fmt;

use semver::VersionReq;
use serde::{Deserialize, Serialize};

use crate::error::SkillsError;

/// Where a skill resolution came from (or where it should come from).
///
/// Used as a hint on `SkillRef::source_hint` and as a tag on
/// `ResolvedSkill`/`SkillSearchHit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillSourceKind {
    /// Local filesystem (markdown packs).
    Fs,
    /// Skill advertised by a running plugin.
    Plugin,
    /// OCI registry-hosted skill pack.
    Registry,
}

impl fmt::Display for SkillSourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fs => f.write_str("fs"),
            Self::Plugin => f.write_str("plugin"),
            Self::Registry => f.write_str("registry"),
        }
    }
}

/// A reference to a skill, with an optional version constraint and optional
/// source hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRef {
    /// Skill name (registry refs may be a full OCI-ish path).
    pub name: String,
    /// Semver requirement; `None` means "any version".
    pub version: Option<VersionReq>,
    /// Hint for which source to try. `None` means "try every configured
    /// source in priority order".
    pub source_hint: Option<SkillSourceKind>,
}

impl SkillRef {
    /// Parse a reference string.
    ///
    /// See module docs for the grammar.
    pub fn parse(raw: &str) -> Result<Self, SkillsError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(SkillsError::InvalidReference("empty reference".into()));
        }

        // Split off optional source hint. A bare "name:port" style segment
        // would collide, so the hint set is fixed to known tokens.
        let (hint, remainder) = match raw.split_once(':') {
            Some((prefix, rest)) => match prefix {
                "fs" => (Some(SkillSourceKind::Fs), rest),
                "plugin" => (Some(SkillSourceKind::Plugin), rest),
                "registry" => (Some(SkillSourceKind::Registry), rest),
                _ => (None, raw),
            },
            None => (None, raw),
        };

        if remainder.is_empty() {
            return Err(SkillsError::InvalidReference(format!(
                "empty name after source hint in '{raw}'"
            )));
        }

        // Split off optional version constraint on the *last* `@`. Registry
        // refs may legitimately contain `@` in digests — but phase-5 registry
        // refs are expected to be `registry:REPO@VERSION`, not digests, so
        // rsplit once is safe.
        let (name, version) = match remainder.rsplit_once('@') {
            Some((name, ver)) => {
                let req = VersionReq::parse(ver).map_err(|e| {
                    SkillsError::InvalidReference(format!(
                        "invalid version constraint '{ver}' in '{raw}': {e}"
                    ))
                })?;
                (name, Some(req))
            }
            None => (remainder, None),
        };

        if name.is_empty() {
            return Err(SkillsError::InvalidReference(format!(
                "empty name in '{raw}'"
            )));
        }

        Ok(SkillRef {
            name: name.to_string(),
            version,
            source_hint: hint,
        })
    }

    /// Returns `true` if this reference's version constraint is satisfied by
    /// the given version string. A `None` constraint matches anything. An
    /// unparseable `version` string counts as "not satisfying".
    pub fn satisfied_by(&self, version: &str) -> bool {
        let Some(req) = &self.version else {
            return true;
        };
        match semver::Version::parse(version) {
            Ok(v) => req.matches(&v),
            Err(_) => false,
        }
    }

    /// If the constraint pins an exact version (`=x.y.z`), return that
    /// version. Otherwise `None`.
    ///
    /// Used by the registry source to refuse fuzzy range queries in phase 5.
    pub fn exact_version(&self) -> Option<semver::Version> {
        let req = self.version.as_ref()?;
        // A `VersionReq` like `=1.2.3` has exactly one comparator with the
        // `Exact` op and all three version fields set.
        if req.comparators.len() != 1 {
            return None;
        }
        let c = &req.comparators[0];
        if c.op != semver::Op::Exact {
            return None;
        }
        Some(semver::Version::new(
            c.major,
            c.minor?,
            c.patch?,
        ))
    }
}

impl fmt::Display for SkillRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(hint) = self.source_hint {
            write!(f, "{hint}:")?;
        }
        f.write_str(&self.name)?;
        if let Some(v) = &self.version {
            write!(f, "@{v}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_name() {
        let r = SkillRef::parse("triage").unwrap();
        assert_eq!(r.name, "triage");
        assert!(r.version.is_none());
        assert!(r.source_hint.is_none());
    }

    #[test]
    fn parses_name_with_caret_constraint() {
        let r = SkillRef::parse("triage@^1.2").unwrap();
        assert_eq!(r.name, "triage");
        let req = r.version.unwrap();
        assert!(req.matches(&semver::Version::parse("1.2.5").unwrap()));
        assert!(!req.matches(&semver::Version::parse("2.0.0").unwrap()));
    }

    #[test]
    fn parses_name_with_exact_pin() {
        let r = SkillRef::parse("triage@=1.0.0").unwrap();
        assert_eq!(r.exact_version(), Some(semver::Version::new(1, 0, 0)));
    }

    #[test]
    fn parses_plugin_hint() {
        let r = SkillRef::parse("plugin:triage@^1").unwrap();
        assert_eq!(r.name, "triage");
        assert_eq!(r.source_hint, Some(SkillSourceKind::Plugin));
    }

    #[test]
    fn parses_registry_hint_with_path() {
        let r = SkillRef::parse("registry:ghcr.io/org/pack@1.0").unwrap();
        assert_eq!(r.source_hint, Some(SkillSourceKind::Registry));
        assert_eq!(r.name, "ghcr.io/org/pack");
        assert!(r.version.is_some());
    }

    #[test]
    fn parses_fs_hint() {
        let r = SkillRef::parse("fs:local-skill").unwrap();
        assert_eq!(r.source_hint, Some(SkillSourceKind::Fs));
        assert_eq!(r.name, "local-skill");
        assert!(r.version.is_none());
    }

    #[test]
    fn empty_raw_is_rejected() {
        assert!(SkillRef::parse("").is_err());
        assert!(SkillRef::parse("   ").is_err());
    }

    #[test]
    fn empty_name_after_hint_is_rejected() {
        assert!(SkillRef::parse("plugin:").is_err());
    }

    #[test]
    fn empty_name_with_version_is_rejected() {
        assert!(SkillRef::parse("@1.0.0").is_err());
    }

    #[test]
    fn invalid_version_constraint_is_rejected() {
        assert!(SkillRef::parse("triage@not-semver!!").is_err());
    }

    #[test]
    fn unknown_prefix_is_treated_as_name() {
        // "foo:bar" where "foo" is not a known hint falls through and becomes
        // a bare name — callers can use the full string as the ref.
        let r = SkillRef::parse("other:triage").unwrap();
        assert_eq!(r.source_hint, None);
        assert_eq!(r.name, "other:triage");
    }

    #[test]
    fn satisfied_by_handles_none_constraint() {
        let r = SkillRef::parse("triage").unwrap();
        assert!(r.satisfied_by("1.2.3"));
        assert!(r.satisfied_by("0.0.1"));
    }

    #[test]
    fn satisfied_by_handles_caret() {
        let r = SkillRef::parse("triage@^1.2").unwrap();
        assert!(r.satisfied_by("1.2.0"));
        assert!(r.satisfied_by("1.9.0"));
        assert!(!r.satisfied_by("2.0.0"));
    }

    #[test]
    fn satisfied_by_returns_false_for_bad_version() {
        let r = SkillRef::parse("triage@^1").unwrap();
        assert!(!r.satisfied_by("not-a-version"));
    }

    #[test]
    fn exact_version_returns_none_for_ranges() {
        let r = SkillRef::parse("triage@^1.2").unwrap();
        assert_eq!(r.exact_version(), None);
    }

    #[test]
    fn display_roundtrips() {
        let raw = "plugin:triage@^1.2";
        let r = SkillRef::parse(raw).unwrap();
        assert_eq!(r.to_string(), "plugin:triage@^1.2");
    }
}
