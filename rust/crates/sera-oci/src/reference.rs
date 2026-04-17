//! OCI reference parsing — the `registry/repository[:tag|@digest]` string.
//!
//! Internally we keep our own `OciReference` type (rather than leaking
//! `oci_distribution::Reference`) so that:
//!
//! 1. Callers depend only on `sera-oci` types, not the transitive OCI crate.
//! 2. Error messages come through `OciError::InvalidReference` uniformly.
//! 3. We can pick a SERA-specific default registry.
//!
//! Conversion to `oci_distribution::Reference` happens inside [`crate::puller`].

use std::fmt;

use crate::error::OciError;

/// Default registry used when a reference omits the registry segment
/// (`my-plugin:1.0` → `ghcr.io/my-plugin:1.0`).
///
/// Chosen because SERA's own plugins are published to GHCR and because
/// `docs/plan/PLUGIN-MCP-ECOSYSTEM.md` §3.2 uses GHCR in its examples. Unlike
/// docker.io there is no hidden namespace rewrite (`library/foo`) to surprise
/// operators.
pub const DEFAULT_REGISTRY: &str = "ghcr.io";

/// A parsed OCI image reference.
///
/// Grammar roughly follows the OCI Distribution Spec:
///
/// ```text
/// reference  := [registry "/"] repository [":" tag] ["@" digest]
/// registry   := hostname [":" port]
/// repository := name ("/" name)*
/// tag        := [A-Za-z0-9_][A-Za-z0-9_.-]{0,127}
/// digest     := algorithm ":" hex
/// ```
///
/// # Examples
///
/// ```
/// use sera_oci::OciReference;
/// let r = OciReference::parse("ghcr.io/org/my-plugin:1.0.0").unwrap();
/// assert_eq!(r.registry, "ghcr.io");
/// assert_eq!(r.repository, "org/my-plugin");
/// assert_eq!(r.tag.as_deref(), Some("1.0.0"));
/// assert!(r.digest.is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciReference {
    /// Registry hostname (e.g. `ghcr.io`, `registry.example.com:5000`).
    pub registry: String,
    /// Repository path, possibly multi-segment (e.g. `org/sub/repo`).
    pub repository: String,
    /// Tag, mutually exclusive with [`digest`](Self::digest) when unambiguous.
    pub tag: Option<String>,
    /// Digest in `algo:hex` form (e.g. `sha256:deadbeef…`).
    pub digest: Option<String>,
}

impl OciReference {
    /// Parse a reference string.
    ///
    /// A reference is interpreted as `[registry/]repository[:tag][@digest]`.
    /// The first `/`-separated segment is treated as the registry if it
    /// contains a `.` or `:` or equals `localhost` — this matches the
    /// heuristic used by `docker pull` and the OCI spec. Otherwise the
    /// reference is prefixed with [`DEFAULT_REGISTRY`].
    ///
    /// # Errors
    ///
    /// Returns [`OciError::InvalidReference`] for malformed input.
    pub fn parse(raw: &str) -> Result<Self, OciError> {
        if raw.is_empty() {
            return Err(OciError::InvalidReference("empty reference".into()));
        }

        // Split off the digest first; a valid reference can only have one `@`.
        let (name_part, digest) = match raw.split_once('@') {
            Some((name, digest)) => {
                validate_digest(digest)?;
                (name, Some(digest.to_string()))
            }
            None => (raw, None),
        };

        if name_part.is_empty() {
            return Err(OciError::InvalidReference(format!(
                "missing repository in '{raw}'"
            )));
        }

        // A tag separator is the *last* colon that appears after the last `/`.
        // Colons inside the registry (port) must not be confused with a tag.
        let (name_no_tag, tag) = split_name_tag(name_part)?;

        // Split name_no_tag into registry + repository. If no separator, or
        // the first segment does not look like a hostname, assume default
        // registry.
        let (registry, repository) = match name_no_tag.split_once('/') {
            Some((first, rest)) if looks_like_registry(first) => {
                (first.to_string(), rest.to_string())
            }
            _ => (DEFAULT_REGISTRY.to_string(), name_no_tag.to_string()),
        };

        if repository.is_empty() {
            return Err(OciError::InvalidReference(format!(
                "missing repository in '{raw}'"
            )));
        }

        // At least one of tag or digest must be present for a pull to make
        // sense, but we allow bare `registry/repo` references too — callers
        // pulling a tagless ref will hit the registry's default behaviour.
        Ok(Self {
            registry,
            repository,
            tag,
            digest,
        })
    }

    /// Rebuild the canonical reference string.
    pub fn to_canonical(&self) -> String {
        let mut out = format!("{}/{}", self.registry, self.repository);
        if let Some(tag) = &self.tag {
            out.push(':');
            out.push_str(tag);
        }
        if let Some(digest) = &self.digest {
            out.push('@');
            out.push_str(digest);
        }
        out
    }
}

impl fmt::Display for OciReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_canonical())
    }
}

/// A host segment is registry-shaped if it contains a `.` (FQDN), a `:`
/// (port), or is exactly `localhost`. This mirrors the heuristic used by the
/// Docker/OCI CLI ecosystem.
fn looks_like_registry(segment: &str) -> bool {
    segment == "localhost" || segment.contains('.') || segment.contains(':')
}

/// Split `foo/bar:tag` into (`foo/bar`, Some("tag")). A colon inside a
/// `registry:port` segment (before the first `/`) is not a tag separator.
fn split_name_tag(input: &str) -> Result<(&str, Option<String>), OciError> {
    // Find the last `/` — everything after it may legitimately contain `:tag`.
    let search_from = input.rfind('/').map(|i| i + 1).unwrap_or(0);
    if let Some(rel_colon) = input[search_from..].find(':') {
        let colon = search_from + rel_colon;
        let name = &input[..colon];
        let tag = &input[colon + 1..];
        if tag.is_empty() {
            return Err(OciError::InvalidReference(format!(
                "empty tag in '{input}'"
            )));
        }
        Ok((name, Some(tag.to_string())))
    } else {
        Ok((input, None))
    }
}

/// Validate a digest of the form `algorithm:hex`.
fn validate_digest(digest: &str) -> Result<(), OciError> {
    match digest.split_once(':') {
        Some((algo, hex)) if !algo.is_empty() && !hex.is_empty() => Ok(()),
        _ => Err(OciError::InvalidReference(format!(
            "malformed digest '{digest}' — expected 'algo:hex'"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_tag() {
        let r = OciReference::parse("ghcr.io/org/my-plugin:1.0.0").unwrap();
        assert_eq!(r.registry, "ghcr.io");
        assert_eq!(r.repository, "org/my-plugin");
        assert_eq!(r.tag.as_deref(), Some("1.0.0"));
        assert!(r.digest.is_none());
    }

    #[test]
    fn parses_digest() {
        let digest = "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        let raw = format!("ghcr.io/org/my-plugin@{digest}");
        let r = OciReference::parse(&raw).unwrap();
        assert_eq!(r.registry, "ghcr.io");
        assert_eq!(r.repository, "org/my-plugin");
        assert!(r.tag.is_none());
        assert_eq!(r.digest.as_deref(), Some(digest));
    }

    #[test]
    fn parses_tag_and_digest() {
        let r = OciReference::parse("ghcr.io/a/b:1.0@sha256:deadbeef").unwrap();
        assert_eq!(r.tag.as_deref(), Some("1.0"));
        assert_eq!(r.digest.as_deref(), Some("sha256:deadbeef"));
    }

    #[test]
    fn defaults_registry_when_missing() {
        let r = OciReference::parse("my-plugin:1.0").unwrap();
        assert_eq!(r.registry, DEFAULT_REGISTRY);
        assert_eq!(r.repository, "my-plugin");
        assert_eq!(r.tag.as_deref(), Some("1.0"));
    }

    #[test]
    fn handles_multi_segment_repository() {
        let r = OciReference::parse("ghcr.io/org/sub/repo:1.0").unwrap();
        assert_eq!(r.registry, "ghcr.io");
        assert_eq!(r.repository, "org/sub/repo");
        assert_eq!(r.tag.as_deref(), Some("1.0"));
    }

    #[test]
    fn handles_registry_with_port() {
        let r = OciReference::parse("registry.local:5000/team/app:dev").unwrap();
        assert_eq!(r.registry, "registry.local:5000");
        assert_eq!(r.repository, "team/app");
        assert_eq!(r.tag.as_deref(), Some("dev"));
    }

    #[test]
    fn handles_localhost() {
        let r = OciReference::parse("localhost/app:1").unwrap();
        assert_eq!(r.registry, "localhost");
        assert_eq!(r.repository, "app");
    }

    #[test]
    fn empty_reference_is_rejected() {
        assert!(OciReference::parse("").is_err());
    }

    #[test]
    fn empty_tag_is_rejected() {
        let err = OciReference::parse("ghcr.io/org/app:").unwrap_err();
        assert!(matches!(err, OciError::InvalidReference(_)));
    }

    #[test]
    fn malformed_digest_is_rejected() {
        let err = OciReference::parse("ghcr.io/org/app@notadigest").unwrap_err();
        assert!(matches!(err, OciError::InvalidReference(_)));
    }

    #[test]
    fn display_round_trips_tag() {
        let raw = "ghcr.io/org/my-plugin:1.0.0";
        let r = OciReference::parse(raw).unwrap();
        assert_eq!(r.to_string(), raw);
    }

    #[test]
    fn display_round_trips_digest() {
        let raw = "ghcr.io/org/my-plugin@sha256:deadbeef";
        let r = OciReference::parse(raw).unwrap();
        assert_eq!(r.to_string(), raw);
    }

    #[test]
    fn display_round_trips_default_registry() {
        // When the user omits the registry, `to_canonical` normalises it in.
        let r = OciReference::parse("my-plugin:1.0").unwrap();
        assert_eq!(r.to_string(), format!("{DEFAULT_REGISTRY}/my-plugin:1.0"));
    }
}
