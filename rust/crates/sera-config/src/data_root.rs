//! Data root — single source of truth for where SERA writes host-side data.
//!
//! Platform-aware default:
//! - Linux / macOS: `$XDG_DATA_HOME/sera` → typically `~/.local/share/sera`
//! - Windows: `%APPDATA%/sera`
//!
//! All production code that previously hardcoded paths like `/tmp/sera`,
//! `/var/lib/sera`, or `/data/sera` should instead resolve paths through a
//! shared [`DataRoot`] so deployments can relocate SERA state by setting the
//! `SERA_DATA_ROOT` environment variable once.

use std::env;
use std::path::{Path, PathBuf};

/// Environment variable used to override the data root at runtime.
pub const DATA_ROOT_ENV: &str = "SERA_DATA_ROOT";

/// Resolved base directory for all SERA host-side state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRoot {
    base: PathBuf,
}

impl DataRoot {
    /// Build a [`DataRoot`] from an explicit path.
    pub fn new<P: Into<PathBuf>>(base: P) -> Self {
        Self { base: base.into() }
    }

    /// Resolve the effective data root:
    /// 1. `SERA_DATA_ROOT` env var if set and non-empty
    /// 2. Platform data dir + `sera` subdir (Linux `~/.local/share/sera`, Windows `%APPDATA%/sera`)
    /// 3. Fallback to `/tmp/sera` when no data dir is available (CI / stripped envs).
    pub fn from_env() -> Self {
        if let Ok(raw) = env::var(DATA_ROOT_ENV)
            && !raw.is_empty()
        {
            return Self::new(PathBuf::from(raw));
        }
        Self::default_path().map(Self::new).unwrap_or_else(|| Self::new(PathBuf::from("/tmp/sera")))
    }

    /// Platform-appropriate default path, or `None` if the platform data dir
    /// cannot be determined.
    pub fn default_path() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("sera"))
    }

    /// Root path (e.g. `~/.local/share/sera`).
    pub fn path(&self) -> &Path {
        &self.base
    }

    /// `<root>/agents/<instance_id>` — per-agent workspace directory.
    pub fn agent_workspace(&self, instance_id: &str) -> PathBuf {
        self.base.join("agents").join(instance_id)
    }

    /// `<root>/memory` — host-side memory block store root.
    pub fn memory_root(&self) -> PathBuf {
        self.base.join("memory")
    }
}

impl Default for DataRoot {
    fn default() -> Self {
        Self::from_env()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_path_uses_platform_data_dir() {
        // On every supported host we test on (Linux, macOS, Windows) dirs::data_dir()
        // returns Some(...), so the default must be Some and end with "sera".
        let p = DataRoot::default_path().expect("platform data dir available");
        assert!(p.ends_with("sera"), "expected default to end with 'sera', got {:?}", p);

        // And the resolved path must be absolute on all supported platforms.
        assert!(p.is_absolute(), "default data root must be absolute: {:?}", p);
    }

    #[test]
    fn explicit_path_overrides_default() {
        let root = DataRoot::new("/custom/sera");
        assert_eq!(root.path(), Path::new("/custom/sera"));
    }

    #[test]
    fn agent_workspace_composes_path() {
        let root = DataRoot::new("/srv/sera");
        assert_eq!(root.agent_workspace("abc"), PathBuf::from("/srv/sera/agents/abc"));
    }

    #[test]
    fn memory_root_composes_path() {
        let root = DataRoot::new("/srv/sera");
        assert_eq!(root.memory_root(), PathBuf::from("/srv/sera/memory"));
    }

    #[test]
    fn from_env_honors_explicit_override() {
        // Snapshot + restore so we don't leak into sibling tests.
        let saved = env::var(DATA_ROOT_ENV).ok();
        unsafe { env::set_var(DATA_ROOT_ENV, "/opt/sera-data") };
        let root = DataRoot::from_env();
        match &saved {
            Some(v) => unsafe { env::set_var(DATA_ROOT_ENV, v) },
            None => unsafe { env::remove_var(DATA_ROOT_ENV) },
        }
        assert_eq!(root.path(), Path::new("/opt/sera-data"));
    }
}
