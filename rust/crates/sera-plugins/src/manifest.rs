//! YAML plugin manifest parsing.
//!
//! Parses manifests of the form:
//!
//! ```yaml
//! apiVersion: sera.dev/v1
//! kind: Plugin
//! metadata:
//!   name: my-plugin
//! spec:
//!   capabilities: [ToolExecutor]
//!   endpoint: "localhost:9090"
//!   health_check_interval: 30s
//! ```

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::PluginError;
use crate::types::{PluginCapability, PluginRegistration, PluginVersion, TlsConfig};

/// Top-level structure of a plugin YAML manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: ManifestMetadata,
    pub spec: ManifestSpec,
}

/// Manifest metadata block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestMetadata {
    pub name: String,
    #[serde(default)]
    pub labels: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub annotations: std::collections::HashMap<String, String>,
}

/// Manifest spec block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestSpec {
    pub capabilities: Vec<PluginCapability>,
    pub endpoint: String,
    /// Health-check interval expressed as a human-readable string, e.g. `"30s"`, `"1m"`.
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval: String,
    pub tls: Option<ManifestTlsConfig>,
    #[serde(default = "default_version")]
    pub version: String,
}

/// TLS config as expressed in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestTlsConfig {
    pub ca_cert: String,
    pub client_cert: String,
    pub client_key: String,
}

fn default_health_check_interval() -> String {
    "30s".into()
}

fn default_version() -> String {
    "1.0.0".into()
}

/// Parse a duration string such as `"30s"`, `"5m"`, `"1h"` into a [`Duration`].
pub fn parse_duration(s: &str) -> Result<Duration, PluginError> {
    let s = s.trim();
    if let Some(secs) = s.strip_suffix('s') {
        let n: u64 = secs.trim().parse().map_err(|_| PluginError::ManifestInvalid {
            reason: format!("invalid duration '{s}'"),
        })?;
        return Ok(Duration::from_secs(n));
    }
    if let Some(mins) = s.strip_suffix('m') {
        let n: u64 = mins.trim().parse().map_err(|_| PluginError::ManifestInvalid {
            reason: format!("invalid duration '{s}'"),
        })?;
        return Ok(Duration::from_secs(n * 60));
    }
    if let Some(hours) = s.strip_suffix('h') {
        let n: u64 = hours.trim().parse().map_err(|_| PluginError::ManifestInvalid {
            reason: format!("invalid duration '{s}'"),
        })?;
        return Ok(Duration::from_secs(n * 3600));
    }
    Err(PluginError::ManifestInvalid {
        reason: format!("unrecognised duration format '{s}' — use e.g. '30s', '5m', '1h'"),
    })
}

/// Parse a version string `"MAJOR.MINOR.PATCH"` into a [`PluginVersion`].
fn parse_version(s: &str) -> Result<PluginVersion, PluginError> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return Err(PluginError::ManifestInvalid {
            reason: format!("version '{s}' must be MAJOR.MINOR.PATCH"),
        });
    }
    let parse_u32 = |p: &str| -> Result<u32, PluginError> {
        p.parse().map_err(|_| PluginError::ManifestInvalid {
            reason: format!("version component '{p}' is not a number"),
        })
    };
    Ok(PluginVersion {
        major: parse_u32(parts[0])?,
        minor: parse_u32(parts[1])?,
        patch: parse_u32(parts[2])?,
    })
}

impl PluginManifest {
    /// Parse a YAML string into a [`PluginManifest`].
    pub fn from_yaml(yaml: &str) -> Result<Self, PluginError> {
        serde_yaml::from_str(yaml).map_err(|e| PluginError::ManifestInvalid {
            reason: e.to_string(),
        })
    }

    /// Validate the manifest and convert it into a [`PluginRegistration`].
    pub fn into_registration(self) -> Result<PluginRegistration, PluginError> {
        if self.kind != "Plugin" {
            return Err(PluginError::ManifestInvalid {
                reason: format!("expected kind 'Plugin', got '{}'", self.kind),
            });
        }
        if self.metadata.name.is_empty() {
            return Err(PluginError::ManifestInvalid {
                reason: "metadata.name must not be empty".into(),
            });
        }

        let version = parse_version(&self.spec.version)?;
        let health_check_interval = parse_duration(&self.spec.health_check_interval)?;

        let tls = self.spec.tls.map(|t| TlsConfig {
            ca_cert: t.ca_cert,
            client_cert: t.client_cert,
            client_key: t.client_key,
        });

        Ok(PluginRegistration {
            name: self.metadata.name,
            version,
            capabilities: self.spec.capabilities,
            endpoint: self.spec.endpoint,
            tls,
            health_check_interval,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_YAML: &str = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: test-plugin
spec:
  capabilities:
    - ToolExecutor
  endpoint: "localhost:9090"
  health_check_interval: 30s
"#;

    #[test]
    fn parse_minimal_manifest() {
        let m = PluginManifest::from_yaml(MINIMAL_YAML).unwrap();
        assert_eq!(m.kind, "Plugin");
        assert_eq!(m.metadata.name, "test-plugin");
        assert_eq!(m.spec.endpoint, "localhost:9090");
    }

    #[test]
    fn into_registration_produces_correct_fields() {
        let m = PluginManifest::from_yaml(MINIMAL_YAML).unwrap();
        let reg = m.into_registration().unwrap();
        assert_eq!(reg.name, "test-plugin");
        assert_eq!(reg.health_check_interval, Duration::from_secs(30));
        assert_eq!(reg.capabilities, vec![PluginCapability::ToolExecutor]);
        assert!(reg.tls.is_none());
    }

    #[test]
    fn wrong_kind_fails() {
        let yaml = MINIMAL_YAML.replace("kind: Plugin", "kind: Agent");
        let m = PluginManifest::from_yaml(&yaml).unwrap();
        let err = m.into_registration().unwrap_err();
        assert!(matches!(err, PluginError::ManifestInvalid { .. }));
    }

    #[test]
    fn empty_name_fails() {
        let yaml = MINIMAL_YAML.replace("name: test-plugin", "name: \"\"");
        let m = PluginManifest::from_yaml(&yaml).unwrap();
        let err = m.into_registration().unwrap_err();
        assert!(matches!(err, PluginError::ManifestInvalid { .. }));
    }

    #[test]
    fn parse_duration_seconds() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
    }

    #[test]
    fn parse_duration_minutes() {
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
    }

    #[test]
    fn parse_duration_hours() {
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
    }

    #[test]
    fn parse_duration_invalid_fails() {
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("10d").is_err());
    }

    #[test]
    fn manifest_with_tls() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: secure-plugin
spec:
  capabilities:
    - MemoryBackend
  endpoint: "10.0.0.1:9090"
  health_check_interval: 1m
  tls:
    ca_cert: "ca-pem"
    client_cert: "cert-pem"
    client_key: "key-pem"
"#;
        let m = PluginManifest::from_yaml(yaml).unwrap();
        let reg = m.into_registration().unwrap();
        let tls = reg.tls.unwrap();
        assert_eq!(tls.ca_cert, "ca-pem");
        assert_eq!(reg.health_check_interval, Duration::from_secs(60));
    }

    #[test]
    fn multiple_capabilities() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: multi
spec:
  capabilities:
    - ToolExecutor
    - SandboxProvider
  endpoint: "localhost:9091"
  health_check_interval: 30s
"#;
        let m = PluginManifest::from_yaml(yaml).unwrap();
        let reg = m.into_registration().unwrap();
        assert_eq!(reg.capabilities.len(), 2);
    }
}
