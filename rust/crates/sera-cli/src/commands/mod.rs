//! CLI command implementations.
//!
//! Each module contains a [`sera_commands::Command`] implementation that is
//! registered in [`crate::build_registry`].

pub mod agent;
pub mod auth;
pub mod chat;
pub mod config;
pub mod init;
pub mod killswitch;
pub mod ping;
pub mod policy;
pub mod provider;
pub mod session;
pub mod workflow;

pub use agent::{AgentListCommand, AgentRunCommand, AgentShowCommand};
pub use auth::{LoginCommand, LogoutCommand, WhoamiCommand};
pub use chat::ChatCommand;
pub use config::{ConfigEditCommand, ConfigGetCommand, ConfigPathCommand, ConfigSetCommand};
pub use init::InitCommand;
pub use killswitch::{KillswitchArmCommand, KillswitchDisarmCommand, KillswitchStatusCommand};
pub use ping::HealthCheckCommand;
pub use policy::{
    PolicyListCommand, PolicyReloadCommand, PolicyShowCommand, PolicyValidateCommand,
};
pub use provider::{
    ProviderAddCommand, ProviderConfigureCommand, ProviderListCommand, ProviderRemoveCommand,
    ProviderSelectCommand,
};
pub use session::{
    SessionBrowseCommand, SessionKillCommand, SessionListCommand, SessionShowCommand,
};
pub use workflow::{
    WorkflowCreateCommand, WorkflowDeleteCommand, WorkflowListCommand, WorkflowShowCommand,
};
