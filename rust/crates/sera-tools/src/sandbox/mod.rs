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
use thiserror::Error;

pub use policy::SandboxPolicy as SandboxPolicyType;

/// Opaque handle to a running sandbox.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SandboxHandle(pub String);

/// Configuration for creating a new sandbox.
#[derive(Debug, Clone, Default)]
pub struct SandboxConfig {
    pub image: Option<String>,
    pub sandbox_policy: Option<SandboxPolicy>,
    pub env: HashMap<String, String>,
    pub labels: HashMap<String, String>,
    pub memory_limit_bytes: Option<u64>,
    pub cpu_limit: Option<f64>,
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
