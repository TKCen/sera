//! Command registry — central store for all registered [`Command`] implementations.

use std::collections::HashMap;

use crate::traits::{Command, CommandCategory};

/// Central registry that maps command names to their implementations.
///
/// Register commands once at startup; look them up by name or enumerate by
/// category for help generation and gateway schema introspection.
pub struct CommandRegistry {
    commands: HashMap<String, Box<dyn Command>>,
}

impl CommandRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    /// Register a command.  If a command with the same name already exists it
    /// is silently replaced (last-write-wins; callers should avoid duplicates).
    pub fn register<C: Command + 'static>(&mut self, cmd: C) {
        self.commands.insert(cmd.name().to_owned(), Box::new(cmd));
    }

    /// Look up a command by its exact name.
    pub fn get(&self, name: &str) -> Option<&dyn Command> {
        self.commands.get(name).map(|b| b.as_ref())
    }

    /// Return all registered commands in unspecified order.
    pub fn list(&self) -> Vec<&dyn Command> {
        self.commands.values().map(|b| b.as_ref()).collect()
    }

    /// Return all commands whose [`describe`](Command::describe) category
    /// matches `cat`.
    pub fn list_by_category(&self, cat: CommandCategory) -> Vec<&dyn Command> {
        self.commands
            .values()
            .filter(|b| b.describe().category == cat)
            .map(|b| b.as_ref())
            .collect()
    }

    /// Total number of registered commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns `true` if no commands are registered.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}
