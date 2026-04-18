//! Sandbox provider abstraction — object-safe async trait with multiple backends.

pub mod policy;

#[cfg(feature = "docker")]
pub mod docker;

pub mod wasm;
pub mod microvm;
pub mod external;
pub mod openshell;

pub use policy::{
    DockerSandboxPolicy, FileSystemSandboxPolicy, L7Protocol, L7Rule, NetworkEndpoint,
    NetworkPolicyRule, NetworkSandboxPolicy, PolicyAction, PolicyStatus, SandboxPolicy,
};

use std::collections::HashMap;

use async_trait::async_trait;
use sera_types::sandbox::SourceMount;
use thiserror::Error;

pub use policy::SandboxPolicy as SandboxPolicyType;

/// Opaque handle to a running sandbox.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SandboxHandle(pub String);

/// A read-write or read-only bind mount requested by the caller on sandbox
/// create. Used for ephemeral agent spawns that need scratch volumes, config
/// drops, or parent-worktree sharing beyond the curated [`SourceMount`] set.
///
/// Unlike [`SourceMount`], mounts specified here:
/// - may be writable (`read_only = false`),
/// - are not restricted to the `/sources/` container prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountSpec {
    /// Absolute host path.
    pub host_path: String,
    /// Absolute container path.
    pub container_path: String,
    /// If `true`, bind is mounted read-only (`:ro`).
    pub read_only: bool,
}

/// Configuration for creating a new sandbox.
#[derive(Debug, Clone, Default)]
pub struct SandboxConfig {
    pub image: Option<String>,
    pub sandbox_policy: Option<SandboxPolicy>,
    pub env: HashMap<String, String>,
    pub labels: HashMap<String, String>,
    pub memory_limit_bytes: Option<u64>,
    pub cpu_limit: Option<f64>,
    pub sources: Vec<SourceMount>,
    /// Additional bind mounts requested by the caller (ephemeral agents etc.).
    /// Appended after [`Self::sources`] into the Docker `-v` flag list.
    pub additional_mounts: Vec<MountSpec>,
}

/// Output from executing a command inside a sandbox.
#[derive(Debug, Clone)]
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Errors from sandbox operations.
#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("not implemented for this provider")]
    NotImplemented,
    #[error("failed to create sandbox: {reason}")]
    CreateFailed { reason: String },
    #[error("failed to execute command: {reason}")]
    ExecFailed { reason: String },
    #[error("failed to destroy sandbox: {reason}")]
    DestroyFailed { reason: String },
    #[error("sandbox not found")]
    NotFound,
    #[error("policy violation: {reason}")]
    PolicyViolation { reason: String },
    #[error("invalid source mount: {reason}")]
    InvalidSourceMount { reason: String },
}

/// Validate source mounts in a config.
///
/// Rules:
/// - Container paths must start with `/sources/`
/// - Container paths must not contain `..`
pub fn validate_sources(sources: &[SourceMount]) -> Result<(), SandboxError> {
    for mount in sources {
        if mount.container_path.contains("..") {
            return Err(SandboxError::InvalidSourceMount {
                reason: format!(
                    "container_path '{}' must not contain '..'",
                    mount.container_path
                ),
            });
        }
        if !mount.container_path.starts_with("/sources/") {
            return Err(SandboxError::InvalidSourceMount {
                reason: format!(
                    "container_path '{}' must start with '/sources/'",
                    mount.container_path
                ),
            });
        }
    }
    Ok(())
}

/// Object-safe async trait for sandbox providers.
#[async_trait]
pub trait SandboxProvider: Send + Sync {
    /// Provider name (e.g. "docker", "wasm").
    fn name(&self) -> &str;

    /// Create a new sandbox and return its handle.
    async fn create(&self, config: &SandboxConfig) -> Result<SandboxHandle, SandboxError>;

    /// Execute a command inside the sandbox.
    async fn execute(
        &self,
        handle: &SandboxHandle,
        command: &str,
        env: &HashMap<String, String>,
    ) -> Result<ExecResult, SandboxError>;

    /// Read a file from the sandbox filesystem.
    async fn read_file(
        &self,
        handle: &SandboxHandle,
        path: &str,
    ) -> Result<Vec<u8>, SandboxError>;

    /// Write a file to the sandbox filesystem.
    async fn write_file(
        &self,
        handle: &SandboxHandle,
        path: &str,
        content: &[u8],
    ) -> Result<(), SandboxError>;

    /// Destroy the sandbox.
    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError>;

    /// Get the status of the sandbox.
    async fn status(&self, handle: &SandboxHandle) -> Result<String, SandboxError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mount(host: &str, container: &str) -> SourceMount {
        SourceMount {
            host_path: host.to_string(),
            container_path: container.to_string(),
            label: None,
        }
    }

    #[test]
    fn sandbox_config_sources_default_empty() {
        let config = SandboxConfig::default();
        assert!(config.sources.is_empty());
    }

    #[test]
    fn sandbox_config_with_sources() {
        let config = SandboxConfig {
            sources: vec![make_mount("/host/ref", "/sources/ref")],
            ..Default::default()
        };
        assert_eq!(config.sources.len(), 1);
        assert_eq!(config.sources[0].container_path, "/sources/ref");
    }

    #[test]
    fn validate_sources_accepts_valid_paths() {
        let sources = vec![
            make_mount("/host/a", "/sources/a"),
            make_mount("/host/b", "/sources/b/nested"),
        ];
        assert!(validate_sources(&sources).is_ok());
    }

    #[test]
    fn validate_sources_rejects_dotdot() {
        let sources = vec![make_mount("/host/a", "/sources/../etc")];
        let err = validate_sources(&sources).unwrap_err();
        assert!(matches!(err, SandboxError::InvalidSourceMount { .. }));
        assert!(err.to_string().contains(".."));
    }

    #[test]
    fn validate_sources_rejects_wrong_prefix() {
        let sources = vec![make_mount("/host/a", "/data/a")];
        let err = validate_sources(&sources).unwrap_err();
        assert!(matches!(err, SandboxError::InvalidSourceMount { .. }));
        assert!(err.to_string().contains("/sources/"));
    }

    #[test]
    fn validate_sources_rejects_root_sources_exact() {
        // "/sources/" requires something after the slash
        let sources = vec![make_mount("/host/a", "/other/sources/a")];
        let err = validate_sources(&sources).unwrap_err();
        assert!(matches!(err, SandboxError::InvalidSourceMount { .. }));
    }

    #[test]
    fn validate_sources_empty_ok() {
        assert!(validate_sources(&[]).is_ok());
    }

    // --- Source mount edge cases ---

    #[test]
    fn validate_sources_rejects_dotdot_in_middle_of_path() {
        // Traversal hidden in middle of path
        let sources = vec![make_mount("/host/a", "/sources/a/../../../etc/passwd")];
        let err = validate_sources(&sources).unwrap_err();
        assert!(matches!(err, SandboxError::InvalidSourceMount { .. }));
    }

    #[test]
    fn validate_sources_rejects_dotdot_at_start() {
        // Path starting with ../
        let sources = vec![make_mount("/host/a", "../sources/a")];
        let err = validate_sources(&sources).unwrap_err();
        // Fails either because of ".." or missing "/sources/" prefix
        assert!(matches!(err, SandboxError::InvalidSourceMount { .. }));
    }

    #[test]
    fn validate_sources_accepts_deeply_nested_valid_path() {
        let sources = vec![make_mount(
            "/host/deep",
            "/sources/project/src/deeply/nested/dir",
        )];
        assert!(validate_sources(&sources).is_ok());
    }

    #[test]
    fn validate_sources_rejects_sources_without_leading_slash() {
        // Must start with /sources/, not sources/
        let sources = vec![make_mount("/host/a", "sources/a")];
        let err = validate_sources(&sources).unwrap_err();
        assert!(matches!(err, SandboxError::InvalidSourceMount { .. }));
    }

    #[test]
    fn validate_sources_rejects_path_containing_sources_not_at_start() {
        // /data/sources/a — not starting with /sources/
        let sources = vec![make_mount("/host/a", "/data/sources/a")];
        let err = validate_sources(&sources).unwrap_err();
        assert!(matches!(err, SandboxError::InvalidSourceMount { .. }));
    }

    #[test]
    fn validate_sources_rejects_first_bad_in_mixed_list() {
        // First mount is bad, second is good — should short-circuit on first error
        let sources = vec![
            make_mount("/host/bad", "/data/bad"),
            make_mount("/host/ok", "/sources/ok"),
        ];
        let err = validate_sources(&sources).unwrap_err();
        assert!(matches!(err, SandboxError::InvalidSourceMount { .. }));
    }

    // --- SandboxConfig additional_mounts ---

    #[test]
    fn sandbox_config_additional_mounts_default_empty() {
        let config = SandboxConfig::default();
        assert!(config.additional_mounts.is_empty());
    }

    #[test]
    fn sandbox_config_additional_mount_read_only_flag() {
        let config = SandboxConfig {
            additional_mounts: vec![
                MountSpec {
                    host_path: "/host/ro".to_string(),
                    container_path: "/mnt/ro".to_string(),
                    read_only: true,
                },
                MountSpec {
                    host_path: "/host/rw".to_string(),
                    container_path: "/mnt/rw".to_string(),
                    read_only: false,
                },
            ],
            ..Default::default()
        };
        assert!(config.additional_mounts[0].read_only);
        assert!(!config.additional_mounts[1].read_only);
    }

    // --- SandboxError display ---

    #[test]
    fn sandbox_error_not_implemented_display() {
        assert_eq!(SandboxError::NotImplemented.to_string(), "not implemented for this provider");
    }

    #[test]
    fn sandbox_error_not_found_display() {
        assert_eq!(SandboxError::NotFound.to_string(), "sandbox not found");
    }

    #[test]
    fn sandbox_error_policy_violation_display() {
        let err = SandboxError::PolicyViolation {
            reason: "network denied".to_string(),
        };
        assert!(err.to_string().contains("network denied"));
    }

    #[test]
    fn sandbox_error_invalid_source_mount_display() {
        let err = SandboxError::InvalidSourceMount {
            reason: "bad prefix".to_string(),
        };
        assert!(err.to_string().contains("bad prefix"));
    }
}
