//! Shared state for LSP-backed tools.
//!
//! `LspToolsState` is injected into every tool invocation — there is no
//! global singleton. It owns the per-language supervisors, the registry,
//! and the symbol cache.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use super::cache::SymbolCache;
use super::error::LspError;
use super::registry::LspServerRegistry;
use super::supervisor::LspProcessSupervisor;

/// Owned state for the LSP tools subsystem. Cheap to clone — all fields are
/// `Arc` or `HashMap` of `Arc`.
#[derive(Debug, Default, Clone)]
pub struct LspToolsState {
    /// Supervisors keyed by `language_id`. Lazily populated on first use.
    pub supervisors: HashMap<String, Arc<LspProcessSupervisor>>,
    pub registry: LspServerRegistry,
    pub cache: SymbolCache,
}

impl LspToolsState {
    pub fn new(registry: LspServerRegistry) -> Self {
        Self {
            supervisors: HashMap::new(),
            registry,
            cache: SymbolCache::new(),
        }
    }

    /// SERA default: Rust-only registry, empty supervisor map, fresh cache.
    pub fn with_defaults() -> Self {
        Self::new(LspServerRegistry::with_defaults())
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

    #[test]
    fn state_with_defaults_has_rust_registry() {
        let st = LspToolsState::with_defaults();
        assert!(st.registry.get("rust").is_some());
        assert!(st.supervisors.is_empty());
        assert!(st.cache.is_empty());
    }
}
