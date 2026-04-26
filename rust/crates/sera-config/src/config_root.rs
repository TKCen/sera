//! Config root — single source of truth for where SERA reads user config.
//!
//! Parallel to [`crate::data_root::DataRoot`] (which addresses *state*). The
//! config root holds *user-edited* manifests: providers, capability policies,
//! agents, workflows, secrets refs. Layout follows a kubernetes-style
//! composition model — a top-level `config.toml` plus subdirectories of
//! typed manifests.
//!
//! Platform-aware default:
//! - Linux / macOS: `$XDG_CONFIG_HOME/sera` → typically `~/.config/sera`
//! - Windows: `{FOLDERID_RoamingAppData}/sera` → typically `%APPDATA%/sera`
//!
//! Override at runtime by setting `SERA_CONFIG_ROOT`.
//!
//! ```text
//! ~/.config/sera/
//!   config.toml         # top-level user prefs (gateway endpoint, theme, …)
//!   providers/   *.yaml # kind: Provider
//!   policies/    *.yaml # kind: CapabilityPolicy
//!   agents/      *.yaml # kind: Agent
//!   workflows/   *.yaml # kind: WorkflowDef
//!   secrets/            # kind: Secret refs (values in OS keychain)
//! ```

use std::env;
use std::path::{Path, PathBuf};

/// Environment variable used to override the config root at runtime.
pub const CONFIG_ROOT_ENV: &str = "SERA_CONFIG_ROOT";

/// Resolved base directory for all SERA user-edited configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRoot {
    base: PathBuf,
}

impl ConfigRoot {
    /// Build a [`ConfigRoot`] from an explicit path.
    pub fn new<P: Into<PathBuf>>(base: P) -> Self {
        Self { base: base.into() }
    }

    /// Resolve the effective config root:
    /// 1. `SERA_CONFIG_ROOT` env var if set and non-empty
    /// 2. Platform config dir + `sera` subdir (Linux `~/.config/sera`, Windows `%APPDATA%/sera`)
    /// 3. Fallback to `/tmp/sera-config` when no config dir is available (CI / stripped envs).
    pub fn from_env() -> Self {
        if let Ok(raw) = env::var(CONFIG_ROOT_ENV)
            && !raw.is_empty()
        {
            return Self::new(PathBuf::from(raw));
        }
        Self::default_path()
            .map(Self::new)
            .unwrap_or_else(|| Self::new(PathBuf::from("/tmp/sera-config")))
    }

    /// Platform-appropriate default path, or `None` if the platform config
    /// dir cannot be determined.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("sera"))
    }

    /// Root path (e.g. `~/.config/sera`).
    pub fn path(&self) -> &Path {
        &self.base
    }

    /// `<root>/config.toml` — top-level user preferences.
    pub fn config_file(&self) -> PathBuf {
        self.base.join("config.toml")
    }

    /// `<root>/providers` — directory of `kind: Provider` manifests.
    pub fn providers_dir(&self) -> PathBuf {
        self.base.join("providers")
    }

    /// `<root>/policies` — directory of `kind: CapabilityPolicy` manifests.
    pub fn policies_dir(&self) -> PathBuf {
        self.base.join("policies")
    }

    /// `<root>/agents` — directory of `kind: Agent` manifests.
    pub fn agents_dir(&self) -> PathBuf {
        self.base.join("agents")
    }

    /// `<root>/workflows` — directory of `kind: WorkflowDef` manifests.
    pub fn workflows_dir(&self) -> PathBuf {
        self.base.join("workflows")
    }

    /// `<root>/secrets` — directory of secret-reference manifests
    /// (values themselves live in the OS keychain).
    pub fn secrets_dir(&self) -> PathBuf {
        self.base.join("secrets")
    }
}

impl Default for ConfigRoot {
    fn default() -> Self {
        Self::from_env()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_path_uses_platform_config_dir() {
        let p = ConfigRoot::default_path().expect("platform config dir available");
        assert!(p.ends_with("sera"), "expected default to end with 'sera', got {:?}", p);
        assert!(p.is_absolute(), "default config root must be absolute: {:?}", p);
    }

    #[test]
    fn explicit_path_overrides_default() {
        let root = ConfigRoot::new("/custom/sera");
        assert_eq!(root.path(), Path::new("/custom/sera"));
    }

    #[test]
    fn subdir_helpers_compose_paths() {
        let root = ConfigRoot::new("/srv/sera");
        assert_eq!(root.config_file(), PathBuf::from("/srv/sera/config.toml"));
        assert_eq!(root.providers_dir(), PathBuf::from("/srv/sera/providers"));
        assert_eq!(root.policies_dir(), PathBuf::from("/srv/sera/policies"));
        assert_eq!(root.agents_dir(), PathBuf::from("/srv/sera/agents"));
        assert_eq!(root.workflows_dir(), PathBuf::from("/srv/sera/workflows"));
        assert_eq!(root.secrets_dir(), PathBuf::from("/srv/sera/secrets"));
    }

    #[test]
    fn from_env_honors_explicit_override() {
        let saved = env::var(CONFIG_ROOT_ENV).ok();
        unsafe { env::set_var(CONFIG_ROOT_ENV, "/opt/sera-conf") };
        let root = ConfigRoot::from_env();
        match &saved {
            Some(v) => unsafe { env::set_var(CONFIG_ROOT_ENV, v) },
            None => unsafe { env::remove_var(CONFIG_ROOT_ENV) },
        }
        assert_eq!(root.path(), Path::new("/opt/sera-conf"));
    }
}
