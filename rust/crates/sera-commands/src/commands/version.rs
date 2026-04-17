//! `version` — returns the crate version string.

use async_trait::async_trait;
use serde_json::json;

use crate::traits::{
    Command, CommandArgSchema, CommandArgs, CommandCategory, CommandContext, CommandDescription,
    CommandError, CommandResult,
};

/// Returns the `sera-commands` crate version embedded at compile time.
pub struct VersionCommand;

#[async_trait]
impl Command for VersionCommand {
    fn name(&self) -> &str {
        "version"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Print the SERA build version".into(),
            help: "Prints the version string compiled into this SERA build.".into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("version")
                .about("Print the SERA build version"),
        )
    }

    async fn execute(
        &self,
        _args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let version = env!("CARGO_PKG_VERSION");
        Ok(CommandResult::ok(json!({ "version": version })))
    }
}
