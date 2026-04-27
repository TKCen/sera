//! `sera killswitch` — arm/disarm/status via the L.3 admin endpoints.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use sera_commands::{
    Command, CommandArgSchema, CommandArgs, CommandCategory, CommandContext, CommandDescription,
    CommandError, CommandResult,
};
use sera_config::ConfigRoot;

use crate::admin_client::{AdminClient, resolve_target_default};

fn map_err<E: std::fmt::Display>(e: E) -> CommandError {
    CommandError::Execution(e.to_string())
}

fn build_client(args: &CommandArgs) -> Result<AdminClient, CommandError> {
    let root = args
        .get("config-root")
        .map(|s| ConfigRoot::new(s.to_string()))
        .unwrap_or_else(ConfigRoot::from_env);
    let target = resolve_target_default(&root, args.get("endpoint"), args.get("admin-token"))
        .map_err(map_err)?;
    AdminClient::new(target).map_err(map_err)
}

async fn post_killswitch(
    args: &CommandArgs,
    path: &'static str,
    success_msg: &'static str,
) -> Result<CommandResult, CommandError> {
    let client = build_client(args)?;
    let resp = client
        .http()
        .post(client.url(path))
        .send()
        .await
        .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(CommandError::Execution(format!(
            "gateway returned HTTP {}",
            resp.status()
        )));
    }
    println!("{success_msg}");
    Ok(CommandResult::ok(json!({ "ok": true })))
}

// ── arm ─────────────────────────────────────────────────────────────────────

pub struct KillswitchArmCommand;

#[async_trait]
impl Command for KillswitchArmCommand {
    fn name(&self) -> &str {
        "killswitch:arm"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Arm the gateway killswitch (POST /admin/killswitch/arm)".into(),
            help: "Halts new tool dispatch; existing turns finish.".into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(clap::Command::new("arm").about("Arm the killswitch"))
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        post_killswitch(&args, "/admin/killswitch/arm", "Killswitch armed").await
    }
}

// ── disarm ──────────────────────────────────────────────────────────────────

pub struct KillswitchDisarmCommand;

#[async_trait]
impl Command for KillswitchDisarmCommand {
    fn name(&self) -> &str {
        "killswitch:disarm"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Disarm the gateway killswitch (POST /admin/killswitch/disarm)".into(),
            help: "Restores normal dispatch.".into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(clap::Command::new("disarm").about("Disarm the killswitch"))
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        post_killswitch(&args, "/admin/killswitch/disarm", "Killswitch disarmed").await
    }
}

// ── status ──────────────────────────────────────────────────────────────────

pub struct KillswitchStatusCommand;

#[async_trait]
impl Command for KillswitchStatusCommand {
    fn name(&self) -> &str {
        "killswitch:status"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Print killswitch state (GET /admin/killswitch/status)".into(),
            help: "Reports `armed: true|false`.".into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(clap::Command::new("status").about("Killswitch status"))
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let client = build_client(&args)?;
        let resp = client
            .http()
            .get(client.url("/admin/killswitch/status"))
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(CommandError::Execution(format!(
                "gateway returned HTTP {}",
                resp.status()
            )));
        }
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| CommandError::Execution(format!("failed to parse response: {e}")))?;
        let armed = body.get("armed").and_then(|v| v.as_bool()).unwrap_or(false);
        println!("Killswitch: {}", if armed { "ARMED" } else { "disarmed" });
        Ok(CommandResult::ok(body))
    }
}
