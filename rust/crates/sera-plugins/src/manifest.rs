//! YAML plugin manifest parsing.
//!
//! Parses manifests of the form:
//!
//! ```yaml
//! apiVersion: sera.dev/v1
//! kind: Plugin
//! metadata:
//!   name: my-grpc-plugin
//! spec:
//!   capabilities: [ToolExecutor]
//!   transport: grpc
//!   grpc:
//!     endpoint: "localhost:9090"
//!   health_check_interval: 30s
//! ```
//!
//! or stdio:
//!
//! ```yaml
//! apiVersion: sera.dev/v1
//! kind: Plugin
//! metadata:
//!   name: my-stdio-plugin
//! spec:
//!   capabilities: [ContextEngine]
//!   transport: stdio
//!   stdio:
//!     command: ["/usr/bin/python", "-m", "my_plugin"]
//!   health_check_interval: 30s
//! ```

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

use crate::error::PluginError;
use crate::types::{PluginCapability, PluginRegistration, PluginTransport, PluginVersion};

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

/// Manifest spec block — transport-discriminated per SPEC-plugins §8.
///
/// The `transport:` key selects the variant; the matching sub-block (`grpc:`
/// or `stdio:`) carries the transport-specific config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestSpec {
    pub capabilities: Vec<PluginCapability>,
    /// Transport selection + config, flattened into the spec block.
    #[serde(flatten)]
    pub transport: PluginTransport,
    /// Health-check interval expressed as a human-readable string, e.g. `"30s"`, `"1m"`.
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval: String,
    #[serde(default = "default_version")]
    pub version: String,
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
        let n: u64 = secs
            .trim()
            .parse()
            .map_err(|_| PluginError::ManifestInvalid {
                reason: format!("invalid duration '{s}'"),
            })?;
        return Ok(Duration::from_secs(n));
    }
    if let Some(mins) = s.strip_suffix('m') {
        let n: u64 = mins
            .trim()
            .parse()
            .map_err(|_| PluginError::ManifestInvalid {
                reason: format!("invalid duration '{s}'"),
            })?;
        return Ok(Duration::from_secs(n * 60));
    }
    if let Some(hours) = s.strip_suffix('h') {
        let n: u64 = hours
            .trim()
            .parse()
            .map_err(|_| PluginError::ManifestInvalid {
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

/// Validate that `command[0]` is an absolute path (§6.2 binary pinning).
pub fn validate_stdio_command(command: &[String]) -> Result<(), PluginError> {
    match command.first() {
        None => Err(PluginError::ManifestInvalid {
            reason: "stdio command must not be empty".into(),
        }),
        Some(cmd) => {
            if !Path::new(cmd).is_absolute() {
                Err(PluginError::ManifestInvalid {
                    reason: format!(
                        "stdio command[0] must be an absolute path (got '{cmd}') — \
                         no $PATH resolution is performed at spawn time (§6.2)"
                    ),
                })
            } else {
                Ok(())
            }
        }
    }
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

        // Validate stdio binary pinning (§6.2)
        if let PluginTransport::Stdio { ref stdio } = self.spec.transport {
            validate_stdio_command(&stdio.command)?;
        }

        let version = parse_version(&self.spec.version)?;
        let health_check_interval = parse_duration(&self.spec.health_check_interval)?;

        Ok(PluginRegistration {
            name: self.metadata.name,
            version,
            capabilities: self.spec.capabilities,
            transport: self.spec.transport,
            health_check_interval,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Flat plugin manifest (sera/v1) — canonical on-disk format for SERA plugins
// ─────────────────────────────────────────────────────────────────────────────

/// Plugin kind — what role this plugin fulfils in the SERA extension model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PluginKind {
    /// Exposes callable tools to agents.
    Tool,
    /// Provides higher-level skill packs to agents.
    Skill,
    /// Implements a backend provider (model, memory, sandbox, …).
    Provider,
    /// Participates in the gateway hook chain.
    Hook,
}

/// Volume mount definition for a containerised plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginVolume {
    /// Path on the host (or in the orchestrator namespace).
    pub host_path: String,
    /// Mount path inside the plugin container.
    pub container_path: String,
    /// When `true` the mount is read-only.
    #[serde(default)]
    pub read_only: bool,
}

/// Flat on-disk manifest for a SERA plugin (`api_version: sera/v1`).
///
/// # Example
///
/// ```yaml
/// api_version: sera/v1
/// name: hello-plugin
/// version: "1.0.0"
/// kind: Tool
/// description: "A minimal hello-world plugin."
/// entry_point: "bin/hello-plugin"
/// capabilities:
///   - "fs:read"
/// ```
///
/// Parse with [`PluginManifestV1::from_yaml`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifestV1 {
    /// Must be exactly `"sera/v1"`.
    pub api_version: String,
    /// Plugin name — must match `^[a-z][a-z0-9-]*$`.
    pub name: String,
    /// Semantic version string, e.g. `"1.2.3"`.
    pub version: String,
    /// Plugin kind.
    pub kind: PluginKind,
    /// Human-readable description.
    pub description: Option<String>,
    /// Plugin author.
    pub author: Option<String>,
    /// SPDX license identifier, e.g. `"Apache-2.0"`.
    pub license: Option<String>,
    /// Plugin homepage URL.
    pub homepage: Option<String>,
    /// Path relative to the plugin root, or a container image reference.
    pub entry_point: String,
    /// Capability strings the plugin requires, e.g. `["fs:read", "net:http"]`.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Minimum sandbox tier required to run this plugin (1–3).
    pub requires_tier: Option<u8>,
    /// Environment variables to inject into the plugin process.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// Volume mounts for containerised plugins.
    #[serde(default)]
    pub volumes: Vec<PluginVolume>,
}

impl PluginManifestV1 {
    /// Parse a YAML string and validate the result.
    ///
    /// Validation rules:
    /// - `api_version` must equal `"sera/v1"`
    /// - `name` must match `^[a-z][a-z0-9-]*$`
    ///
    /// Returns [`PluginError::ManifestInvalid`] for parse or validation errors.
    pub fn from_yaml(yaml: &str) -> Result<Self, PluginError> {
        let manifest: Self =
            serde_yaml::from_str(yaml).map_err(|e| PluginError::ManifestInvalid {
                reason: e.to_string(),
            })?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), PluginError> {
        if self.api_version != "sera/v1" {
            return Err(PluginError::ManifestInvalid {
                reason: format!("api_version must be 'sera/v1', got '{}'", self.api_version),
            });
        }
        if !is_valid_plugin_name(&self.name) {
            return Err(PluginError::ManifestInvalid {
                reason: format!(
                    "name '{}' is invalid — must match ^[a-z][a-z0-9-]*$",
                    self.name
                ),
            });
        }
        Ok(())
    }
}

/// Returns `true` if `name` matches `^[a-z][a-z0-9-]*$`.
fn is_valid_plugin_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// ─────────────────────────────────────────────────────────────────────────────
// PluginService trait
//
// gRPC transport (tonic + .proto files in rust/proto/plugin/) is deferred to
// ecosystem phase S. For now, gateway wiring uses this Rust trait directly.
// Follow-up bead: sera-ecosystem-phase1-grpc.
// ─────────────────────────────────────────────────────────────────────────────

/// Sync trait for plugin lifecycle management.
///
/// A future phase will generate tonic stubs from `rust/proto/plugin/registry.proto`
/// and implement this trait over the gRPC channel. Until then, gateway code
/// targets this trait for testability.
pub trait PluginService: Send + Sync {
    /// Return metadata for all registered plugins.
    fn list_plugins(&self) -> Vec<PluginManifestV1>;

    /// Load (register) a plugin from its manifest.
    ///
    /// Returns [`PluginError::RegistrationFailed`] if the name is already taken.
    fn load_plugin(&self, manifest: PluginManifestV1) -> Result<(), PluginError>;

    /// Unload (deregister) a plugin by name.
    ///
    /// Returns [`PluginError::PluginNotFound`] if no such plugin is registered.
    fn unload_plugin(&self, name: &str) -> Result<(), PluginError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PluginTransport;

    // Updated to use the new transport-discriminated shape (SPEC-plugins §8).
    const MINIMAL_YAML: &str = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: test-plugin
spec:
  capabilities:
    - ToolExecutor
  transport: grpc
  grpc:
    endpoint: "localhost:9090"
  health_check_interval: 30s
"#;

    #[test]
    fn parse_minimal_manifest() {
        let m = PluginManifest::from_yaml(MINIMAL_YAML).unwrap();
        assert_eq!(m.kind, "Plugin");
        assert_eq!(m.metadata.name, "test-plugin");
        // transport is grpc with the expected endpoint
        match &m.spec.transport {
            PluginTransport::Grpc { grpc } => assert_eq!(grpc.endpoint, "localhost:9090"),
            other => panic!("expected Grpc transport, got {other:?}"),
        }
    }

    #[test]
    fn into_registration_produces_correct_fields() {
        let m = PluginManifest::from_yaml(MINIMAL_YAML).unwrap();
        let reg = m.into_registration().unwrap();
        assert_eq!(reg.name, "test-plugin");
        assert_eq!(reg.health_check_interval, Duration::from_secs(30));
        assert_eq!(reg.capabilities, vec![PluginCapability::ToolExecutor]);
        match &reg.transport {
            PluginTransport::Grpc { grpc } => {
                assert_eq!(grpc.endpoint, "localhost:9090");
                assert!(grpc.tls.is_none());
            }
            other => panic!("expected Grpc transport, got {other:?}"),
        }
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
  transport: grpc
  grpc:
    endpoint: "10.0.0.1:9090"
    tls:
      ca_cert: "ca-pem"
      client_cert: "cert-pem"
      client_key: "key-pem"
  health_check_interval: 1m
"#;
        let m = PluginManifest::from_yaml(yaml).unwrap();
        let reg = m.into_registration().unwrap();
        match &reg.transport {
            PluginTransport::Grpc { grpc } => {
                let tls = grpc.tls.as_ref().unwrap();
                assert_eq!(tls.ca_cert, "ca-pem");
            }
            other => panic!("expected Grpc transport, got {other:?}"),
        }
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
  transport: grpc
  grpc:
    endpoint: "localhost:9091"
  health_check_interval: 30s
"#;
        let m = PluginManifest::from_yaml(yaml).unwrap();
        let reg = m.into_registration().unwrap();
        assert_eq!(reg.capabilities.len(), 2);
    }

    #[test]
    fn stdio_manifest_parses() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: my-stdio-plugin
spec:
  capabilities:
    - ContextEngine
  transport: stdio
  stdio:
    command: ["/usr/bin/python", "-m", "my_plugin"]
    env:
      MY_PLUGIN_CONFIG: "/etc/sera/plugins/my-plugin.toml"
  health_check_interval: 30s
"#;
        let m = PluginManifest::from_yaml(yaml).unwrap();
        let reg = m.into_registration().unwrap();
        assert_eq!(reg.name, "my-stdio-plugin");
        assert_eq!(reg.capabilities, vec![PluginCapability::ContextEngine]);
        match &reg.transport {
            PluginTransport::Stdio { stdio } => {
                assert_eq!(stdio.command, vec!["/usr/bin/python", "-m", "my_plugin"]);
                assert_eq!(
                    stdio.env.get("MY_PLUGIN_CONFIG").map(String::as_str),
                    Some("/etc/sera/plugins/my-plugin.toml")
                );
            }
            other => panic!("expected Stdio transport, got {other:?}"),
        }
    }

    #[test]
    fn stdio_relative_command_rejected() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: bad-plugin
spec:
  capabilities:
    - ToolExecutor
  transport: stdio
  stdio:
    command: ["python", "-m", "my_plugin"]
  health_check_interval: 30s
"#;
        let m = PluginManifest::from_yaml(yaml).unwrap();
        let err = m.into_registration().unwrap_err();
        assert!(matches!(err, PluginError::ManifestInvalid { .. }));
        assert!(err.to_string().contains("absolute path"));
    }

    #[test]
    fn stdio_empty_command_rejected() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: bad-plugin
spec:
  capabilities:
    - ToolExecutor
  transport: stdio
  stdio:
    command: []
  health_check_interval: 30s
"#;
        let m = PluginManifest::from_yaml(yaml).unwrap();
        let err = m.into_registration().unwrap_err();
        assert!(matches!(err, PluginError::ManifestInvalid { .. }));
    }

    // ── PluginManifestV1 tests ────────────────────────────────────────────────

    const MINIMAL_V1_YAML: &str = r#"
api_version: sera/v1
name: hello-plugin
version: "1.0.0"
kind: Tool
entry_point: "bin/hello-plugin"
"#;

    #[test]
    fn v1_valid_minimal_parses() {
        let m = PluginManifestV1::from_yaml(MINIMAL_V1_YAML).unwrap();
        assert_eq!(m.api_version, "sera/v1");
        assert_eq!(m.name, "hello-plugin");
        assert_eq!(m.version, "1.0.0");
        assert_eq!(m.kind, PluginKind::Tool);
        assert_eq!(m.entry_point, "bin/hello-plugin");
        assert!(m.capabilities.is_empty());
        assert!(m.env.is_empty());
        assert!(m.volumes.is_empty());
        assert!(m.requires_tier.is_none());
    }

    #[test]
    fn v1_invalid_api_version_rejected() {
        let yaml = MINIMAL_V1_YAML.replace("sera/v1", "sera/v2");
        let err = PluginManifestV1::from_yaml(&yaml).unwrap_err();
        assert!(matches!(err, PluginError::ManifestInvalid { .. }));
        let msg = err.to_string();
        assert!(msg.contains("api_version"));
    }

    #[test]
    fn v1_name_starts_with_digit_rejected() {
        let yaml = MINIMAL_V1_YAML.replace("name: hello-plugin", "name: 1bad-name");
        let err = PluginManifestV1::from_yaml(&yaml).unwrap_err();
        assert!(matches!(err, PluginError::ManifestInvalid { .. }));
        assert!(err.to_string().contains("1bad-name"));
    }

    #[test]
    fn v1_name_with_uppercase_rejected() {
        let yaml = MINIMAL_V1_YAML.replace("name: hello-plugin", "name: BadName");
        let err = PluginManifestV1::from_yaml(&yaml).unwrap_err();
        assert!(matches!(err, PluginError::ManifestInvalid { .. }));
        assert!(err.to_string().contains("BadName"));
    }

    #[test]
    fn v1_kind_variants_roundtrip() {
        for (yaml_kind, expected) in &[
            ("Tool", PluginKind::Tool),
            ("Skill", PluginKind::Skill),
            ("Provider", PluginKind::Provider),
            ("Hook", PluginKind::Hook),
        ] {
            let yaml = format!(
                "api_version: sera/v1\nname: test\nversion: \"1.0.0\"\nkind: {yaml_kind}\nentry_point: bin/x\n"
            );
            let m = PluginManifestV1::from_yaml(&yaml).unwrap();
            assert_eq!(&m.kind, expected, "kind {yaml_kind} did not roundtrip");
            // Roundtrip through serde_yaml
            let reserialized = serde_yaml::to_string(&m).unwrap();
            let m2: PluginManifestV1 = serde_yaml::from_str(&reserialized).unwrap();
            assert_eq!(&m2.kind, expected);
        }
    }

    #[test]
    fn v1_missing_required_field_produces_error() {
        // Missing `entry_point`
        let yaml = r#"
api_version: sera/v1
name: no-entry
version: "1.0.0"
kind: Tool
"#;
        let err = PluginManifestV1::from_yaml(yaml).unwrap_err();
        assert!(matches!(err, PluginError::ManifestInvalid { .. }));
        // serde_yaml error message should mention the missing field
        assert!(err.to_string().contains("entry_point"));
    }

    #[test]
    fn v1_volume_read_only_serializes_correctly() {
        let yaml = r#"
api_version: sera/v1
name: vol-plugin
version: "1.0.0"
kind: Provider
entry_point: "bin/vol-plugin"
volumes:
  - host_path: "/data/models"
    container_path: "/models"
    read_only: true
  - host_path: "/tmp/scratch"
    container_path: "/scratch"
"#;
        let m = PluginManifestV1::from_yaml(yaml).unwrap();
        assert_eq!(m.volumes.len(), 2);
        let ro = &m.volumes[0];
        assert_eq!(ro.host_path, "/data/models");
        assert_eq!(ro.container_path, "/models");
        assert!(ro.read_only);
        let rw = &m.volumes[1];
        assert!(!rw.read_only);

        // Serialise and check read_only appears in output
        let out = serde_yaml::to_string(&m).unwrap();
        assert!(out.contains("read_only: true"));
    }

    #[test]
    fn v1_full_manifest_parses() {
        let yaml = r#"
api_version: sera/v1
name: full-plugin
version: "2.1.0"
kind: Skill
description: "Full featured plugin"
author: "SERA Team"
license: "Apache-2.0"
homepage: "https://example.com"
entry_point: "ghcr.io/org/full-plugin:2.1.0"
capabilities:
  - "fs:read"
  - "net:http"
requires_tier: 2
env:
  LOG_LEVEL: debug
  API_KEY: placeholder
volumes:
  - host_path: "/var/data"
    container_path: "/data"
    read_only: false
"#;
        let m = PluginManifestV1::from_yaml(yaml).unwrap();
        assert_eq!(m.name, "full-plugin");
        assert_eq!(m.kind, PluginKind::Skill);
        assert_eq!(m.capabilities, vec!["fs:read", "net:http"]);
        assert_eq!(m.requires_tier, Some(2));
        assert_eq!(m.env.get("LOG_LEVEL").map(String::as_str), Some("debug"));
        assert_eq!(m.volumes.len(), 1);
    }
}
