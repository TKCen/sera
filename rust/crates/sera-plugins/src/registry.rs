//! Plugin registry trait and in-memory implementation.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::error::PluginError;
use crate::types::{PluginCapability, PluginHealth, PluginInfo, PluginRegistration};

/// Registry of active plugins.
///
/// Implementors store, retrieve and query plugin registrations. The primary
/// implementation is [`InMemoryPluginRegistry`]; persistent backends can be
/// added later by implementing this trait.
#[async_trait]
pub trait PluginRegistry: Send + Sync {
    /// Register a new plugin. Returns [`PluginError::RegistrationFailed`] if a
    /// plugin with the same name already exists.
    async fn register(&self, registration: PluginRegistration) -> Result<(), PluginError>;

    /// Remove a plugin from the registry.
    async fn deregister(&self, name: &str) -> Result<(), PluginError>;

    /// Look up a single plugin by name.
    async fn get(&self, name: &str) -> Result<PluginInfo, PluginError>;

    /// List all registered plugins.
    async fn list(&self) -> Vec<PluginInfo>;

    /// Find all plugins that advertise a given capability.
    async fn find_by_capability(&self, cap: &PluginCapability) -> Vec<PluginInfo>;

    /// Update the health snapshot for a plugin.
    async fn update_health(&self, name: &str, health: PluginHealth) -> Result<(), PluginError>;
}

/// Thread-safe in-memory plugin registry backed by a `RwLock<HashMap>`.
#[derive(Debug, Default, Clone)]
pub struct InMemoryPluginRegistry {
    plugins: Arc<RwLock<HashMap<String, PluginInfo>>>,
}

impl InMemoryPluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PluginRegistry for InMemoryPluginRegistry {
    async fn register(&self, registration: PluginRegistration) -> Result<(), PluginError> {
        let mut guard = self.plugins.write().await;
        if guard.contains_key(&registration.name) {
            warn!(plugin = %registration.name, "plugin already registered");
            return Err(PluginError::RegistrationFailed {
                reason: format!("plugin '{}' is already registered", registration.name),
            });
        }
        let name = registration.name.clone();
        guard.insert(name.clone(), PluginInfo::new(registration));
        info!(plugin = %name, "plugin registered");
        Ok(())
    }

    async fn deregister(&self, name: &str) -> Result<(), PluginError> {
        let mut guard = self.plugins.write().await;
        if guard.remove(name).is_none() {
            return Err(PluginError::PluginNotFound { name: name.into() });
        }
        info!(plugin = %name, "plugin deregistered");
        Ok(())
    }

    async fn get(&self, name: &str) -> Result<PluginInfo, PluginError> {
        let guard = self.plugins.read().await;
        guard
            .get(name)
            .cloned()
            .ok_or_else(|| PluginError::PluginNotFound { name: name.into() })
    }

    async fn list(&self) -> Vec<PluginInfo> {
        let guard = self.plugins.read().await;
        guard.values().cloned().collect()
    }

    async fn find_by_capability(&self, cap: &PluginCapability) -> Vec<PluginInfo> {
        let guard = self.plugins.read().await;
        guard
            .values()
            .filter(|info| info.registration.capabilities.contains(cap))
            .cloned()
            .collect()
    }

    async fn update_health(&self, name: &str, health: PluginHealth) -> Result<(), PluginError> {
        let mut guard = self.plugins.write().await;
        match guard.get_mut(name) {
            Some(info) => {
                debug!(plugin = %name, healthy = %health.healthy, "health updated");
                info.health = health;
                Ok(())
            }
            None => Err(PluginError::PluginNotFound { name: name.into() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{PluginCapability, PluginVersion};
    use std::time::Duration;

    fn make_registration(name: &str, caps: Vec<PluginCapability>) -> PluginRegistration {
        PluginRegistration {
            name: name.into(),
            version: PluginVersion::new(1, 0, 0),
            capabilities: caps,
            endpoint: format!("localhost:{}", 9000),
            tls: None,
            health_check_interval: Duration::from_secs(30),
        }
    }

    #[tokio::test]
    async fn register_and_get() {
        let registry = InMemoryPluginRegistry::new();
        let reg = make_registration("my-plugin", vec![PluginCapability::ToolExecutor]);
        registry.register(reg).await.unwrap();
        let info = registry.get("my-plugin").await.unwrap();
        assert_eq!(info.registration.name, "my-plugin");
    }

    #[tokio::test]
    async fn duplicate_registration_fails() {
        let registry = InMemoryPluginRegistry::new();
        let reg = make_registration("dup", vec![]);
        registry.register(reg.clone()).await.unwrap();
        let err = registry.register(reg).await.unwrap_err();
        assert!(matches!(err, PluginError::RegistrationFailed { .. }));
    }

    #[tokio::test]
    async fn deregister_removes_plugin() {
        let registry = InMemoryPluginRegistry::new();
        registry
            .register(make_registration("to-remove", vec![]))
            .await
            .unwrap();
        registry.deregister("to-remove").await.unwrap();
        let err = registry.get("to-remove").await.unwrap_err();
        assert!(matches!(err, PluginError::PluginNotFound { .. }));
    }

    #[tokio::test]
    async fn deregister_missing_plugin_fails() {
        let registry = InMemoryPluginRegistry::new();
        let err = registry.deregister("ghost").await.unwrap_err();
        assert!(matches!(err, PluginError::PluginNotFound { .. }));
    }

    #[tokio::test]
    async fn list_returns_all() {
        let registry = InMemoryPluginRegistry::new();
        registry
            .register(make_registration("a", vec![]))
            .await
            .unwrap();
        registry
            .register(make_registration("b", vec![]))
            .await
            .unwrap();
        let all = registry.list().await;
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn find_by_capability_filters_correctly() {
        let registry = InMemoryPluginRegistry::new();
        registry
            .register(make_registration(
                "tool-plugin",
                vec![PluginCapability::ToolExecutor],
            ))
            .await
            .unwrap();
        registry
            .register(make_registration(
                "memory-plugin",
                vec![PluginCapability::MemoryBackend],
            ))
            .await
            .unwrap();

        let tools = registry
            .find_by_capability(&PluginCapability::ToolExecutor)
            .await;
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].registration.name, "tool-plugin");
    }

    #[tokio::test]
    async fn update_health_reflects_in_get() {
        let registry = InMemoryPluginRegistry::new();
        registry
            .register(make_registration("hp", vec![]))
            .await
            .unwrap();

        let health = PluginHealth::ok(15);
        registry.update_health("hp", health).await.unwrap();

        let info = registry.get("hp").await.unwrap();
        assert!(info.health.healthy);
        assert_eq!(info.health.latency_ms, Some(15));
    }

    #[tokio::test]
    async fn update_health_missing_plugin_fails() {
        let registry = InMemoryPluginRegistry::new();
        let err = registry
            .update_health("ghost", PluginHealth::initial())
            .await
            .unwrap_err();
        assert!(matches!(err, PluginError::PluginNotFound { .. }));
    }
}
