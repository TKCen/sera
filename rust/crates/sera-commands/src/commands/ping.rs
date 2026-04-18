//! `ping` — trivial liveness command that returns `"pong"`.

use async_trait::async_trait;
use serde_json::json;

use crate::traits::{
    Command, CommandArgSchema, CommandArgs, CommandCategory, CommandContext, CommandDescription,
    CommandError, CommandResult,
};

/// Returns `"pong"` immediately.  Useful as a liveness probe from CLI and
/// gateway health checks.
pub struct PingCommand;

#[async_trait]
impl Command for PingCommand {
    fn name(&self) -> &str {
        "ping"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Check that the SERA command bus is reachable".into(),
            help: "Sends a ping and receives a pong.  No arguments required.".into(),
            category: CommandCategory::Diagnostic,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("ping")
                .about("Check that the SERA command bus is reachable"),
        )
    }

    async fn execute(
        &self,
        _args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        Ok(CommandResult::ok(json!({ "message": "pong" })))
    }
}
