//! ShadowConfigStore — in-memory overlay over a production ConfigStore.
//!
//! Writes go to the overlay; reads check the overlay first, then fall through
//! to the underlying production store. Used for dry-run / shadow deployments.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config_store::{ConfigStore, ConfigStoreError, ManifestValue};

/// A read/write overlay over a production `ConfigStore`.
///
/// Values written via `overlay_put` shadow the prod store without mutating it.
/// Call `commit_overlay` to apply changes (unimplemented in this milestone).
pub struct ShadowConfigStore<S: ConfigStore> {
    prod: Arc<S>,
    overlay: RwLock<HashMap<String, ManifestValue>>,
}

impl<S: ConfigStore> ShadowConfigStore<S> {
    /// Wrap a production store in a shadow overlay.
    pub fn new(prod: Arc<S>) -> Self {
        Self {
            prod,
            overlay: RwLock::new(HashMap::new()),
        }
    }

    /// Write a value into the overlay (does not touch the prod store).
    pub async fn overlay_put(&self, key: impl Into<String>, value: ManifestValue) {
        self.overlay.write().await.insert(key.into(), value);
    }

    /// Read a value: overlay takes precedence over prod.
    pub async fn get(&self, key: &str) -> Result<Option<ManifestValue>, ConfigStoreError> {
        {
            let guard = self.overlay.read().await;
            if let Some(v) = guard.get(key) {
                return Ok(Some(v.clone()));
            }
        }
        self.prod.get(key).await
    }

    /// Returns `true` if the overlay contains any unsaved changes.
    pub async fn is_dirty(&self) -> bool {
        !self.overlay.read().await.is_empty()
    }

    /// Discard all overlay entries, restoring prod-only reads.
    pub async fn discard(&self) {
        self.overlay.write().await.clear();
    }

    /// Apply overlay changes to the prod store.
    ///
    /// # Panics
    ///
    /// This method is not yet implemented.
    pub async fn commit_overlay(&self) -> Result<(), ConfigStoreError> {
        unimplemented!("commit_overlay is not yet implemented for this milestone")
    }
}
