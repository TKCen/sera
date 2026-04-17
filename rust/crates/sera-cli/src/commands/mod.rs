//! CLI command implementations.
//!
//! Each module contains a [`sera_commands::Command`] implementation that is
//! registered in [`crate::build_registry`].

pub mod ping;

pub use ping::PingCommand;
