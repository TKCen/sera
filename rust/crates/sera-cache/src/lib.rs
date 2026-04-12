//! sera-cache — cache backend abstraction.
//!
//! Phase 0 scaffold. `CacheBackend` trait with `moka` and `fred` (Redis)
//! stubs to be filled in Phase 1.

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("cache miss: {key}")]
    Miss { key: String },
    #[error("backend error: {reason}")]
    Backend { reason: String },
}

/// Minimal async cache interface.
///
/// Phase 0: trait definition only. Phase 1 adds `MokaBackend` (in-process)
/// and `FredBackend` (Redis) implementations.
#[async_trait]
pub trait CacheBackend: Send + Sync + 'static {
    async fn get(&self, key: &str) -> Result<Option<serde_json::Value>, CacheError>;
    async fn set(&self, key: &str, value: serde_json::Value, ttl_secs: Option<u64>) -> Result<(), CacheError>;
    async fn delete(&self, key: &str) -> Result<(), CacheError>;
}
