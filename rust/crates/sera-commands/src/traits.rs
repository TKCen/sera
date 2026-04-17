//! Core command abstractions for SERA's unified command registry.
//!
//! A `Command` is defined once and can be dispatched from either the CLI
//! (terminal) or the gateway (HTTP/gRPC). The registry holds all registered
//! commands and provides lookup and category-filtered iteration.

use async_trait::async_trait;
use serde_json::Value;

/// Category grouping for commands, used for help display and gateway introspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandCategory {
    Agent,
    Session,
    Knowledge,
    System,
    Diagnostic,
}

impl CommandCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Session => "session",
            Self::Knowledge => "knowledge",
            Self::System => "system",
            Self::Diagnostic => "diagnostic",
        }
    }
}

impl std::fmt::Display for CommandCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Human-readable description of a command.
#[derive(Debug, Clone)]
pub struct CommandDescription {
    /// One-line summary shown in `--help` listings.
    pub summary: String,
    /// Extended help text shown with `command --help`.
    pub help: String,
    /// Category for grouping in help output and gateway schema.
    pub category: CommandCategory,
}

/// Argument schema backed by a `clap::Command`.
///
/// The CLI uses this directly for argument parsing.  The gateway can
/// introspect the `clap::Command` to derive an equivalent JSON/HTTP schema.
pub struct CommandArgSchema(pub clap::Command);

/// Parsed arguments passed to [`Command::execute`].
///
/// Carries the raw `clap::ArgMatches` from CLI parsing or the equivalent
/// key-value map when dispatched from the gateway.
#[derive(Debug, Default)]
pub struct CommandArgs {
    /// Raw string arguments (key → value).  An empty map means no arguments.
    pub values: std::collections::HashMap<String, String>,
}

impl CommandArgs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.values.insert(key.into(), value.into());
    }
}

/// Execution context provided to every command.
///
/// Kept minimal for the scaffold; fields will grow as auth/tracing land.
#[derive(Debug, Default)]
pub struct CommandContext {
    /// Identity of the caller (empty string = anonymous / CLI).
    pub caller_id: String,
    /// Optional request-scoped trace ID for distributed tracing.
    pub trace_id: Option<String>,
}

impl CommandContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_caller(caller_id: impl Into<String>) -> Self {
        Self {
            caller_id: caller_id.into(),
            trace_id: None,
        }
    }
}

/// The result produced by a successful command execution.
#[derive(Debug, Clone)]
pub struct CommandResult {
    /// Exit code (0 = success, non-zero = error, matching Unix convention).
    pub exit_code: i32,
    /// Structured output data; may be `Value::Null` for commands with no output.
    pub data: Value,
}

impl CommandResult {
    pub fn ok(data: Value) -> Self {
        Self { exit_code: 0, data }
    }

    pub fn success() -> Self {
        Self::ok(Value::Null)
    }
}

/// Errors that can occur during command dispatch.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// No command with the given name is registered.
    #[error("command not found: {0}")]
    NotFound(String),
    /// The supplied arguments did not pass validation.
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
    /// The command encountered a runtime error during execution.
    #[error("execution error: {0}")]
    Execution(String),
}

/// Unified command interface shared between CLI and gateway dispatch paths.
///
/// Implement this trait once per command; register the implementation with
/// [`CommandRegistry`](crate::registry::CommandRegistry).  The CLI calls
/// [`execute`](Command::execute) directly; the gateway wraps the call in an
/// HTTP handler.
#[async_trait]
pub trait Command: Send + Sync {
    /// Unique, kebab-case name used for registration and dispatch (e.g. `"ping"`).
    fn name(&self) -> &str;

    /// Human-readable description for help output and gateway introspection.
    fn describe(&self) -> CommandDescription;

    /// Clap-backed argument schema.  The CLI parses args with this; the
    /// gateway introspects it to produce an HTTP/JSON parameter schema.
    fn argument_schema(&self) -> CommandArgSchema;

    /// Execute the command and return a structured result.
    async fn execute(
        &self,
        args: CommandArgs,
        ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError>;
}
