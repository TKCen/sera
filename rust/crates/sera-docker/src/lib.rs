//! SERA Docker — container lifecycle management via `bollard`.
//!
//! Replaces: SandboxManager, ContainerSecurityMapper, BindMountBuilder, WorktreeManager
//! from the TypeScript codebase.

pub mod container;
pub mod error;

pub use container::ContainerManager;
pub use error::DockerError;

/// Output from executing a command in a container.
#[derive(Debug, serde::Serialize)]
pub struct ExecOutput {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
}
