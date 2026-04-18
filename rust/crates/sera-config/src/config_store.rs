//! ConfigStore trait — uniform key/value access over config backends.

use async_trait::async_trait;

/// A manifest value — any JSON-compatible value.
pub type ManifestValue = serde_json::Value;

/// Errors from ConfigStore operations.
#[derive(Debug, thiserror::Error)]
pub enum ConfigStoreError {
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("serialisation error: {0}")]
    Serialise(String),
    #[error("backend error: {0}")]
    Backend(String),
}

/// Async trait for read-only access to a config backend.
#[async_trait]
pub trait ConfigStore: Send + Sync + 'static {
    /// Retrieve the value for `key`, or `None` if absent.
    async fn get(&self, key: &str) -> Result<Option<ManifestValue>, ConfigStoreError>;

    /// List all key/value pairs whose key starts with `prefix`.
    async fn list(&self, prefix: &str) -> Result<Vec<(String, ManifestValue)>, ConfigStoreError>;

    /// Return a monotonically increasing version counter for change detection.
    async fn version(&self) -> Result<u64, ConfigStoreError>;

    /// Write a value to the store. Override this to make a store writable.
    async fn put(&self, _key: &str, _value: ManifestValue) -> Result<(), ConfigStoreError> {
        Err(ConfigStoreError::Backend("store is read-only".to_string()))
    }
}
