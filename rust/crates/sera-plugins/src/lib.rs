//! `sera-plugins` — dual-transport (gRPC + stdio) plugin registry and SDK for SERA.
//!
//! Plugins are out-of-process services that implement SERA trait contracts over
//! the wire. Two transports are supported: **gRPC** (TCP, mTLS) and **stdio**
//! (spawned child process, stdin/stdout framed JSON-RPC). Both expose the same
//! capability set, manifest shape, supervision model, and audit envelope.
//!
//! This crate provides:
//!
//! - [`types`] — core data types (registration, capability, health, transport config)
//! - [`registry`] — [`PluginRegistry`] trait + [`InMemoryPluginRegistry`] (with stdio lifecycle)
//! - [`manifest`] — YAML manifest parsing (`kind: Plugin` and `sera/v1` flat format)
//! - [`circuit_breaker`] — three-state circuit breaker for failure isolation
//! - [`error`] — [`PluginError`] with [`From`] impl into [`SeraError`]
//!
//! # Usage pattern
//!
//! ```rust,ignore
//! use sera_plugins::{
//!     InMemoryPluginRegistry, PluginRegistry,
//!     manifest::PluginManifest,
//!     CircuitBreaker,
//! };
//! use std::time::Duration;
//!
//! // 1. Parse a manifest and obtain a registration
//! let manifest = PluginManifest::from_yaml(YAML)?;
//! let registration = manifest.into_registration()?;
//! let plugin_name = registration.name.clone();
//!
//! // 2. Register in the in-memory registry
//! let registry = InMemoryPluginRegistry::new();
//! registry.register(registration).await?;
//!
//! // 3. Guard calls with a circuit breaker
//! let cb = CircuitBreaker::new(&plugin_name, 3, Duration::from_secs(30));
//! cb.allow()?;                 // returns Err(PluginError::CircuitOpen) when tripped
//! cb.record_success();         // call after a successful RPC / stdio exchange
//! cb.record_failure();         // call after a failed RPC / stdio exchange
//! ```

pub mod circuit_breaker;
pub mod error;
pub mod manifest;
pub mod registry;
pub mod types;

// ── Error ────────────────────────────────────────────────────────────────────
pub use error::PluginError;

// ── Registry ─────────────────────────────────────────────────────────────────
pub use registry::{InMemoryPluginRegistry, PluginRegistry};

// ── Types ────────────────────────────────────────────────────────────────────
pub use types::{
    GrpcTransportConfig, PluginCapability, PluginHealth, PluginInfo, PluginRegistration,
    PluginTransport, PluginVersion, StdioTransportConfig, TlsConfig,
};

// ── Manifest ─────────────────────────────────────────────────────────────────
/// Re-exported manifest types for callers that want flat imports.
pub use manifest::{
    ManifestMetadata, ManifestSpec, PluginKind, PluginManifest, PluginManifestV1, PluginService,
    PluginVolume,
};

// ── Circuit breaker ──────────────────────────────────────────────────────────
pub use circuit_breaker::{CircuitBreaker, CircuitState};
