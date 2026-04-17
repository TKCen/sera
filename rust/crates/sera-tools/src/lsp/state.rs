//! Shared state for LSP-backed tools.
//!
//! `LspToolsState` is injected into every tool invocation — there is no
//! global singleton. It owns the per-language supervisors, the registry,
//! the symbol cache, and the per-request timeout budget.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use super::cache::SymbolCache;
use super::error::LspError;
use super::registry::LspServerRegistry;
use super::supervisor::LspProcessSupervisor;

/// Default per-request timeout — matches the bead spec.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Owned state for the LSP tools subsystem.
///
/// Cheap to clone — the supervisor map lives inside an `Arc<RwLock<..>>` and
/// every other field is already `Arc`- or value-typed.
#[derive(Debug, Clone)]
pub struct LspToolsState {
    /// Supervisors keyed by `language_id`. Lazily populated on first
    /// `get_or_spawn` call. Kept in a read-write lock because one spawn under
    /// contention must not block concurrent readers that are pulling cached
    /// supervisors for other languages.
    pub supervisors: Arc<RwLock<HashMap<String, Arc<LspProcessSupervisor>>>>,
    pub registry: LspServerRegistry,
    pub cache: SymbolCache,
    /// Per-request timeout applied to `textDocument/documentSymbol` and other
    /// single-shot LSP calls. Default: [`DEFAULT_REQUEST_TIMEOUT`].
    pub request_timeout: Duration,
}

impl Default for LspToolsState {
    fn default() -> Self {
        Self {
            supervisors: Arc::new(RwLock::new(HashMap::new())),
            registry: LspServerRegistry::default(),
            cache: SymbolCache::default(),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }
}

impl LspToolsState {
    pub fn new(registry: LspServerRegistry) -> Self {
        Self {
            supervisors: Arc::new(RwLock::new(HashMap::new())),
            registry,
            cache: SymbolCache::new(),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    /// SERA default: Rust-only registry, empty supervisor map, fresh cache.
    pub fn with_defaults() -> Self {
        Self::new(LspServerRegistry::with_defaults())
    }

    /// Resolve a supervisor for `language_id`, spawning one lazily if none
    /// exists yet. Concurrent callers for the same language may race to spawn;
    /// the loser's supervisor is shut down to avoid a process leak.
    ///
    /// Errors with [`LspError::Unsupported`] if the language has no entry in
    /// the registry, or surfaces whatever [`LspProcessSupervisor::new`] /
    /// [`LspProcessSupervisor::initialize`] raise.
    pub async fn get_or_spawn(
        &self,
        language_id: &str,
        project_root: &Path,
    ) -> Result<Arc<LspProcessSupervisor>, LspError> {
        // Fast path — shared read lock, no spawn.
        {
            let guard = self.supervisors.read().await;
            if let Some(existing) = guard.get(language_id)
                && existing.is_healthy()
            {
                return Ok(existing.clone());
            }
        }

        // Slow path — resolve config + spawn before taking the write lock so
        // that other languages can keep progressing.
        let config = self
            .registry
            .get(language_id)
            .cloned()
            .ok_or_else(|| LspError::Unsupported {
                language: language_id.to_string(),
            })?;
        let supervisor = Arc::new(LspProcessSupervisor::new(&config).await?);
        supervisor.initialize(project_root).await?;

        // Re-check under the write lock in case a concurrent caller beat us.
        let mut guard = self.supervisors.write().await;
        if let Some(existing) = guard.get(language_id)
            && existing.is_healthy()
        {
            // Peer won the race — tear down our freshly-spawned instance.
            let _ = supervisor.shutdown().await;
            return Ok(existing.clone());
        }
        guard.insert(language_id.to_string(), supervisor.clone());
        Ok(supervisor)
    }
}

/// Normalize a caller-supplied `relative_path` against `project_root`.
///
/// Rejects any path whose component sequence escapes `project_root` via `..`
/// or starts with a root component (absolute path). Returns the joined
/// absolute path on success.
///
/// * Rejects: `../etc/passwd`, `foo/../../escape`, `/abs/path`
/// * Allows: `src/lib.rs`, `crates/sera-tools/src/lsp/mod.rs`, `./src/lib.rs`
pub fn normalize_path(project_root: &Path, relative_path: &Path) -> Result<PathBuf, LspError> {
    if relative_path.is_absolute() {
        return Err(LspError::PathTraversal);
    }
    let mut out = PathBuf::new();
    let mut depth: i64 = 0;
    for comp in relative_path.components() {
        match comp {
            Component::CurDir => continue,
            Component::RootDir | Component::Prefix(_) => return Err(LspError::PathTraversal),
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err(LspError::PathTraversal);
                }
                out.pop();
            }
            Component::Normal(c) => {
                depth += 1;
                out.push(c);
            }
        }
    }
    Ok(project_root.join(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_simple_relative_path() {
        let root = Path::new("/tmp/proj");
        let got = normalize_path(root, Path::new("src/lib.rs")).unwrap();
        assert_eq!(got, PathBuf::from("/tmp/proj/src/lib.rs"));
    }

    #[test]
    fn normalize_accepts_curdir_prefix() {
        let root = Path::new("/tmp/proj");
        let got = normalize_path(root, Path::new("./src/lib.rs")).unwrap();
        assert_eq!(got, PathBuf::from("/tmp/proj/src/lib.rs"));
    }

    #[test]
    fn normalize_rejects_parent_escape() {
        let root = Path::new("/tmp/proj");
        // table-driven
        let bad_inputs: &[&str] = &[
            "../etc/passwd",
            "foo/../../escape",
            "../..",
            "../sibling/lib.rs",
        ];
        for bad in bad_inputs {
            let err = normalize_path(root, Path::new(bad)).expect_err(bad);
            assert!(matches!(err, LspError::PathTraversal), "bad case: {bad}");
        }
    }

    #[test]
    fn normalize_rejects_absolute_path() {
        let root = Path::new("/tmp/proj");
        let err = normalize_path(root, Path::new("/etc/passwd")).expect_err("must reject");
        assert!(matches!(err, LspError::PathTraversal));
    }

    #[test]
    fn normalize_allows_balanced_parent_dirs() {
        // src/../src/lib.rs never dips below depth 0 — this is allowed.
        let root = Path::new("/tmp/proj");
        let got = normalize_path(root, Path::new("src/../src/lib.rs")).unwrap();
        assert_eq!(got, PathBuf::from("/tmp/proj/src/lib.rs"));
    }

    #[tokio::test]
    async fn state_with_defaults_has_rust_registry() {
        let st = LspToolsState::with_defaults();
        assert!(st.registry.get("rust").is_some());
        assert!(st.supervisors.read().await.is_empty());
        assert!(st.cache.is_empty());
        assert_eq!(st.request_timeout, DEFAULT_REQUEST_TIMEOUT);
    }

    #[tokio::test]
    async fn get_or_spawn_rejects_unknown_language() {
        let st = LspToolsState::new(LspServerRegistry::new());
        let err = st
            .get_or_spawn("kotlin", Path::new("/tmp"))
            .await
            .expect_err("no registry entry");
        match err {
            LspError::Unsupported { language } => assert_eq!(language, "kotlin"),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
