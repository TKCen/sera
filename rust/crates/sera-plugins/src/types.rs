//! Core plugin types: registration, capabilities, health, and TLS config.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A plugin capability advertised at registration time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PluginCapability {
    MemoryBackend,
    ToolExecutor,
    SandboxProvider,
    AuthProvider,
    SecretProvider,
    RealtimeBackend,
    #[serde(untagged)]
    Custom(String),
}

impl fmt::Display for PluginCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MemoryBackend => write!(f, "MemoryBackend"),
            Self::ToolExecutor => write!(f, "ToolExecutor"),
            Self::SandboxProvider => write!(f, "SandboxProvider"),
            Self::AuthProvider => write!(f, "AuthProvider"),
            Self::SecretProvider => write!(f, "SecretProvider"),
            Self::RealtimeBackend => write!(f, "RealtimeBackend"),
            Self::Custom(s) => write!(f, "Custom({s})"),
        }
    }
}

/// Semantic version of a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl PluginVersion {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch }
    }
}

impl fmt::Display for PluginVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// mTLS configuration for plugin connections (required for Tier 2/3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    /// PEM-encoded CA certificate.
    pub ca_cert: String,
    /// PEM-encoded client certificate.
    pub client_cert: String,
    /// PEM-encoded client private key.
    pub client_key: String,
}

/// Plugin registration descriptor submitted when a plugin connects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRegistration {
    /// Unique plugin name (used as the registry key).
    pub name: String,
    pub version: PluginVersion,
    pub capabilities: Vec<PluginCapability>,
    /// gRPC endpoint the plugin listens on (e.g. `"localhost:9090"`).
    pub endpoint: String,
    /// mTLS config — required for Tier 2/3, optional for localhost dev.
    pub tls: Option<TlsConfig>,
    /// How often the gateway should perform a health check.
    pub health_check_interval: std::time::Duration,
}

/// Point-in-time health snapshot for a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHealth {
    pub healthy: bool,
    pub last_check: DateTime<Utc>,
    pub consecutive_failures: u32,
    pub latency_ms: Option<u64>,
}

impl PluginHealth {
    /// Construct an initial (unknown) health record.
    pub fn initial() -> Self {
        Self {
            healthy: false,
            last_check: Utc::now(),
            consecutive_failures: 0,
            latency_ms: None,
        }
    }

    /// Construct a successful health record.
    pub fn ok(latency_ms: u64) -> Self {
        Self {
            healthy: true,
            last_check: Utc::now(),
            consecutive_failures: 0,
            latency_ms: Some(latency_ms),
        }
    }
}

/// Full plugin descriptor stored in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub registration: PluginRegistration,
    pub health: PluginHealth,
    pub registered_at: DateTime<Utc>,
}

impl PluginInfo {
    pub fn new(registration: PluginRegistration) -> Self {
        Self {
            health: PluginHealth::initial(),
            registered_at: Utc::now(),
            registration,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_version_display() {
        let v = PluginVersion::new(1, 2, 3);
        assert_eq!(v.to_string(), "1.2.3");
    }

    #[test]
    fn capability_display() {
        assert_eq!(PluginCapability::MemoryBackend.to_string(), "MemoryBackend");
        assert_eq!(
            PluginCapability::Custom("MyThing".into()).to_string(),
            "Custom(MyThing)"
        );
    }

    #[test]
    fn capability_serde_roundtrip() {
        let cap = PluginCapability::ToolExecutor;
        let json = serde_json::to_string(&cap).unwrap();
        let back: PluginCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cap);
    }

    #[test]
    fn plugin_health_initial_is_unhealthy() {
        let h = PluginHealth::initial();
        assert!(!h.healthy);
        assert_eq!(h.consecutive_failures, 0);
        assert!(h.latency_ms.is_none());
    }

    #[test]
    fn plugin_health_ok_is_healthy() {
        let h = PluginHealth::ok(42);
        assert!(h.healthy);
        assert_eq!(h.latency_ms, Some(42));
    }

    #[test]
    fn plugin_info_new_sets_initial_health() {
        let reg = PluginRegistration {
            name: "test".into(),
            version: PluginVersion::new(1, 0, 0),
            capabilities: vec![PluginCapability::ToolExecutor],
            endpoint: "localhost:9090".into(),
            tls: None,
            health_check_interval: std::time::Duration::from_secs(30),
        };
        let info = PluginInfo::new(reg);
        assert!(!info.health.healthy);
        assert_eq!(info.registration.name, "test");
    }
}
