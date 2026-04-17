//! OCI registry-backed [`SkillSource`].
//!
//! # Phase 5 scope
//!
//! * `resolve` requires an **exact** version pin (`=X.Y.Z`). Range queries
//!   return `Unsupported { source: Registry, reason: "…" }` — a future OCI
//!   index lookup lands in phase L.
//! * `search` is not implementable without a distribution-spec search
//!   endpoint and returns `Unsupported` for now.
//!
//! The reference template lets operators point at private registries, e.g.:
//!
//! ```text
//! "ghcr.io/sera-skills/{name}:{version}"
//! "registry.example.com/skills/{name}:{version}"
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use tracing::debug;

use sera_oci::{OciError, OciPuller, OciReference};

use crate::error::SkillsError;
use crate::markdown::parse_skill_markdown_str;
use crate::skill_ref::{SkillRef, SkillSourceKind};
use crate::source::{ResolvedSkill, SkillSearchHit, SkillSource};

/// Default reference template — resolves to GHCR where SERA publishes its
/// first-party skill packs.
pub const DEFAULT_REFERENCE_TEMPLATE: &str = "ghcr.io/sera-skills/{name}:{version}";

/// Abstracted manifest puller so tests can inject a mock without touching
/// real network traffic.
///
/// The default implementation is `Arc<OciPuller>`.
#[async_trait]
pub trait OciSkillPuller: Send + Sync + 'static {
    async fn pull_manifest_yaml(&self, reference: &OciReference) -> Result<Vec<u8>, OciError>;
}

#[async_trait]
impl OciSkillPuller for OciPuller {
    async fn pull_manifest_yaml(&self, reference: &OciReference) -> Result<Vec<u8>, OciError> {
        let bytes = self.pull_manifest(reference).await?;
        Ok(bytes.into_vec())
    }
}

/// A `SkillSource` that pulls skill markdown from an OCI registry.
///
/// Construction does not touch the network.
#[derive(Clone)]
pub struct RegistrySource {
    puller: Arc<dyn OciSkillPuller>,
    reference_template: String,
}

impl RegistrySource {
    /// Create a new registry source using the default GHCR template.
    pub fn new(puller: Arc<dyn OciSkillPuller>) -> Self {
        Self {
            puller,
            reference_template: DEFAULT_REFERENCE_TEMPLATE.to_string(),
        }
    }

    /// Override the reference template. The string must contain the
    /// placeholders `{name}` and `{version}`.
    pub fn with_template(mut self, template: impl Into<String>) -> Self {
        self.reference_template = template.into();
        self
    }

    fn build_reference(&self, name: &str, version: &str) -> Result<OciReference, SkillsError> {
        let raw = self
            .reference_template
            .replace("{name}", name)
            .replace("{version}", version);
        OciReference::parse(&raw).map_err(|e| {
            SkillsError::InvalidReference(format!(
                "registry template expanded to invalid OCI reference '{raw}': {e}"
            ))
        })
    }
}

#[async_trait]
impl SkillSource for RegistrySource {
    fn kind(&self) -> SkillSourceKind {
        SkillSourceKind::Registry
    }

    async fn resolve(&self, skill_ref: &SkillRef) -> Result<ResolvedSkill, SkillsError> {
        let Some(exact) = skill_ref.exact_version() else {
            return Err(SkillsError::Unsupported {
                source_kind: SkillSourceKind::Registry,
                reason: "registry source requires exact version (=x.y.z); fuzzy ranges need an \
                         OCI search index (phase L)"
                    .into(),
            });
        };

        let version = exact.to_string();
        let oci_ref = self.build_reference(&skill_ref.name, &version)?;
        debug!(reference = %oci_ref, "pulling skill markdown from OCI");

        let bytes = self
            .puller
            .pull_manifest_yaml(&oci_ref)
            .await
            .map_err(|e| SkillsError::Oci(e.to_string()))?;

        let raw = String::from_utf8(bytes).map_err(|e| {
            SkillsError::InvalidFormat(format!("registry layer is not valid UTF-8: {e}"))
        })?;

        let parsed = parse_skill_markdown_str(&raw, std::path::PathBuf::from(oci_ref.to_string()))?;

        Ok(ResolvedSkill {
            reference: skill_ref.clone(),
            definition: parsed.definition,
            pack_name: oci_ref.to_string(),
            source: SkillSourceKind::Registry,
        })
    }

    async fn search(&self, _query: &str) -> Result<Vec<SkillSearchHit>, SkillsError> {
        Err(SkillsError::Unsupported {
            source_kind: SkillSourceKind::Registry,
            reason: "registry search requires an OCI distribution-spec search index (phase L)"
                .into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct MockPuller {
        payload: Vec<u8>,
        seen: Mutex<Vec<String>>,
    }

    impl MockPuller {
        fn new(payload: &str) -> Self {
            Self {
                payload: payload.as_bytes().to_vec(),
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl OciSkillPuller for MockPuller {
        async fn pull_manifest_yaml(&self, reference: &OciReference) -> Result<Vec<u8>, OciError> {
            self.seen.lock().unwrap().push(reference.to_string());
            Ok(self.payload.clone())
        }
    }

    const CANNED_SKILL: &str = "---\nname: triage\nversion: 1.0.0\ndescription: Route incident tickets\n---\nBody from registry.\n";

    #[tokio::test]
    async fn exact_version_resolves_via_mock_puller() {
        let puller = Arc::new(MockPuller::new(CANNED_SKILL));
        let src = RegistrySource::new(puller.clone()).with_template("ghcr.io/org/{name}:{version}");

        let r = SkillRef::parse("triage@=1.0.0").unwrap();
        let resolved = src.resolve(&r).await.unwrap();
        assert_eq!(resolved.definition.name, "triage");
        assert!(resolved
            .definition
            .body
            .as_deref()
            .unwrap()
            .contains("Body from registry"));
        assert_eq!(resolved.source, SkillSourceKind::Registry);

        let seen = puller.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].contains("triage"));
        assert!(seen[0].contains("1.0.0"));
    }

    #[tokio::test]
    async fn range_version_is_refused() {
        let puller = Arc::new(MockPuller::new(CANNED_SKILL));
        let src = RegistrySource::new(puller);
        let r = SkillRef::parse("triage@^1").unwrap();
        let err = src.resolve(&r).await.unwrap_err();
        match err {
            SkillsError::Unsupported { source_kind, reason } => {
                assert_eq!(source_kind, SkillSourceKind::Registry);
                assert!(reason.contains("exact"));
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn missing_version_is_refused() {
        let puller = Arc::new(MockPuller::new(CANNED_SKILL));
        let src = RegistrySource::new(puller);
        let r = SkillRef::parse("triage").unwrap();
        let err = src.resolve(&r).await.unwrap_err();
        assert!(matches!(err, SkillsError::Unsupported { .. }));
    }

    #[tokio::test]
    async fn search_returns_unsupported() {
        let puller = Arc::new(MockPuller::new(CANNED_SKILL));
        let src = RegistrySource::new(puller);
        let err = src.search("anything").await.unwrap_err();
        assert!(matches!(err, SkillsError::Unsupported { .. }));
    }

    #[tokio::test]
    async fn template_expansion_produces_valid_reference() {
        let puller = Arc::new(MockPuller::new(CANNED_SKILL));
        let src = RegistrySource::new(puller.clone())
            .with_template("registry.example.com/skills/{name}:{version}");

        let r = SkillRef::parse("triage@=2.1.3").unwrap();
        src.resolve(&r).await.unwrap();
        let seen = puller.seen.lock().unwrap();
        assert!(seen[0].contains("registry.example.com"));
        assert!(seen[0].contains("2.1.3"));
    }

    // --- additional gap-filling tests ---

    #[tokio::test]
    async fn source_kind_is_registry() {
        let puller = Arc::new(MockPuller::new(CANNED_SKILL));
        let src = RegistrySource::new(puller);
        assert_eq!(src.kind(), SkillSourceKind::Registry);
    }

    #[tokio::test]
    async fn non_utf8_bytes_from_puller_yield_error() {
        // Construct a puller that returns bytes that are not valid UTF-8.
        struct BadUtf8Puller;
        #[async_trait]
        impl OciSkillPuller for BadUtf8Puller {
            async fn pull_manifest_yaml(
                &self,
                _reference: &OciReference,
            ) -> Result<Vec<u8>, OciError> {
                // 0xFF 0xFE are not valid UTF-8 continuations.
                Ok(vec![0xFF, 0xFE, 0x00])
            }
        }
        let src = RegistrySource::new(Arc::new(BadUtf8Puller))
            .with_template("ghcr.io/org/{name}:{version}");
        let r = SkillRef::parse("triage@=1.0.0").unwrap();
        let err = src.resolve(&r).await.unwrap_err();
        match err {
            SkillsError::InvalidFormat(msg) => {
                assert!(msg.contains("UTF-8"), "error should mention UTF-8");
            }
            other => panic!("expected InvalidFormat, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn puller_error_is_propagated_as_oci_error() {
        struct FailingPuller;
        #[async_trait]
        impl OciSkillPuller for FailingPuller {
            async fn pull_manifest_yaml(
                &self,
                _reference: &OciReference,
            ) -> Result<Vec<u8>, OciError> {
                Err(OciError::NotFound("triage:1.0.0".into()))
            }
        }
        let src = RegistrySource::new(Arc::new(FailingPuller))
            .with_template("ghcr.io/org/{name}:{version}");
        let r = SkillRef::parse("triage@=1.0.0").unwrap();
        let err = src.resolve(&r).await.unwrap_err();
        assert!(matches!(err, SkillsError::Oci(_)));
    }

    #[tokio::test]
    async fn malformed_markdown_from_registry_yields_format_error() {
        // Puller returns bytes that are valid UTF-8 but not valid skill markdown.
        let puller = Arc::new(MockPuller::new("not a skill file at all — no frontmatter"));
        let src = RegistrySource::new(puller).with_template("ghcr.io/org/{name}:{version}");
        let r = SkillRef::parse("triage@=1.0.0").unwrap();
        let err = src.resolve(&r).await.unwrap_err();
        assert!(
            matches!(err, SkillsError::Format(_)),
            "expected Format error for missing frontmatter, got {err:?}"
        );
    }

    #[tokio::test]
    async fn resolved_skill_pack_name_matches_oci_ref() {
        let puller = Arc::new(MockPuller::new(CANNED_SKILL));
        let src = RegistrySource::new(puller).with_template("ghcr.io/org/{name}:{version}");
        let r = SkillRef::parse("triage@=1.0.0").unwrap();
        let resolved = src.resolve(&r).await.unwrap();
        assert!(resolved.pack_name.contains("triage"));
        assert!(resolved.pack_name.contains("1.0.0"));
    }
}
