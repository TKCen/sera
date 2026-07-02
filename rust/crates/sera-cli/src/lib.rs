//! `sera-cli` library — exposes internals for integration tests.

pub mod admin_client;
pub mod circle_closeout;
pub mod commands;
pub mod config;
pub mod http;
pub mod sse;
pub mod token_store;

use sera_commands::CommandRegistry;

/// Build the default command registry with all CLI commands registered.
pub fn build_registry() -> CommandRegistry {
    let mut registry = CommandRegistry::new();
    registry.register(commands::HealthCheckCommand);
    registry.register(commands::LoginCommand::new());
    registry.register(commands::WhoamiCommand::new());
    registry.register(commands::LogoutCommand::new());
    registry.register(commands::AgentListCommand::new());
    registry.register(commands::AgentShowCommand::new());
    registry.register(commands::AgentRunCommand::new());
    registry.register(commands::ChatCommand::new());
    registry.register(commands::ProviderListCommand);
    registry.register(commands::ProviderAddCommand::new());
    registry.register(commands::ProviderRemoveCommand::new());
    registry.register(commands::ProviderSelectCommand);
    registry.register(commands::ProviderConfigureCommand::new());
    registry.register(commands::InitCommand::new());
    registry.register(commands::WorkflowListCommand);
    registry.register(commands::WorkflowShowCommand);
    registry.register(commands::WorkflowCreateCommand);
    registry.register(commands::WorkflowDeleteCommand);
    registry.register(commands::PolicyListCommand);
    registry.register(commands::PolicyShowCommand);
    registry.register(commands::PolicyReloadCommand);
    registry.register(commands::PolicyValidateCommand);
    registry.register(commands::SessionListCommand);
    registry.register(commands::SessionShowCommand);
    registry.register(commands::SessionKillCommand);
    registry.register(commands::SessionBrowseCommand);
    registry.register(commands::KillswitchArmCommand);
    registry.register(commands::KillswitchDisarmCommand);
    registry.register(commands::KillswitchStatusCommand);
    registry.register(commands::ConfigGetCommand);
    registry.register(commands::ConfigSetCommand);
    registry.register(commands::ConfigEditCommand);
    registry.register(commands::ConfigPathCommand);
    registry
}
