use std::collections::HashMap;

use sera_types::hook::HookMetadata;
use tracing::debug;

use crate::hook_trait::Hook;

/// In-process hook registry.
///
/// Hooks are keyed by the name returned from [`Hook::metadata`]. Only one
/// hook with a given name can be registered at a time — re-registering
/// replaces the existing entry.
#[derive(Default)]
pub struct HookRegistry {
    hooks: HashMap<String, Box<dyn Hook>>,
}

impl HookRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a hook. If a hook with the same name already exists it is
    /// replaced and the old one is dropped.
    pub fn register(&mut self, hook: Box<dyn Hook>) {
        let name = hook.metadata().name.clone();
        debug!(hook = %name, "registering hook");
        self.hooks.insert(name, hook);
    }

    /// Look up a hook by name.
    pub fn get(&self, name: &str) -> Option<&dyn Hook> {
        self.hooks.get(name).map(|h| h.as_ref())
    }

    /// Remove a hook by name. Returns `true` if the hook existed.
    pub fn unregister(&mut self, name: &str) -> bool {
        let removed = self.hooks.remove(name).is_some();
        if removed {
            debug!(hook = %name, "unregistered hook");
        }
        removed
    }

    /// List metadata for all registered hooks (owned copies).
    pub fn list(&self) -> Vec<HookMetadata> {
        self.hooks.values().map(|h| h.metadata()).collect()
    }

    /// Check whether a hook with the given name is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.hooks.contains_key(name)
    }
}
