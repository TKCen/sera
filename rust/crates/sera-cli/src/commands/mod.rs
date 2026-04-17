//! CLI command implementations.
//!
//! Each module contains a [`sera_commands::Command`] implementation that is
//! registered in [`crate::build_registry`].

pub mod auth;
pub mod ping;

pub use auth::{LoginCommand, LogoutCommand, WhoamiCommand};
pub use ping::PingCommand;
