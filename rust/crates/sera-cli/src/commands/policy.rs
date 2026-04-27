//! `sera policy` — list/show/reload/validate via the L.3 admin endpoints.

use anyhow::Result;
use async_trait::async_trait;
use comfy_table::{Table, presets::UTF8_FULL};
use reqwest::StatusCode;
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

// ── list ────────────────────────────────────────────────────────────────────

pub struct PolicyListCommand;

#[async_trait]
impl Command for PolicyListCommand {
    fn name(&self) -> &str {
        "policy:list"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "List capability policies (GET /admin/policies)".into(),
            help: "Lists every YAML policy under the gateway's policies directory."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(clap::Command::new("list").about("List capability policies"))
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let client = build_client(&args)?;
        let resp = client
            .http()
            .get(client.url("/admin/policies"))
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

        let dir = body
            .get("dir")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let names = body
            .get("policies")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if !dir.is_empty() {
            println!("Policies directory: {dir}");
        }
        if names.is_empty() {
            println!("(no policies configured)");
        } else {
            for n in &names {
                if let Some(s) = n.as_str() {
                    println!("- {s}");
                }
            }
        }
        Ok(CommandResult::ok(body))
    }
}

// ── show ────────────────────────────────────────────────────────────────────

pub struct PolicyShowCommand;

#[async_trait]
impl Command for PolicyShowCommand {
    fn name(&self) -> &str {
        "policy:show"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Show a policy manifest (GET /admin/policies/{name})".into(),
            help: "Fetches a single policy and prints the manifest as JSON.".into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("show")
                .about("Show a policy manifest")
                .arg(clap::Arg::new("name").required(true).help("Policy name (file stem)")),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let name = args
            .get("name")
            .ok_or_else(|| CommandError::InvalidArgs("policy name required".into()))?
            .to_owned();
        let client = build_client(&args)?;
        let resp = client
            .http()
            .get(client.url(&format!("/admin/policies/{name}")))
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;
        match resp.status() {
            StatusCode::NOT_FOUND => Err(CommandError::Execution(format!("policy not found: {name}"))),
            s if !s.is_success() => Err(CommandError::Execution(format!("gateway returned HTTP {s}"))),
            _ => {
                let body: serde_json::Value = resp.json().await.map_err(|e| {
                    CommandError::Execution(format!("failed to parse response: {e}"))
                })?;
                println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                Ok(CommandResult::ok(body))
            }
        }
    }
}

// ── reload ──────────────────────────────────────────────────────────────────

pub struct PolicyReloadCommand;

#[async_trait]
impl Command for PolicyReloadCommand {
    fn name(&self) -> &str {
        "policy:reload"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Reload all policies from disk (POST /admin/policies/reload)".into(),
            help: "Re-reads the policies directory and atomically swaps the live registry."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(clap::Command::new("reload").about("Reload policies"))
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let client = build_client(&args)?;
        let resp = client
            .http()
            .post(client.url("/admin/policies/reload"))
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;
        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .await
            .unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            let detail = body
                .get("detail")
                .and_then(|v| v.as_str())
                .unwrap_or("(no detail)");
            return Err(CommandError::Execution(format!(
                "policy reload failed: HTTP {status}: {detail}"
            )));
        }
        let loaded = body.get("loaded").and_then(|v| v.as_u64()).unwrap_or(0);
        println!("Reloaded {loaded} policies");
        Ok(CommandResult::ok(body))
    }
}

// ── validate ────────────────────────────────────────────────────────────────

pub struct PolicyValidateCommand;

#[async_trait]
impl Command for PolicyValidateCommand {
    fn name(&self) -> &str {
        "policy:validate"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Validate every policy YAML on disk (POST /admin/policies/validate)".into(),
            help: "Parses each policy file and reports per-file ok/error status."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(clap::Command::new("validate").about("Validate policies"))
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let client = build_client(&args)?;
        let resp = client
            .http()
            .post(client.url("/admin/policies/validate"))
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

        let results = body
            .get("results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let all_ok = body
            .get("ok")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header(["FILE", "STATUS", "ERROR"]);
        for r in &results {
            let file = r.get("file").and_then(|v| v.as_str()).unwrap_or("");
            let ok = r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            let err = r.get("error").and_then(|v| v.as_str()).unwrap_or("");
            table.add_row([file, if ok { "ok" } else { "error" }, err]);
        }
        println!("{table}");

        let mut result = CommandResult::ok(body);
        if !all_ok {
            result.exit_code = 1;
        }
        Ok(result)
    }
}
