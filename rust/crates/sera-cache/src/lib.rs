//! sera-cache — cache backend abstraction.
//!
//! Phase 0 scaffold. `CacheBackend` trait with `moka` and `fred` (Redis)
//! stubs to be filled in Phase 1.

use async_trait::async_trait;
use moka::future::Cache as MokaCache;
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
/// Phase 0: trait definition + MokaBackend (in-process).
/// Phase 1 adds `FredBackend` (Redis) implementation.
#[async_trait]
pub trait CacheBackend: Send + Sync + 'static {
    async fn get(&self, key: &str) -> Result<Option<serde_json::Value>, CacheError>;
    async fn set(&self, key: &str, value: serde_json::Value, ttl_secs: Option<u64>) -> Result<(), CacheError>;
    async fn delete(&self, key: &str) -> Result<(), CacheError>;
}

/// In-process cache using Moka.
///
/// Suitable for single-instance deployments. Phase 1 adds Redis via `fred`.
pub struct MokaBackend {
    cache: MokaCache<String, serde_json::Value>,
}

impl MokaBackend {
    pub fn new(max_capacity: u64) -> Self {
        let cache = MokaCache::builder()
            .max_capacity(max_capacity)
            .build();
        Self { cache }
    }
}

#[async_trait]
impl CacheBackend for MokaBackend {
    async fn get(&self, key: &str) -> Result<Option<serde_json::Value>, CacheError> {
        Ok(self.cache.get(key).await)
    }

    async fn set(
        &self,
        key: &str,
        value: serde_json::Value,
        _ttl_secs: Option<u64>,
    ) -> Result<(), CacheError> {
        // Note: TTL not yet supported in moka future API - builder().time_to_live() not exposed
        self.cache.insert(key.to_owned(), value).await;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        self.cache.invalidate(key).await;
        Ok(())
    }
}
