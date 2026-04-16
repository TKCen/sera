use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use serde_json::json;
use sera_config::config_store::{ConfigStore, ConfigStoreError, ManifestValue};
use sera_config::shadow_store::ShadowConfigStore;

/// Minimal in-memory ConfigStore for testing.
struct MemStore {
    data: Mutex<HashMap<String, ManifestValue>>,
}

impl MemStore {
    fn from_pairs(pairs: &[(&str, ManifestValue)]) -> Self {
        Self {
            data: Mutex::new(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()),
        }
    }
}

#[async_trait]
impl ConfigStore for MemStore {
    async fn get(&self, key: &str) -> Result<Option<ManifestValue>, ConfigStoreError> {
        Ok(self.data.lock().unwrap().get(key).cloned())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<(String, ManifestValue)>, ConfigStoreError> {
        let data = self.data.lock().unwrap();
        Ok(data.iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    async fn version(&self) -> Result<u64, ConfigStoreError> {
        Ok(1)
    }

    async fn put(&self, key: &str, value: ManifestValue) -> Result<(), ConfigStoreError> {
        self.data.lock().unwrap().insert(key.to_string(), value);
        Ok(())
    }
}

#[tokio::test]
async fn shadow_overlay_shadows_prod_value() {
    let prod = Arc::new(MemStore::from_pairs(&[("foo", json!("prod_value"))]));
    let shadow = ShadowConfigStore::new(prod);

    shadow.overlay_put("foo", json!("overlay_value")).await;

    let result = shadow.get("foo").await.unwrap();
    assert_eq!(result, Some(json!("overlay_value")));
}

#[tokio::test]
async fn shadow_fallthrough_returns_prod_for_unset_keys() {
    let prod = Arc::new(MemStore::from_pairs(&[("bar", json!(42))]));
    let shadow = ShadowConfigStore::new(prod);

    // "bar" is not in the overlay, should fall through to prod
    let result = shadow.get("bar").await.unwrap();
    assert_eq!(result, Some(json!(42)));
}

#[tokio::test]
async fn shadow_discard_removes_overlay() {
    let prod = Arc::new(MemStore::from_pairs(&[("baz", json!("prod"))]));
    let shadow = ShadowConfigStore::new(prod);

    shadow.overlay_put("baz", json!("overlay")).await;
    assert!(shadow.is_dirty().await);

    shadow.discard().await;
    assert!(!shadow.is_dirty().await);

    // After discard, should fall through to prod
    let result = shadow.get("baz").await.unwrap();
    assert_eq!(result, Some(json!("prod")));
}

#[tokio::test]
async fn shadow_commit_overlay_persists_to_prod() {
    let prod = Arc::new(MemStore::from_pairs(&[("existing", json!("prod_value"))]));
    let shadow = ShadowConfigStore::new(prod.clone());

    // Overlay a new key and an existing key
    shadow.overlay_put("new_key", json!("overlay_new")).await;
    shadow.overlay_put("existing", json!("overlay_updated")).await;

    // Before commit, overlay takes precedence
    assert_eq!(shadow.get("new_key").await.unwrap(), Some(json!("overlay_new")));
    assert_eq!(shadow.get("existing").await.unwrap(), Some(json!("overlay_updated")));

    // Commit to prod
    shadow.commit_overlay().await.unwrap();

    // After commit, overlay is cleared and prod has the values
    assert!(!shadow.is_dirty().await);

    // Prod store has the committed values
    assert_eq!(prod.get("new_key").await.unwrap(), Some(json!("overlay_new")));
    assert_eq!(prod.get("existing").await.unwrap(), Some(json!("overlay_updated")));
}
