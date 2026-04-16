//! sera-secrets — secrets provider abstraction.
//!
//! Provides a unified async `SecretsProvider` trait with multiple backends:
//!
//! - [`EnvSecretsProvider`] — reads `SERA_SECRET_*` environment variables (read-only)
//! - [`DockerSecretsProvider`] — reads Docker-mounted secrets from `/run/secrets/` (read-only)
//! - [`FileSecretsProvider`] — reads and writes secrets as files in a directory
//! - [`ChainedSecretsProvider`] — tries multiple providers in order for fallback
//!
//! Enterprise scaffolds (not yet implemented) live in [`enterprise`].

use async_trait::async_trait;
use thiserror::Error;

pub mod chained;
pub mod docker;
pub mod enterprise;
pub mod env;
pub mod file;

pub use chained::ChainedSecretsProvider;
pub use docker::DockerSecretsProvider;
pub use env::EnvSecretsProvider;
pub use file::FileSecretsProvider;

#[derive(Debug, Error)]
pub enum SecretsError {
    #[error("secret not found: {key}")]
    NotFound { key: String },
    #[error("provider error: {reason}")]
    Provider { reason: String },
    #[error("provider is read-only")]
    ReadOnly,
    #[error("I/O error: {reason}")]
    Io { reason: String },
}

/// Async secrets provider interface.
///
/// Implementations must be `Send + Sync + 'static` to be usable in async contexts
/// and as trait objects.
#[async_trait]
pub trait SecretsProvider: Send + Sync + 'static {
    /// Returns a human-readable name for this provider (e.g. `"env"`, `"docker"`).
    fn provider_name(&self) -> &str;

    /// Retrieve a secret by key. Returns [`SecretsError::NotFound`] if absent.
    async fn get_secret(&self, key: &str) -> Result<String, SecretsError>;

    /// List all available secret keys.
    async fn list_keys(&self) -> Result<Vec<String>, SecretsError>;

    /// Store a secret. Returns [`SecretsError::ReadOnly`] for read-only providers.
    async fn store(&self, key: &str, value: &str) -> Result<(), SecretsError>;

    /// Delete a secret. Returns [`SecretsError::ReadOnly`] for read-only providers,
    /// or [`SecretsError::NotFound`] if the key does not exist.
    async fn delete(&self, key: &str) -> Result<(), SecretsError>;
}
