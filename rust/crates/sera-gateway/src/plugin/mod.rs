//! Plugin registry — hooks for plugin events.

pub use crate::harness_dispatch::{
    new_plugin_registry, validate_plugin_event_namespace, PluginEvent, PluginRegistration,
    PluginRegistry,
};
