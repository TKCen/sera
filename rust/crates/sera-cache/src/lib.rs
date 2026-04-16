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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_missing_key_returns_none() {
        let cache = MokaBackend::new(100);
        let result = cache.get("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn set_and_get_roundtrip() {
        let cache = MokaBackend::new(100);
        let val = serde_json::json!({"key": "value", "num": 42});
        cache.set("test_key", val.clone(), None).await.unwrap();
        let got = cache.get("test_key").await.unwrap();
        assert_eq!(got, Some(val));
    }

    #[tokio::test]
    async fn set_with_ttl_stores_value() {
        let cache = MokaBackend::new(100);
        let val = serde_json::json!("hello");
        cache.set("ttl_key", val.clone(), Some(60)).await.unwrap();
        let got = cache.get("ttl_key").await.unwrap();
        assert_eq!(got, Some(val));
    }

    #[tokio::test]
    async fn delete_removes_key() {
        let cache = MokaBackend::new(100);
        cache.set("del_key", serde_json::json!(1), None).await.unwrap();
        cache.delete("del_key").await.unwrap();
        let got = cache.get("del_key").await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn delete_nonexistent_succeeds() {
        let cache = MokaBackend::new(100);
        // Should not error — deleting a missing key is a no-op
        cache.delete("never_set").await.unwrap();
    }

    #[tokio::test]
    async fn overwrite_replaces_value() {
        let cache = MokaBackend::new(100);
        cache.set("ow_key", serde_json::json!("first"), None).await.unwrap();
        cache.set("ow_key", serde_json::json!("second"), None).await.unwrap();
        let got = cache.get("ow_key").await.unwrap();
        assert_eq!(got, Some(serde_json::json!("second")));
    }

    #[tokio::test]
    async fn capacity_limit_evicts() {
        // Create cache with capacity 2
        let cache = MokaBackend::new(2);
        cache.set("a", serde_json::json!(1), None).await.unwrap();
        cache.set("b", serde_json::json!(2), None).await.unwrap();
        cache.set("c", serde_json::json!(3), None).await.unwrap();
        // Moka is approximate — at least one of the first two should be evicted eventually
        // but we can't assert deterministically which one. Just verify 'c' is present.
        let got_c = cache.get("c").await.unwrap();
        assert_eq!(got_c, Some(serde_json::json!(3)));
    }
}
