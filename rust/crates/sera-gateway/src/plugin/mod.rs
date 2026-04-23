//! Plugin registry — hooks for plugin events.

pub use crate::harness_dispatch::{
    PluginEvent, PluginHookEntry, PluginRegistry, new_plugin_registry,
    validate_plugin_event_namespace,
};
