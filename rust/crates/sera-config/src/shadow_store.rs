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
/// Call `commit_overlay` to flush all pending overlay entries into the underlying
/// store and clear the overlay. Reads always check the overlay first, then fall
/// through to the prod store for keys not in the overlay.
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

    /// Apply overlay changes to the prod store, then clear the overlay.
    ///
    /// Semantics: "apply-all-and-clear" — every pending overlay entry is written
    /// to the underlying prod store in an unspecified order, and the overlay is
    /// cleared on success. This is the simplest useful behaviour for a shadow
    /// deployment: promote all staged changes atomically at the call site.
    ///
    /// If the prod store rejects any write (e.g. a read-only backend) the
    /// operation returns that error immediately and the overlay is left **intact**
    /// so the caller can inspect or retry the pending changes.
    pub async fn commit_overlay(&self) -> Result<(), ConfigStoreError> {
        // Snapshot the overlay entries without draining so that the overlay
        // remains intact if any prod write fails.
        let entries: Vec<(String, ManifestValue)> = {
            let guard = self.overlay.read().await;
            guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };

        for (key, value) in entries {
            self.prod.put(&key, value).await?;
        }

        // All writes succeeded — clear the overlay now.
        self.overlay.write().await.clear();
        Ok(())
    }
}
