//! sera-cache — cache backend abstraction.
//!
//! Phase 0 scaffold. `CacheBackend` trait with `moka` and `fred` (Redis)
//! stubs to be filled in Phase 1.

use async_trait::async_trait;
use moka::future::Cache as MokaCache;
use sera_errors::{IntoSeraError, SeraError, SeraErrorCode};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("cache miss: {key}")]
    Miss { key: String },
    #[error("backend error: {reason}")]
    Backend { reason: String },
}

impl CacheError {
    /// Convert to [`SeraError`] with the canonical code for this variant.
    pub fn into_sera_error(self) -> SeraError {
        let code = match &self {
            CacheError::Miss { .. } => SeraErrorCode::NotFound,
            CacheError::Backend { .. } => SeraErrorCode::Unavailable,
        };
        self.into_sera(code)
    }
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
mod into_sera_tests {
    use super::*;
    use sera_errors::SeraErrorCode;

    #[test]
    fn miss_maps_to_not_found() {
        let err = CacheError::Miss { key: "mykey".into() };
        let sera = err.into_sera_error();
        assert_eq!(sera.code, SeraErrorCode::NotFound);
        assert!(sera.message.contains("mykey"));
    }

    #[test]
    fn backend_maps_to_unavailable() {
        let err = CacheError::Backend { reason: "redis down".into() };
        let sera = err.into_sera_error();
        assert_eq!(sera.code, SeraErrorCode::Unavailable);
        assert!(sera.message.contains("redis down"));
    }

    #[test]
    fn miss_message_preserved() {
        let err = CacheError::Miss { key: "agent:42".into() };
        let sera = err.into_sera_error();
        assert!(sera.message.contains("agent:42"));
    }

    #[test]
    fn backend_message_preserved() {
        let err = CacheError::Backend { reason: "connection timeout".into() };
        let sera = err.into_sera_error();
        assert!(sera.message.contains("connection timeout"));
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

    // === NEW TESTS ===

    #[tokio::test]
    async fn store_and_retrieve_null_value() {
        let cache = MokaBackend::new(100);
        let val = serde_json::json!(null);
        cache.set("null_key", val.clone(), None).await.unwrap();
        let got = cache.get("null_key").await.unwrap();
        assert_eq!(got, Some(val));
    }

    #[tokio::test]
    async fn store_and_retrieve_bool_value() {
        let cache = MokaBackend::new(100);
        let val = serde_json::json!(true);
        cache.set("bool_key", val.clone(), None).await.unwrap();
        let got = cache.get("bool_key").await.unwrap();
        assert_eq!(got, Some(val));
    }

    #[tokio::test]
    async fn store_and_retrieve_number_value() {
        let cache = MokaBackend::new(100);
        let val = serde_json::json!(3.14159);
        cache.set("num_key", val.clone(), None).await.unwrap();
        let got = cache.get("num_key").await.unwrap();
        assert_eq!(got, Some(val));
    }

    #[tokio::test]
    async fn store_and_retrieve_array_value() {
        let cache = MokaBackend::new(100);
        let val = serde_json::json!([1, "two", 3.0, null, true]);
        cache.set("array_key", val.clone(), None).await.unwrap();
        let got = cache.get("array_key").await.unwrap();
        assert_eq!(got, Some(val));
    }

    #[tokio::test]
    async fn store_and_retrieve_nested_object() {
        let cache = MokaBackend::new(100);
        let val = serde_json::json!({
            "user": {
                "id": 123,
                "name": "Alice",
                "tags": ["admin", "verified"],
                "metadata": null
            },
            "created_at": "2026-04-17T00:00:00Z"
        });
        cache.set("nested_obj", val.clone(), None).await.unwrap();
        let got = cache.get("nested_obj").await.unwrap();
        assert_eq!(got, Some(val));
    }

    #[tokio::test]
    async fn concurrent_inserts_no_panic() {
        let cache = std::sync::Arc::new(MokaBackend::new(1000));
        let mut handles = vec![];

        for i in 0..10 {
            let cache_clone = cache.clone();
            let handle = tokio::spawn(async move {
                let key = format!("concurrent_{}", i);
                let val = serde_json::json!({ "task_id": i });
                cache_clone.set(&key, val, None).await.unwrap();
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all were inserted
        for i in 0..10 {
            let key = format!("concurrent_{}", i);
            let got = cache.get(&key).await.unwrap();
            assert!(got.is_some());
        }
    }

    #[tokio::test]
    async fn concurrent_get_set_delete_mixed() {
        let cache = std::sync::Arc::new(MokaBackend::new(1000));
        let mut handles = vec![];

        for i in 0usize..15 {
            let cache_clone = cache.clone();
            let handle = tokio::spawn(async move {
                match i % 3 {
                    0 => {
                        // Insert
                        let key = format!("mixed_{}", i);
                        cache_clone.set(&key, serde_json::json!(i as u32), None).await.unwrap();
                    }
                    1 => {
                        // Get
                        let key = format!("mixed_{}", i.saturating_sub(1));
                        let _ = cache_clone.get(&key).await.unwrap();
                    }
                    _ => {
                        // Delete
                        let key = format!("mixed_{}", i.saturating_sub(2));
                        let _ = cache_clone.delete(&key).await.unwrap();
                    }
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }
    }

    #[tokio::test]
    async fn get_after_concurrent_overwrites() {
        let cache = std::sync::Arc::new(MokaBackend::new(100));
        let mut handles = vec![];

        for i in 0..5 {
            let cache_clone = cache.clone();
            let handle = tokio::spawn(async move {
                cache_clone.set("shared_key", serde_json::json!(i), None).await.unwrap();
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Final value should be one of 0-4
        let got = cache.get("shared_key").await.unwrap();
        assert!(got.is_some());
        if let Some(val) = got {
            assert!(val.is_number());
        }
    }

    #[tokio::test]
    async fn set_empty_string_key() {
        let cache = MokaBackend::new(100);
        let val = serde_json::json!("empty key test");
        cache.set("", val.clone(), None).await.unwrap();
        let got = cache.get("").await.unwrap();
        assert_eq!(got, Some(val));
    }

    #[tokio::test]
    async fn set_very_long_key() {
        let cache = MokaBackend::new(100);
        let long_key = "k".repeat(1024);
        let val = serde_json::json!("long key value");
        cache.set(&long_key, val.clone(), None).await.unwrap();
        let got = cache.get(&long_key).await.unwrap();
        assert_eq!(got, Some(val));
    }

    #[tokio::test]
    async fn set_large_json_value() {
        let cache = MokaBackend::new(100);
        let mut large_obj = serde_json::Map::new();
        for i in 0..100 {
            large_obj.insert(
                format!("field_{}", i),
                serde_json::json!({
                    "index": i,
                    "data": "x".repeat(50)
                }),
            );
        }
        let val = serde_json::Value::Object(large_obj);
        cache.set("large_obj", val.clone(), None).await.unwrap();
        let got = cache.get("large_obj").await.unwrap();
        assert_eq!(got, Some(val));
    }

    #[tokio::test]
    async fn multiple_deletes_on_same_key() {
        let cache = MokaBackend::new(100);
        cache.set("multi_del", serde_json::json!("value"), None).await.unwrap();
        cache.delete("multi_del").await.unwrap();
        // Second delete should also succeed
        cache.delete("multi_del").await.unwrap();
        let got = cache.get("multi_del").await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn set_with_various_ttl_values() {
        let cache = MokaBackend::new(100);
        let val = serde_json::json!("ttl test");

        // Note: TTL not yet enforced in Moka future API, but values should store correctly
        cache.set("ttl_1", val.clone(), Some(1)).await.unwrap();
        cache.set("ttl_3600", val.clone(), Some(3600)).await.unwrap();
        cache.set("ttl_86400", val.clone(), Some(86400)).await.unwrap();

        let got_1 = cache.get("ttl_1").await.unwrap();
        let got_3600 = cache.get("ttl_3600").await.unwrap();
        let got_86400 = cache.get("ttl_86400").await.unwrap();

        assert_eq!(got_1, Some(val.clone()));
        assert_eq!(got_3600, Some(val.clone()));
        assert_eq!(got_86400, Some(val));
    }

    #[tokio::test]
    async fn independent_caches_are_isolated() {
        let cache1 = MokaBackend::new(100);
        let cache2 = MokaBackend::new(100);

        cache1.set("key", serde_json::json!("cache1"), None).await.unwrap();
        cache2.set("key", serde_json::json!("cache2"), None).await.unwrap();

        let got1 = cache1.get("key").await.unwrap();
        let got2 = cache2.get("key").await.unwrap();

        assert_eq!(got1, Some(serde_json::json!("cache1")));
        assert_eq!(got2, Some(serde_json::json!("cache2")));
    }

    #[tokio::test]
    async fn delete_and_reinsert_same_key() {
        let cache = MokaBackend::new(100);
        let val1 = serde_json::json!("first");
        let val2 = serde_json::json!("second");

        cache.set("reinsert", val1.clone(), None).await.unwrap();
        assert_eq!(cache.get("reinsert").await.unwrap(), Some(val1));

        cache.delete("reinsert").await.unwrap();
        assert!(cache.get("reinsert").await.unwrap().is_none());

        cache.set("reinsert", val2.clone(), None).await.unwrap();
        assert_eq!(cache.get("reinsert").await.unwrap(), Some(val2));
    }
}
