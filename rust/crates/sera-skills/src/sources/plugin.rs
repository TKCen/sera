//! Plugin-advertised [`SkillSource`].
//!
//! # Phase 5 scope
//!
//! The full plugin RPC for skill advertisement lands in phase M. On phase 5
//! we keep a well-defined surface so callers can build [`SkillResolver`]s
//! that include a plugin source today — they will automatically light up
//! once plugin side RPCs exist.
//!
//! For now:
//!   * `resolve` attempts to locate a plugin whose capabilities include a
//!     `Custom("SkillProvider")` tag. If none is registered, returns
//!     `NotFound`. If one is registered, returns a clean `Unavailable` error
//!     pointing at the phase-M gap rather than panicking.
//!   * `search` returns an empty list with a debug log.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::debug;

use sera_plugins::{PluginCapability, PluginRegistry};

use crate::error::SkillsError;
use crate::skill_ref::{SkillRef, SkillSourceKind};
use crate::source::{ResolvedSkill, SkillSearchHit, SkillSource};

/// Capability tag that plugins use to opt into skill advertisement.
///
/// Kept as a `Custom` variant so this lane does not force new entries on
/// the canonical `PluginCapability` enum (which is phase-M territory).
pub const SKILL_PROVIDER_CAPABILITY: &str = "SkillProvider";

/// A `SkillSource` backed by a [`PluginRegistry`].
///
/// Phase-5 stub — see module docs.
#[derive(Clone)]
pub struct PluginSource {
    registry: Arc<dyn PluginRegistry>,
}

impl PluginSource {
    pub fn new(registry: Arc<dyn PluginRegistry>) -> Self {
        Self { registry }
    }

    fn provider_cap() -> PluginCapability {
        PluginCapability::Custom(SKILL_PROVIDER_CAPABILITY.to_string())
    }
}

#[async_trait]
impl SkillSource for PluginSource {
    fn kind(&self) -> SkillSourceKind {
        SkillSourceKind::Plugin
    }

    async fn resolve(&self, skill_ref: &SkillRef) -> Result<ResolvedSkill, SkillsError> {
        let providers = self
            .registry
            .find_by_capability(&Self::provider_cap())
            .await;
        if providers.is_empty() {
            debug!(
                ?skill_ref,
                "no plugin advertises SkillProvider capability; cannot resolve"
            );
            return Err(SkillsError::SkillNotFound(skill_ref.name.clone()));
        }
        Err(SkillsError::Unavailable {
            source_kind: SkillSourceKind::Plugin,
            reason: "plugin skill advertisement RPC not yet implemented (phase M)".into(),
        })
    }

    async fn search(&self, _query: &str) -> Result<Vec<SkillSearchHit>, SkillsError> {
        debug!("PluginSource::search is a phase-M stub; returning empty hit list");
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use sera_plugins::{
        GrpcTransportConfig, PluginError, PluginHealth, PluginInfo, PluginRegistration,
        PluginTransport, PluginVersion,
    };
    use std::sync::Mutex;
    use std::time::Duration;

    /// Minimal registry mock: returns whatever we seed it with.
    struct MockRegistry {
        entries: Mutex<Vec<(PluginCapability, Vec<PluginInfo>)>>,
    }

    impl MockRegistry {
        fn new() -> Self {
            Self {
                entries: Mutex::new(Vec::new()),
            }
        }
        fn seed(&self, cap: PluginCapability, infos: Vec<PluginInfo>) {
            self.entries.lock().unwrap().push((cap, infos));
        }
    }

    #[async_trait]
    impl PluginRegistry for MockRegistry {
        async fn register(&self, _r: PluginRegistration) -> Result<(), PluginError> {
            Ok(())
        }
        async fn deregister(&self, _n: &str) -> Result<(), PluginError> {
            Ok(())
        }
        async fn get(&self, name: &str) -> Result<PluginInfo, PluginError> {
            Err(PluginError::PluginNotFound { name: name.into() })
        }
        async fn list(&self) -> Vec<PluginInfo> {
            Vec::new()
        }
        async fn find_by_capability(&self, cap: &PluginCapability) -> Vec<PluginInfo> {
            self.entries
                .lock()
                .unwrap()
                .iter()
                .find(|(c, _)| c == cap)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        }
        async fn update_health(&self, _n: &str, _h: PluginHealth) -> Result<(), PluginError> {
            Ok(())
        }
    }

    fn make_info(name: &str) -> PluginInfo {
        PluginInfo::new(PluginRegistration {
            name: name.into(),
            version: PluginVersion::new(1, 0, 0),
            capabilities: vec![PluginCapability::Custom(
                SKILL_PROVIDER_CAPABILITY.to_string(),
            )],
            transport: PluginTransport::Grpc {
                grpc: GrpcTransportConfig {
                    endpoint: "localhost:9000".into(),
                    tls: None,
                },
            },
            health_check_interval: Duration::from_secs(30),
        })
    }

    #[tokio::test]
    async fn resolve_returns_not_found_when_no_providers() {
        let registry = Arc::new(MockRegistry::new());
        let src = PluginSource::new(registry);
        let r = SkillRef::parse("anything").unwrap();
        let err = src.resolve(&r).await.unwrap_err();
        assert!(matches!(err, SkillsError::SkillNotFound(_)));
    }

    #[tokio::test]
    async fn resolve_returns_unavailable_when_provider_registered() {
        let registry = MockRegistry::new();
        registry.seed(
            PluginCapability::Custom(SKILL_PROVIDER_CAPABILITY.to_string()),
            vec![make_info("skillful-plugin")],
        );
        let src = PluginSource::new(Arc::new(registry));
        let r = SkillRef::parse("triage").unwrap();
        let err = src.resolve(&r).await.unwrap_err();
        match err {
            SkillsError::Unavailable { source_kind, reason } => {
                assert_eq!(source_kind, SkillSourceKind::Plugin);
                assert!(reason.contains("phase M"));
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn search_returns_empty() {
        let registry = Arc::new(MockRegistry::new());
        let src = PluginSource::new(registry);
        let hits = src.search("anything").await.unwrap();
        assert!(hits.is_empty());
    }
}
