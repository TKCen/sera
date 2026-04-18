//! `sera-cli` library — exposes internals for integration tests.

pub mod commands;
pub mod config;
pub mod http;
pub mod sse;
pub mod token_store;

use sera_commands::CommandRegistry;

/// Build the default command registry with all CLI commands registered.
pub fn build_registry() -> CommandRegistry {
    let mut registry = CommandRegistry::new();
    registry.register(commands::PingCommand);
    registry.register(commands::LoginCommand::new());
    registry.register(commands::WhoamiCommand::new());
    registry.register(commands::LogoutCommand::new());
    registry.register(commands::AgentListCommand::new());
    registry.register(commands::AgentShowCommand::new());
    registry.register(commands::AgentRunCommand::new());
    registry.register(commands::ChatCommand::new());
    registry
}
