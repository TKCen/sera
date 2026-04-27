//! CLI command implementations.
//!
//! Each module contains a [`sera_commands::Command`] implementation that is
//! registered in [`crate::build_registry`].

pub mod agent;
pub mod auth;
pub mod chat;
pub mod init;
pub mod ping;
pub mod provider;

pub use agent::{AgentListCommand, AgentRunCommand, AgentShowCommand};
pub use auth::{LoginCommand, LogoutCommand, WhoamiCommand};
pub use chat::ChatCommand;
pub use init::InitCommand;
pub use ping::HealthCheckCommand;
pub use provider::{
    ProviderAddCommand, ProviderConfigureCommand, ProviderListCommand, ProviderRemoveCommand,
    ProviderSelectCommand,
};
