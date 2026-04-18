//! sera-commands — unified command registry shared between CLI and gateway.
//!
//! Define a command once by implementing [`Command`]; register it with
//! [`CommandRegistry`]; dispatch from either the CLI entry point or the HTTP
//! gateway without duplication.
//!
//! # Quick start
//!
//! ```rust
//! use sera_commands::{CommandRegistry, PingCommand, VersionCommand};
//!
//! let mut registry = CommandRegistry::new();
//! registry.register(PingCommand);
//! registry.register(VersionCommand);
//!
//! assert!(registry.get("ping").is_some());
//! assert_eq!(registry.len(), 2);
//! ```

pub mod commands;
pub mod registry;
pub mod traits;

// Re-export the most-used surface at crate root.
pub use commands::{PingCommand, VersionCommand};
pub use registry::CommandRegistry;
pub use traits::{
    Command, CommandArgSchema, CommandArgs, CommandCategory, CommandContext, CommandDescription,
    CommandError, CommandResult,
};
