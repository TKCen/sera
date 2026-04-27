//! `sera session` — list/show/kill/browse via the L.3 admin endpoints.

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

fn render_sessions_table(sessions: &[serde_json::Value]) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(["ID", "AGENT", "SESSION KEY", "STATE"]);
    for s in sessions {
        table.add_row([
            s.get("id").and_then(|v| v.as_str()).unwrap_or(""),
            s.get("agent_id").and_then(|v| v.as_str()).unwrap_or(""),
            s.get("session_key").and_then(|v| v.as_str()).unwrap_or(""),
            s.get("state").and_then(|v| v.as_str()).unwrap_or(""),
        ]);
    }
    println!("{table}");
}

async fn fetch_sessions(client: &AdminClient) -> Result<Vec<serde_json::Value>, CommandError> {
    let resp = client
        .http()
        .get(client.url("/admin/sessions"))
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
    Ok(body.as_array().cloned().unwrap_or_default())
}

// ── list ────────────────────────────────────────────────────────────────────

pub struct SessionListCommand;

#[async_trait]
impl Command for SessionListCommand {
    fn name(&self) -> &str {
        "session:list"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "List active sessions (GET /admin/sessions)".into(),
            help: "Renders a table of active sessions.".into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(clap::Command::new("list").about("List active sessions"))
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let client = build_client(&args)?;
        let sessions = fetch_sessions(&client).await?;
        render_sessions_table(&sessions);
        Ok(CommandResult::ok(json!(sessions)))
    }
}

// ── show ────────────────────────────────────────────────────────────────────

pub struct SessionShowCommand;

#[async_trait]
impl Command for SessionShowCommand {
    fn name(&self) -> &str {
        "session:show"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Show one session (GET /admin/sessions/{id})".into(),
            help: "Pretty-prints a single session record.".into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("show")
                .about("Show a session")
                .arg(clap::Arg::new("id").required(true).help("Session id")),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let id = args
            .get("id")
            .ok_or_else(|| CommandError::InvalidArgs("session id required".into()))?
            .to_owned();
        let client = build_client(&args)?;
        let resp = client
            .http()
            .get(client.url(&format!("/admin/sessions/{id}")))
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;
        match resp.status() {
            StatusCode::NOT_FOUND => Err(CommandError::Execution(format!("session not found: {id}"))),
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

// ── kill ────────────────────────────────────────────────────────────────────

pub struct SessionKillCommand;

#[async_trait]
impl Command for SessionKillCommand {
    fn name(&self) -> &str {
        "session:kill"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Cancel a session (DELETE /admin/sessions/{id})".into(),
            help: "Fires the session's cancellation token. Idempotent.".into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("kill")
                .about("Kill a session")
                .arg(clap::Arg::new("id").required(true).help("Session id")),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let id = args
            .get("id")
            .ok_or_else(|| CommandError::InvalidArgs("session id required".into()))?
            .to_owned();
        let client = build_client(&args)?;
        let resp = client
            .http()
            .delete(client.url(&format!("/admin/sessions/{id}")))
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(CommandError::Execution(format!(
                "gateway returned HTTP {}",
                resp.status()
            )));
        }
        println!("Cancelled session {id}");
        Ok(CommandResult::ok(json!({ "cancelled": id })))
    }
}

// ── browse ──────────────────────────────────────────────────────────────────

const BROWSE_PAGE_SIZE: usize = 20;

pub struct SessionBrowseCommand;

#[async_trait]
impl Command for SessionBrowseCommand {
    fn name(&self) -> &str {
        "session:browse"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Paginated browser for active sessions".into(),
            help: "On a TTY, prints sessions a page at a time and prompts for \
                   ENTER (next page) or `q` (quit). On a non-TTY (pipe, CI, test), \
                   behaves like `session list`."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(clap::Command::new("browse").about("Paginated session browser"))
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let client = build_client(&args)?;
        let sessions = fetch_sessions(&client).await?;

        if !is_tty() {
            render_sessions_table(&sessions);
            return Ok(CommandResult::ok(json!(sessions)));
        }

        if sessions.is_empty() {
            println!("(no active sessions)");
            return Ok(CommandResult::ok(json!(sessions)));
        }

        for (idx, page) in sessions.chunks(BROWSE_PAGE_SIZE).enumerate() {
            println!(
                "── page {}/{} ──────────────────────────────",
                idx + 1,
                sessions.len().div_ceil(BROWSE_PAGE_SIZE)
            );
            render_sessions_table(page);
            if (idx + 1) * BROWSE_PAGE_SIZE >= sessions.len() {
                break;
            }
            println!("[ENTER] next page, [q ENTER] quit");
            let mut buf = String::new();
            std::io::stdin()
                .read_line(&mut buf)
                .map_err(|e| CommandError::Execution(format!("stdin: {e}")))?;
            if buf.trim().eq_ignore_ascii_case("q") {
                break;
            }
        }

        Ok(CommandResult::ok(json!(sessions)))
    }
}

#[cfg(unix)]
fn is_tty() -> bool {
    use std::os::fd::AsRawFd;
    // Safe: AsRawFd returns a valid fd; isatty has no UB on invalid fd, just returns 0.
    unsafe { libc_isatty(std::io::stdin().as_raw_fd()) == 1 }
}

#[cfg(not(unix))]
fn is_tty() -> bool {
    // Conservative on non-Unix: skip pagination so non-TTY environments
    // (Windows CI, piped tests) take the list-only path.
    false
}

#[cfg(unix)]
unsafe extern "C" {
    fn isatty(fd: i32) -> i32;
}

#[cfg(unix)]
unsafe fn libc_isatty(fd: i32) -> i32 {
    unsafe { isatty(fd) }
}
