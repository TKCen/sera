//! sera-secrets — secrets provider abstraction.
//!
//! Phase 0 scaffold. `SecretsProvider` trait + `EnvSecretsProvider`
//! (reads SERA_SECRET_* env vars). DockerSandboxProvider needs this
//! for secret injection at container create().

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretsError {
    #[error("secret not found: {key}")]
    NotFound { key: String },
    #[error("provider error: {reason}")]
    Provider { reason: String },
}

/// Minimal async secrets provider interface.
#[async_trait]
pub trait SecretsProvider: Send + Sync + 'static {
    async fn get_secret(&self, key: &str) -> Result<String, SecretsError>;
    async fn list_keys(&self) -> Result<Vec<String>, SecretsError>;
}

/// Reads secrets from `SERA_SECRET_*` environment variables.
///
/// E.g. `SERA_SECRET_DB_PASSWORD` → key `DB_PASSWORD`, value from env.
pub struct EnvSecretsProvider;

#[async_trait]
impl SecretsProvider for EnvSecretsProvider {
    async fn get_secret(&self, key: &str) -> Result<String, SecretsError> {
        let env_key = format!("SERA_SECRET_{key}");
        std::env::var(&env_key).map_err(|_| SecretsError::NotFound {
            key: key.to_owned(),
        })
    }

    async fn list_keys(&self) -> Result<Vec<String>, SecretsError> {
        Ok(std::env::vars()
            .filter_map(|(k, _)| k.strip_prefix("SERA_SECRET_").map(|s| s.to_owned()))
            .collect())
    }
}
