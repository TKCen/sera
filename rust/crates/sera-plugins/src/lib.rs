//! `sera-plugins` — gRPC plugin registry and SDK for SERA.
//!
//! Plugins are out-of-process services that implement SERA trait contracts over
//! the wire. This crate provides:
//!
//! - [`types`] — core data types (registration, capability, health, TLS)
//! - [`registry`] — [`PluginRegistry`] trait + [`InMemoryPluginRegistry`]
//! - [`manifest`] — YAML manifest parsing (`kind: Plugin`)
//! - [`circuit_breaker`] — three-state circuit breaker for failure isolation
//! - [`error`] — [`PluginError`] with [`From`] impl into [`SeraError`]
//!
//! # Quick start
//!
//! ```rust,ignore
//! use sera_plugins::{
//!     manifest::PluginManifest,
//!     registry::{InMemoryPluginRegistry, PluginRegistry},
//! };
//!
//! let registry = InMemoryPluginRegistry::new();
//! let manifest = PluginManifest::from_yaml(YAML)?;
//! let registration = manifest.into_registration()?;
//! registry.register(registration).await?;
//! ```

pub mod circuit_breaker;
pub mod error;
pub mod manifest;
pub mod registry;
pub mod types;

pub use error::PluginError;
pub use registry::{InMemoryPluginRegistry, PluginRegistry};
pub use types::{
    PluginCapability, PluginHealth, PluginInfo, PluginRegistration, PluginVersion, TlsConfig,
};
