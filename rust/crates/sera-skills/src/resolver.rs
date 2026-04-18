//! [`SkillResolver`] — fans out `SkillRef` lookups across a prioritized list
//! of [`SkillSource`]s.
//!
//! Semantics:
//!   * `resolve_batch` walks `sources` in construction order per ref; the
//!     **first** source that returns `Ok(_)` wins. Resolution errors other
//!     than `SkillNotFound` / `Unavailable` / `Unsupported` short-circuit the
//!     batch.
//!   * `search` fans out concurrently across every source and accumulates
//!     hits in source-priority order (sources earlier in the list win
//!     position).
//!   * If the `SkillRef` carries a `source_hint`, only matching sources are
//!     consulted.

use std::path::PathBuf;
use std::sync::Arc;

use futures_util::future::join_all;
use tracing::debug;

use sera_oci::OciPuller;
use sera_plugins::PluginRegistry;

use crate::error::SkillsError;
use crate::skill_ref::SkillRef;
use crate::source::{ResolvedSkill, SkillSearchHit, SkillSource};
use crate::sources::{FileSystemSource, OciSkillPuller, PluginSource, RegistrySource};

/// A resolver that tries each of its sources in priority order.
pub struct SkillResolver {
    sources: Vec<Arc<dyn SkillSource>>,
}

impl SkillResolver {
    pub fn new(sources: Vec<Arc<dyn SkillSource>>) -> Self {
        Self { sources }
    }

    pub fn builder() -> SkillResolverBuilder {
        SkillResolverBuilder::default()
    }

    /// Borrow the configured sources.
    pub fn sources(&self) -> &[Arc<dyn SkillSource>] {
        &self.sources
    }

    /// Resolve a batch of references. Misses are collected rather than
    /// aborting the whole batch.
    pub async fn resolve_batch(
        &self,
        refs: &[SkillRef],
    ) -> Result<ResolvedSkillBundle, SkillsError> {
        let mut skills = Vec::with_capacity(refs.len());
        let mut misses = Vec::new();

        for r in refs {
            let mut resolved = None;
            for source in self.sources_for(r) {
                match source.resolve(r).await {
                    Ok(v) => {
                        resolved = Some(v);
                        break;
                    }
                    Err(SkillsError::SkillNotFound(_))
                    | Err(SkillsError::NotFound(_))
                    | Err(SkillsError::Unavailable { .. })
                    | Err(SkillsError::Unsupported { .. }) => {
                        debug!(?r, "source returned soft-miss; trying next");
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
            match resolved {
                Some(r) => skills.push(r),
                None => misses.push(r.clone()),
            }
        }

        Ok(ResolvedSkillBundle { skills, misses })
    }

    /// Search across all sources in parallel.
    pub async fn search(&self, query: &str) -> Vec<SkillSearchHit> {
        let futures = self
            .sources
            .iter()
            .map(|s| {
                let s = Arc::clone(s);
                let q = query.to_string();
                async move { (s.kind(), s.search(&q).await) }
            })
            .collect::<Vec<_>>();

        let results = join_all(futures).await;
        let mut hits = Vec::new();
        for (kind, res) in results {
            match res {
                Ok(mut h) => hits.append(&mut h),
                Err(e) => debug!(?kind, error = %e, "source search failed"),
            }
        }
        hits
    }

    fn sources_for<'a>(&'a self, skill_ref: &'a SkillRef) -> Vec<&'a Arc<dyn SkillSource>> {
        match skill_ref.source_hint {
            Some(hint) => self
                .sources
                .iter()
                .filter(|s| s.kind() == hint)
                .collect(),
            None => self.sources.iter().collect(),
        }
    }
}

/// Builder for [`SkillResolver`] that wires up the three stock sources.
#[derive(Default)]
pub struct SkillResolverBuilder {
    sources: Vec<Arc<dyn SkillSource>>,
}

impl SkillResolverBuilder {
    pub fn with_filesystem(mut self, paths: Vec<PathBuf>) -> Self {
        self.sources.push(Arc::new(FileSystemSource::new(paths)));
        self
    }

    pub fn with_plugins(mut self, registry: Arc<dyn PluginRegistry>) -> Self {
        self.sources.push(Arc::new(PluginSource::new(registry)));
        self
    }

    pub fn with_registry(
        mut self,
        puller: Arc<OciPuller>,
        reference_template: String,
    ) -> Self {
        let puller: Arc<dyn OciSkillPuller> = puller;
        self.sources
            .push(Arc::new(RegistrySource::new(puller).with_template(reference_template)));
        self
    }

    /// Inject a custom source. Useful for tests or alternative backends.
    pub fn with_source(mut self, source: Arc<dyn SkillSource>) -> Self {
        self.sources.push(source);
        self
    }

    pub fn build(self) -> SkillResolver {
        SkillResolver::new(self.sources)
    }
}

/// Outcome of a batch resolve.
#[derive(Debug, Clone)]
pub struct ResolvedSkillBundle {
    pub skills: Vec<ResolvedSkill>,
    pub misses: Vec<SkillRef>,
}

impl ResolvedSkillBundle {
    pub fn is_complete(&self) -> bool {
        self.misses.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_ref::SkillSourceKind;
    use async_trait::async_trait;
    use sera_types::skill::SkillDefinition;
    use std::sync::Mutex;

    /// Programmable mock source — returns the preconfigured outcome for a
    /// ref name. Tracks which queries have been observed so tests can
    /// assert call order.
    struct MockSource {
        kind: SkillSourceKind,
        resolves: Vec<(String, Result<ResolvedSkill, SkillsError>)>,
        hits: Vec<SkillSearchHit>,
        calls: Mutex<Vec<String>>,
    }

    impl MockSource {
        fn new(kind: SkillSourceKind, _name: &str) -> Self {
            Self {
                kind,
                resolves: Vec::new(),
                hits: Vec::new(),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    fn def(name: &str, version: &str) -> SkillDefinition {
        SkillDefinition {
            name: name.into(),
            description: None,
            version: Some(version.into()),
            parameters: None,
            source: None,
            body: Some(format!("body for {name}")),
            triggers: vec![],
            model_override: None,
            context_budget_tokens: None,
            tool_bindings: vec![],
            mcp_servers: vec![],
        }
    }

    fn resolved(name: &str, version: &str, kind: SkillSourceKind, pack: &str) -> ResolvedSkill {
        ResolvedSkill {
            reference: SkillRef::parse(name).unwrap(),
            definition: def(name, version),
            pack_name: pack.into(),
            source: kind,
        }
    }

    #[async_trait]
    impl SkillSource for MockSource {
        fn kind(&self) -> SkillSourceKind {
            self.kind
        }

        async fn resolve(&self, skill_ref: &SkillRef) -> Result<ResolvedSkill, SkillsError> {
            self.calls.lock().unwrap().push(skill_ref.name.clone());
            for (needle, outcome) in &self.resolves {
                if *needle == skill_ref.name {
                    return match outcome {
                        Ok(v) => Ok(v.clone()),
                        Err(e) => Err(clone_error(e)),
                    };
                }
            }
            Err(SkillsError::SkillNotFound(skill_ref.name.clone()))
        }

        async fn search(&self, _query: &str) -> Result<Vec<SkillSearchHit>, SkillsError> {
            Ok(self.hits.clone())
        }
    }

    fn clone_error(e: &SkillsError) -> SkillsError {
        match e {
            SkillsError::SkillNotFound(n) => SkillsError::SkillNotFound(n.clone()),
            SkillsError::NotFound(n) => SkillsError::NotFound(n.clone()),
            SkillsError::Unavailable { source_kind, reason } => SkillsError::Unavailable {
                source_kind: *source_kind,
                reason: reason.clone(),
            },
            SkillsError::Unsupported { source_kind, reason } => SkillsError::Unsupported {
                source_kind: *source_kind,
                reason: reason.clone(),
            },
            other => SkillsError::LoadFailed(other.to_string()),
        }
    }

    fn hit(name: &str, source: SkillSourceKind, pack: &str) -> SkillSearchHit {
        SkillSearchHit {
            name: name.into(),
            version: "1.0.0".into(),
            description: "desc".into(),
            source,
            pack_name: pack.into(),
        }
    }

    #[tokio::test]
    async fn resolve_batch_first_source_wins() {
        let mut a = MockSource::new(SkillSourceKind::Fs, "a");
        a.resolves.push((
            "triage".into(),
            Ok(resolved("triage", "1.0.0", SkillSourceKind::Fs, "first")),
        ));
        let mut b = MockSource::new(SkillSourceKind::Registry, "b");
        b.resolves.push((
            "triage".into(),
            Ok(resolved("triage", "1.0.0", SkillSourceKind::Registry, "second")),
        ));

        let resolver = SkillResolver::new(vec![Arc::new(a), Arc::new(b)]);
        let refs = vec![SkillRef::parse("triage").unwrap()];
        let bundle = resolver.resolve_batch(&refs).await.unwrap();
        assert_eq!(bundle.skills.len(), 1);
        assert_eq!(bundle.skills[0].source, SkillSourceKind::Fs);
        assert!(bundle.is_complete());
    }

    #[tokio::test]
    async fn resolve_batch_falls_through_on_soft_miss() {
        let a = MockSource::new(SkillSourceKind::Fs, "a"); // returns SkillNotFound
        let mut b = MockSource::new(SkillSourceKind::Registry, "b");
        b.resolves.push((
            "triage".into(),
            Ok(resolved("triage", "1.0.0", SkillSourceKind::Registry, "second")),
        ));

        let resolver = SkillResolver::new(vec![Arc::new(a), Arc::new(b)]);
        let bundle = resolver
            .resolve_batch(&[SkillRef::parse("triage").unwrap()])
            .await
            .unwrap();
        assert_eq!(bundle.skills.len(), 1);
        assert_eq!(bundle.skills[0].source, SkillSourceKind::Registry);
    }

    #[tokio::test]
    async fn resolve_batch_collects_misses() {
        let a = MockSource::new(SkillSourceKind::Fs, "a");
        let resolver = SkillResolver::new(vec![Arc::new(a)]);
        let refs = vec![
            SkillRef::parse("ghost").unwrap(),
            SkillRef::parse("spectre").unwrap(),
        ];
        let bundle = resolver.resolve_batch(&refs).await.unwrap();
        assert!(bundle.skills.is_empty());
        assert_eq!(bundle.misses.len(), 2);
        assert!(!bundle.is_complete());
    }

    #[tokio::test]
    async fn resolve_batch_respects_source_hint() {
        let a = MockSource::new(SkillSourceKind::Fs, "a");
        let mut b = MockSource::new(SkillSourceKind::Registry, "b");
        b.resolves.push((
            "triage".into(),
            Ok(resolved("triage", "1.0.0", SkillSourceKind::Registry, "second")),
        ));

        let resolver = SkillResolver::new(vec![Arc::new(a), Arc::new(b)]);
        // Hint forces registry-only lookup even though Fs is first.
        let refs = vec![SkillRef::parse("registry:triage").unwrap()];
        let bundle = resolver.resolve_batch(&refs).await.unwrap();
        assert_eq!(bundle.skills.len(), 1);
        assert_eq!(bundle.skills[0].source, SkillSourceKind::Registry);
    }

    #[tokio::test]
    async fn resolve_batch_propagates_hard_errors() {
        let mut a = MockSource::new(SkillSourceKind::Fs, "a");
        a.resolves.push((
            "triage".into(),
            Err(SkillsError::LoadFailed("disk on fire".into())),
        ));
        let resolver = SkillResolver::new(vec![Arc::new(a)]);
        let err = resolver
            .resolve_batch(&[SkillRef::parse("triage").unwrap()])
            .await
            .unwrap_err();
        assert!(matches!(err, SkillsError::LoadFailed(_)));
    }

    #[tokio::test]
    async fn search_accumulates_hits_across_sources_in_priority_order() {
        let mut a = MockSource::new(SkillSourceKind::Fs, "a");
        a.hits.push(hit("triage", SkillSourceKind::Fs, "fs-pack"));
        let mut b = MockSource::new(SkillSourceKind::Plugin, "b");
        b.hits.push(hit("deploy", SkillSourceKind::Plugin, "plug"));

        let resolver = SkillResolver::new(vec![Arc::new(a), Arc::new(b)]);
        let hits = resolver.search("").await;
        assert_eq!(hits.len(), 2);
        // First source's hits come first.
        assert_eq!(hits[0].source, SkillSourceKind::Fs);
        assert_eq!(hits[1].source, SkillSourceKind::Plugin);
    }

    #[tokio::test]
    async fn search_tolerates_source_errors() {
        struct FailingSource;
        #[async_trait]
        impl SkillSource for FailingSource {
            fn kind(&self) -> SkillSourceKind {
                SkillSourceKind::Registry
            }
            async fn resolve(&self, _r: &SkillRef) -> Result<ResolvedSkill, SkillsError> {
                unreachable!()
            }
            async fn search(&self, _q: &str) -> Result<Vec<SkillSearchHit>, SkillsError> {
                Err(SkillsError::Unsupported {
                    source_kind: SkillSourceKind::Registry,
                    reason: "phase L".into(),
                })
            }
        }

        let mut a = MockSource::new(SkillSourceKind::Fs, "a");
        a.hits.push(hit("triage", SkillSourceKind::Fs, "fs-pack"));
        let resolver = SkillResolver::new(vec![Arc::new(a), Arc::new(FailingSource)]);
        let hits = resolver.search("triage").await;
        assert_eq!(hits.len(), 1);
    }

    #[tokio::test]
    async fn builder_with_source_threads_through() {
        let a = MockSource::new(SkillSourceKind::Fs, "a");
        let resolver = SkillResolver::builder().with_source(Arc::new(a)).build();
        assert_eq!(resolver.sources().len(), 1);
    }
}
