//! SERA Docker — container lifecycle management via `bollard`.
//!
//! Replaces: SandboxManager, ContainerSecurityMapper, BindMountBuilder, WorktreeManager
//! from the TypeScript codebase.

pub mod container;
pub mod error;
pub mod events;

pub use container::ContainerManager;
pub use error::DockerError;
pub use events::DockerEventListener;
pub use events::DockerEvent;

/// Output from executing a command in a container.
#[derive(Debug, serde::Serialize)]
pub struct ExecOutput {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
}
