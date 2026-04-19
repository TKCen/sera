use std::collections::HashMap;

use sera_types::hook::HookMetadata;
use tracing::debug;

use crate::hook_trait::Hook;

/// Execution tier for a registered hook.
///
/// Internal hooks are built-in Rust hooks that run first and whose
/// cancellations always propagate through the chain.  Plugin hooks
/// (third-party / WASM) run second; a Plugin-tier cancellation is
/// logged but does not prevent remaining Plugin hooks from running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HookTier {
    /// Built-in, first-party hook.  Runs before [`HookTier::Plugin`] hooks.
    /// Cancellation from an Internal hook aborts the whole chain.
    #[default]
    Internal,
    /// Third-party / WASM hook.  Runs after all [`HookTier::Internal`] hooks.
    /// A terminal result from a Plugin hook is logged and the remaining
    /// Plugin hooks are skipped, but Internal hooks are never affected.
    Plugin,
}

/// In-process hook registry.
///
/// Hooks are keyed by the name returned from [`Hook::metadata`]. Only one
/// hook with a given name can be registered at a time — re-registering
/// replaces the existing entry.
///
/// Hooks are stored in two separate buckets — one per [`HookTier`] — so
/// that [`crate::executor::ChainExecutor`] can dispatch Internal hooks
/// before Plugin hooks.
#[derive(Default)]
pub struct HookRegistry {
    hooks: HashMap<String, (Box<dyn Hook>, HookTier)>,
}

impl HookRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a hook at the default tier ([`HookTier::Internal`]).
    ///
    /// If a hook with the same name already exists it is replaced and the old
    /// one is dropped.
    pub fn register(&mut self, hook: Box<dyn Hook>) {
        self.register_with_tier(hook, HookTier::Internal);
    }

    /// Register a hook at an explicit [`HookTier`].
    ///
    /// If a hook with the same name already exists it is replaced and the old
    /// one is dropped.
    pub fn register_with_tier(&mut self, hook: Box<dyn Hook>, tier: HookTier) {
        let name = hook.metadata().name.clone();
        debug!(hook = %name, ?tier, "registering hook");
        self.hooks.insert(name, (hook, tier));
    }

    /// Look up a hook by name.
    pub fn get(&self, name: &str) -> Option<&dyn Hook> {
        self.hooks.get(name).map(|(h, _)| h.as_ref())
    }

    /// Return the tier of a registered hook, or `None` if not registered.
    pub fn tier(&self, name: &str) -> Option<HookTier> {
        self.hooks.get(name).map(|(_, t)| *t)
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
        self.hooks.values().map(|(h, _)| h.metadata()).collect()
    }

    /// List metadata for hooks in the given tier only.
    pub fn list_by_tier(&self, tier: HookTier) -> Vec<HookMetadata> {
        self.hooks
            .values()
            .filter(|(_, t)| *t == tier)
            .map(|(h, _)| h.metadata())
            .collect()
    }

    /// Check whether a hook with the given name is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.hooks.contains_key(name)
    }
}
