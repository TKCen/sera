//! sera-oci — OCI registry client for SERA plugin and skill pack distribution.
//!
//! **Ecosystem phase S scope** (see `docs/plan/PLUGIN-MCP-ECOSYSTEM.md` §3.5):
//! OCI *pull* of the plugin manifest YAML layer only. No signing, no push,
//! no binary-layer pull — those land in phases M and L.
//!
//! # Usage
//!
//! ```no_run
//! # async fn demo() -> Result<(), sera_oci::OciError> {
//! use sera_oci::{OciPuller, OciReference};
//!
//! let reference = OciReference::parse("ghcr.io/org/my-plugin:1.0.0")?;
//! let puller = OciPuller::new();
//! let manifest_bytes = puller.pull_manifest(&reference).await?;
//! // Hand `manifest_bytes.as_bytes()` to `sera_plugins::PluginManifestV1::from_yaml`.
//! # Ok(())
//! # }
//! ```
//!
//! The crate deliberately does **not** depend on `sera-plugins` — consumers
//! parse the returned YAML bytes themselves. This keeps the crate graph flat
//! and avoids forcing OCI pulls to carry the full plugin type surface.

pub mod auth;
pub mod error;
pub mod media_types;
pub mod puller;
pub mod reference;

pub use auth::{DockerConfig, RegistryCredential, credential_for, load_docker_config, load_docker_config_from_str};
pub use error::OciError;
pub use media_types::{PLUGIN_BINARY, PLUGIN_MANIFEST_V1_YAML, PLUGIN_PROTO};
pub use puller::{OciPuller, PluginManifestBytes};
pub use reference::OciReference;
