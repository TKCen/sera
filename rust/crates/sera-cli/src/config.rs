//! CLI configuration — loaded from `~/.sera/config.toml` by default.
//!
//! The `--config <path>` flag overrides the default path.  If the file does
//! not exist, `CliConfig::default()` is returned silently (first-run UX).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Top-level CLI configuration file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CliConfig {
    /// Base URL of the SERA gateway (e.g. `http://localhost:8080`).
    pub endpoint: String,

    /// Default agent ID used when `--agent` is omitted.
    pub default_agent: Option<String>,

    /// Path to the file containing the bearer token.
    /// Populated by `sera auth login` (sera-j1hs).
    pub token_path: Option<PathBuf>,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:8080".into(),
            default_agent: None,
            token_path: None,
        }
    }
}

impl CliConfig {
    /// Return the default config file path: `~/.sera/config.toml`.
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".sera")
            .join("config.toml")
    }

    /// Load config from `path`.  Returns `CliConfig::default()` if the file
    /// does not exist (graceful first-run).  Returns an error for parse failures.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            tracing::debug!(path = %path.display(), "config file not found, using defaults");
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config file: {}", path.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("failed to parse config file: {}", path.display()))
    }

    /// Persist config to `path`, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create config directory: {}", parent.display()))?;
        }
        let raw = toml::to_string_pretty(self)
            .context("failed to serialize config")?;
        std::fs::write(path, raw)
            .with_context(|| format!("failed to write config file: {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn default_config_has_expected_endpoint() {
        let cfg = CliConfig::default();
        assert_eq!(cfg.endpoint, "http://localhost:8080");
        assert!(cfg.default_agent.is_none());
        assert!(cfg.token_path.is_none());
    }

    #[test]
    fn load_nonexistent_file_returns_default() {
        let path = PathBuf::from("/tmp/nonexistent-sera-config-abc123.toml");
        let cfg = CliConfig::load(&path).unwrap();
        assert_eq!(cfg.endpoint, "http://localhost:8080");
    }

    #[test]
    fn load_and_round_trip() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"endpoint = "http://gateway.example.com:9090"
default_agent = "my-agent"
"#
        )
        .unwrap();
        let cfg = CliConfig::load(f.path()).unwrap();
        assert_eq!(cfg.endpoint, "http://gateway.example.com:9090");
        assert_eq!(cfg.default_agent.as_deref(), Some("my-agent"));
    }

    #[test]
    fn save_creates_file_and_can_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subdir").join("config.toml");
        let cfg = CliConfig {
            endpoint: "http://saved-endpoint:1234".into(),
            default_agent: Some("saved-agent".into()),
            token_path: None,
        };
        cfg.save(&path).unwrap();
        assert!(path.exists());
        let reloaded = CliConfig::load(&path).unwrap();
        assert_eq!(reloaded.endpoint, "http://saved-endpoint:1234");
        assert_eq!(reloaded.default_agent.as_deref(), Some("saved-agent"));
    }
}
