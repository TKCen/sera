//! Concrete `SkillSource` implementations.
//!
//! - [`fs::FileSystemSource`] — markdown packs on disk.
//! - [`plugin::PluginSource`]  — skills advertised by a running plugin
//!   (phase-M stub on phase 5).
//! - [`registry::RegistrySource`] — OCI-hosted skill packs.

pub mod fs;
pub mod plugin;
pub mod registry;

pub use fs::FileSystemSource;
pub use plugin::PluginSource;
pub use registry::{OciSkillPuller, RegistrySource};
