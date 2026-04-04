//! SERA Docker — container lifecycle management via `bollard`.
//!
//! Replaces: SandboxManager, ContainerSecurityMapper, BindMountBuilder, WorktreeManager
//! from the TypeScript codebase.

pub mod container;
pub mod error;

pub use container::ContainerManager;
pub use error::DockerError;
