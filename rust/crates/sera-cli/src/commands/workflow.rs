//! `sera workflow` — list/show/create/delete via the L.3 admin endpoints.
//!
//! `create` and `delete` hit `/admin/workflows[/{id}]` which return HTTP 501
//! today (write paths are gated on L.5 wiring); we surface that as a clear
//! error rather than a hard failure. `list` and `show` work end-to-end.

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

fn not_implemented_message() -> CommandError {
    CommandError::Execution(
        "gateway returned 501 Not Implemented — workflow create/delete is L.5 not yet wired"
            .into(),
    )
}

// ── list ────────────────────────────────────────────────────────────────────

pub struct WorkflowListCommand;

#[async_trait]
impl Command for WorkflowListCommand {
    fn name(&self) -> &str {
        "workflow:list"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "List workflows (GET /admin/workflows)".into(),
            help: "Calls the gateway admin endpoint and prints a table of \
                   active workflows."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(clap::Command::new("list").about("List workflows"))
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let client = build_client(&args)?;
        let resp = client
            .http()
            .get(client.url("/admin/workflows"))
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

        let workflows = body
            .get("workflows")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header(["ID", "AGENT", "STATUS", "TITLE"]);
        for w in &workflows {
            table.add_row([
                w.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                w.get("agent_id").and_then(|v| v.as_str()).unwrap_or(""),
                w.get("status").and_then(|v| v.as_str()).unwrap_or(""),
                w.get("title").and_then(|v| v.as_str()).unwrap_or(""),
            ]);
        }
        println!("{table}");
        Ok(CommandResult::ok(json!({ "workflows": workflows })))
    }
}

// ── show ────────────────────────────────────────────────────────────────────

pub struct WorkflowShowCommand;

#[async_trait]
impl Command for WorkflowShowCommand {
    fn name(&self) -> &str {
        "workflow:show"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Show workflow detail (GET /admin/workflows/{id})".into(),
            help: "Fetches one workflow's record by id and pretty-prints it.".into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("show")
                .about("Show workflow detail")
                .arg(clap::Arg::new("id").required(true).help("Workflow ID")),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let id = args
            .get("id")
            .ok_or_else(|| CommandError::InvalidArgs("workflow id required".into()))?
            .to_owned();
        let client = build_client(&args)?;
        let resp = client
            .http()
            .get(client.url(&format!("/admin/workflows/{id}")))
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;
        match resp.status() {
            StatusCode::NOT_FOUND => Err(CommandError::Execution(format!("workflow not found: {id}"))),
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

// ── create ──────────────────────────────────────────────────────────────────

pub struct WorkflowCreateCommand;

#[async_trait]
impl Command for WorkflowCreateCommand {
    fn name(&self) -> &str {
        "workflow:create"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Create a workflow from a YAML manifest file (deferred to L.5)".into(),
            help: "Reads <file> as YAML and POSTs it to /admin/workflows. The L.3 \
                   gateway returns 501 today — write paths land in L.5."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("create")
                .about("Create a workflow from a manifest file")
                .arg(
                    clap::Arg::new("file")
                        .required(true)
                        .help("Path to a workflow manifest (YAML)"),
                ),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let path = args
            .get("file")
            .ok_or_else(|| CommandError::InvalidArgs("file path required".into()))?
            .to_owned();
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| CommandError::Execution(format!("failed to read {path}: {e}")))?;
        let parsed: serde_yaml::Value = serde_yaml::from_str(&raw)
            .map_err(|e| CommandError::Execution(format!("failed to parse {path}: {e}")))?;
        let body = serde_json::to_value(&parsed)
            .map_err(|e| CommandError::Execution(format!("failed to convert YAML: {e}")))?;

        let client = build_client(&args)?;
        let resp = client
            .http()
            .post(client.url("/admin/workflows"))
            .json(&body)
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;
        if resp.status() == StatusCode::NOT_IMPLEMENTED {
            return Err(not_implemented_message());
        }
        if !resp.status().is_success() {
            return Err(CommandError::Execution(format!(
                "gateway returned HTTP {}",
                resp.status()
            )));
        }
        let response: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| CommandError::Execution(format!("failed to parse response: {e}")))?;
        println!("{}", serde_json::to_string_pretty(&response).unwrap_or_default());
        Ok(CommandResult::ok(response))
    }
}

// ── delete ──────────────────────────────────────────────────────────────────

pub struct WorkflowDeleteCommand;

#[async_trait]
impl Command for WorkflowDeleteCommand {
    fn name(&self) -> &str {
        "workflow:delete"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Delete a workflow (deferred to L.5)".into(),
            help: "DELETEs /admin/workflows/{id}. The L.3 gateway returns 501 today."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("delete")
                .about("Delete a workflow")
                .arg(clap::Arg::new("id").required(true).help("Workflow ID")),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let id = args
            .get("id")
            .ok_or_else(|| CommandError::InvalidArgs("workflow id required".into()))?
            .to_owned();
        let client = build_client(&args)?;
        let resp = client
            .http()
            .delete(client.url(&format!("/admin/workflows/{id}")))
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;
        if resp.status() == StatusCode::NOT_IMPLEMENTED {
            return Err(not_implemented_message());
        }
        if resp.status() == StatusCode::NOT_FOUND {
            return Err(CommandError::Execution(format!("workflow not found: {id}")));
        }
        if !resp.status().is_success() {
            return Err(CommandError::Execution(format!(
                "gateway returned HTTP {}",
                resp.status()
            )));
        }
        println!("Deleted workflow {id}");
        Ok(CommandResult::ok(json!({ "deleted": id })))
    }
}
