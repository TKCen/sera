//! `sera-cli` library — exposes internals for integration tests.

pub mod commands;
pub mod config;
pub mod http;

use sera_commands::CommandRegistry;

/// Build the default command registry with all CLI commands registered.
pub fn build_registry() -> CommandRegistry {
    let mut registry = CommandRegistry::new();
    registry.register(commands::PingCommand);
    registry
}
