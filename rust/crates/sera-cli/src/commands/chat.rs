//! `sera chat` — interactive REPL over the streaming `/api/chat` endpoint.
//!
//! Design notes:
//! - The REPL lives on top of the autonomous gateway's existing HTTP-SSE
//!   streaming surface (`POST /api/chat` with `"stream": true`).  Centrifugo
//!   is not required — per bead sera-oa6n, the local profile must work over
//!   plain HTTP.
//! - Line editing uses `tokio::io::BufReader::new(stdin()).lines()` rather
//!   than pulling in `rustyline`.  Rustyline on Linux needs ncurses/readline
//!   linkage that bloats the binary and occasionally breaks in minimal
//!   containers; a plain buffered reader keeps the REPL portable.  History
//!   and multi-line editing are explicit non-goals for v1 — we can layer
//!   rustyline in a follow-up without changing the inline command surface.
//! - Inline commands (`/help`, `/quit`, `/dump`, `/approve <id>`) are
//!   dispatched locally; anything else is submitted as a new turn.
//! - Ctrl+D closes stdin cleanly; Ctrl+C is handled via `tokio::signal`.
//!
//! No emoji are emitted — the project convention (`CLAUDE.md`) is text-only
//! decorations unless the user asks for them explicitly.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use sera_commands::{
    Command, CommandArgSchema, CommandArgs, CommandCategory, CommandContext, CommandDescription,
    CommandError, CommandResult,
};

use crate::http::build_client_with_token;
use crate::sse::{SseClient, StreamEvent};
use crate::token_store::{best_available_store, TokenStore};

// ---------------------------------------------------------------------------
// Inline command parser
// ---------------------------------------------------------------------------

/// A command typed at the `> ` prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineCommand {
    /// User typed plain text; submit as a turn.
    Prompt(String),
    /// `/help` — print the inline command list.
    Help,
    /// `/quit` (or Ctrl+D / Ctrl+C) — exit.
    Quit,
    /// `/dump` — print the current MemoryBlock (best-effort).
    Dump,
    /// `/approve <id>` — approve a pending HITL request.
    Approve(String),
    /// `/<unknown>` — unrecognised command, echo help.
    Unknown(String),
}

/// Parse a line from stdin into an [`InlineCommand`].  Empty / whitespace
/// lines are also mapped to `Prompt("")` so the caller can filter them.
pub fn parse_inline_command(raw: &str) -> InlineCommand {
    let trimmed = raw.trim();
    if !trimmed.starts_with('/') {
        return InlineCommand::Prompt(trimmed.to_owned());
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let head = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    match head {
        "/help" | "/?" => InlineCommand::Help,
        "/quit" | "/exit" | "/q" => InlineCommand::Quit,
        "/dump" => InlineCommand::Dump,
        "/approve" => {
            if rest.is_empty() {
                InlineCommand::Unknown("/approve requires an id".into())
            } else {
                InlineCommand::Approve(rest.to_owned())
            }
        }
        other => InlineCommand::Unknown(other.to_owned()),
    }
}

// ---------------------------------------------------------------------------
// ChatCommand
// ---------------------------------------------------------------------------

fn no_token_error() -> CommandError {
    CommandError::Execution("not authenticated — run `sera auth login` first".into())
}

/// `sera chat` — interactive REPL.
pub struct ChatCommand {
    store: Arc<dyn TokenStore>,
}

impl ChatCommand {
    pub fn new() -> Self {
        Self {
            store: Arc::from(best_available_store()),
        }
    }

    pub fn with_store(store: Arc<dyn TokenStore>) -> Self {
        Self { store }
    }
}

impl Default for ChatCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for ChatCommand {
    fn name(&self) -> &str {
        "chat"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Interactive REPL — stream turns to/from an agent".into(),
            help: "Opens an interactive chat session against an agent, streaming \
                   tokens over plain HTTP SSE. Inline commands: /help, /quit, /dump, \
                   /approve <id>. Ctrl+C and Ctrl+D exit cleanly. Centrifugo is not \
                   required; the session works over plain HTTP."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("chat")
                .about("Interactive streaming REPL")
                .arg(
                    clap::Arg::new("agent")
                        .long("agent")
                        .help("Agent ID or name to open a session against")
                        .value_name("AGENT"),
                )
                .arg(
                    clap::Arg::new("session")
                        .help("Optional session ID to resume (otherwise a new one is created)")
                        .value_name("SESSION_ID"),
                )
                .arg(
                    clap::Arg::new("endpoint")
                        .long("endpoint")
                        .short('e')
                        .help("Gateway base URL (overrides config)")
                        .value_name("URL"),
                )
                .arg(
                    clap::Arg::new("api-url")
                        .long("api-url")
                        .help("Alias for --endpoint")
                        .value_name("URL"),
                ),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let endpoint = args
            .get("api-url")
            .or_else(|| args.get("endpoint"))
            .unwrap_or("http://localhost:8080")
            .trim_end_matches('/')
            .to_owned();

        let agent = args.get("agent").map(str::to_owned);
        let session = args.get("session").map(str::to_owned);

        if agent.is_none() && session.is_none() {
            return Err(CommandError::InvalidArgs(
                "either --agent <id> or a session id positional argument is required".into(),
            ));
        }

        let token = self
            .store
            .load()
            .map_err(|e| CommandError::Execution(format!("failed to load token: {e}")))?
            .ok_or_else(no_token_error)?;

        let http = build_client_with_token(&token)
            .map_err(|e| CommandError::Execution(e.to_string()))?;

        let exit_code = run_repl(http, endpoint, agent, session)
            .await
            .map_err(|e| CommandError::Execution(e.to_string()))?;

        let mut result = CommandResult::success();
        result.exit_code = exit_code;
        Ok(result)
    }
}

/// Build and run the REPL loop.  Returns the suggested process exit code.
async fn run_repl(
    http: reqwest::Client,
    endpoint: String,
    agent: Option<String>,
    session: Option<String>,
) -> Result<i32> {
    // Resolve the agent id: explicit `--agent` wins; otherwise we fall back
    // to "sera" (the autonomous gateway's default agent name).
    let agent_id = agent.unwrap_or_else(|| "sera".to_owned());

    print_help();
    print_banner(&endpoint, &agent_id, session.as_deref());

    let sse = SseClient::new(http.clone(), endpoint.clone());
    let mut stdin = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    // Track session id once the gateway tells us one.  Used by /dump and
    // /approve paths that need the current session context.
    let mut current_session: Option<String> = session;

    loop {
        // Prompt.
        stdout.write_all(b"> ").await.ok();
        stdout.flush().await.ok();

        // ── Read one line ──────────────────────────────────────────────
        let line = tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\n[exit]");
                return Ok(0);
            }
            res = stdin.next_line() => {
                match res {
                    Ok(Some(s)) => s,
                    // EOF (Ctrl+D) — clean exit.
                    Ok(None) => {
                        eprintln!("\n[exit]");
                        return Ok(0);
                    }
                    Err(e) => return Err(anyhow!("stdin read error: {e}")),
                }
            }
        };

        // ── Dispatch ──────────────────────────────────────────────────
        let cmd = parse_inline_command(&line);
        match cmd {
            InlineCommand::Prompt(p) if p.is_empty() => continue,
            InlineCommand::Quit => {
                eprintln!("[exit]");
                return Ok(0);
            }
            InlineCommand::Help => {
                print_help();
                continue;
            }
            InlineCommand::Dump => {
                if let Err(e) = dump_memory(&http, &endpoint, current_session.as_deref()).await {
                    eprintln!("error: {e}");
                }
                continue;
            }
            InlineCommand::Approve(id) => {
                if let Err(e) = approve_hitl(&http, &endpoint, &id).await {
                    eprintln!("error: {e}");
                } else {
                    println!("approved: {id}");
                }
                continue;
            }
            InlineCommand::Unknown(msg) => {
                eprintln!("unknown command: {msg} (try /help)");
                continue;
            }
            InlineCommand::Prompt(text) => {
                // Submit the turn and stream the reply.
                let turn = turn_payload(&agent_id, current_session.as_deref(), &text);
                let mut stream = match sse.post_stream("/api/chat", turn).await {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: {e}");
                        continue;
                    }
                };

                // Drain events until `Done` or the stream ends.  Ctrl+C
                // aborts the current turn but keeps the REPL alive.
                let ctrl_c = tokio::signal::ctrl_c();
                tokio::pin!(ctrl_c);
                loop {
                    tokio::select! {
                        biased;
                        _ = &mut ctrl_c => {
                            eprintln!("\n[interrupted]");
                            break;
                        }
                        ev = stream.next() => {
                            match ev {
                                Some(Ok(StreamEvent::Token { delta, session_id })) => {
                                    if current_session.is_none() && !session_id.is_empty() {
                                        current_session = Some(session_id);
                                    }
                                    stdout.write_all(delta.as_bytes()).await.ok();
                                    stdout.flush().await.ok();
                                }
                                Some(Ok(StreamEvent::ToolCall { name, args })) => {
                                    println!("\n* tool: {name}({args})");
                                }
                                Some(Ok(StreamEvent::ToolResult { name, result })) => {
                                    println!("\n* tool result [{name}]: {result}");
                                }
                                Some(Ok(StreamEvent::HitlPending { id })) => {
                                    println!("\n[HITL pending: {id}]  use /approve {id} to approve");
                                }
                                Some(Ok(StreamEvent::MemoryPressure { message })) => {
                                    println!("\n[memory: {message}]");
                                }
                                Some(Ok(StreamEvent::Error { message })) => {
                                    eprintln!("\nerror: {message}");
                                    break;
                                }
                                Some(Ok(StreamEvent::Done { .. })) => {
                                    println!(); // flush trailing newline
                                    break;
                                }
                                Some(Ok(StreamEvent::Other { event, .. })) => {
                                    tracing::debug!(%event, "unhandled sse event");
                                }
                                Some(Err(e)) => {
                                    eprintln!("\nstream error: {e}");
                                    break;
                                }
                                None => {
                                    println!();
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub(crate) fn turn_payload(
    agent: &str,
    session: Option<&str>,
    message: &str,
) -> serde_json::Value {
    let mut body = json!({
        "agent": agent,
        "agentInstanceId": agent,
        "message": message,
        "stream": true,
    });
    if let Some(s) = session {
        body["session_id"] = json!(s);
        body["sessionId"] = json!(s);
    }
    body
}

async fn dump_memory(
    http: &reqwest::Client,
    endpoint: &str,
    session_id: Option<&str>,
) -> Result<()> {
    let Some(sid) = session_id else {
        println!("[no active session yet — send a prompt first]");
        return Ok(());
    };
    // The full gateway exposes /api/sessions/:id/memory but the autonomous
    // gateway only has /api/sessions/:id/transcript.  Try memory first, then
    // fall back to transcript.  Both 404s are reported as "not available".
    let memory_url = format!("{endpoint}/api/sessions/{sid}/memory");
    let resp = http
        .get(&memory_url)
        .send()
        .await
        .with_context(|| format!("GET {memory_url}"))?;
    if resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.context("parse memory response")?;
        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
        return Ok(());
    }
    // Fall through to transcript.
    let transcript_url = format!("{endpoint}/api/sessions/{sid}/transcript");
    let resp = http
        .get(&transcript_url)
        .send()
        .await
        .with_context(|| format!("GET {transcript_url}"))?;
    if !resp.status().is_success() {
        println!("[memory endpoint not available on this gateway]");
        return Ok(());
    }
    let body: serde_json::Value = resp.json().await.context("parse transcript response")?;
    println!("--- transcript for {sid} ---");
    println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
    Ok(())
}

async fn approve_hitl(http: &reqwest::Client, endpoint: &str, id: &str) -> Result<()> {
    let url = format!("{endpoint}/api/permission-requests/{id}/approve");
    let resp = http
        .post(&url)
        .json(&json!({}))
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!("HITL approval not supported on this gateway (404)");
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("gateway returned HTTP {status}: {body}");
    }
    Ok(())
}

fn print_help() {
    println!("sera chat — inline commands:");
    println!("  /help             show this help");
    println!("  /quit             exit (Ctrl+D or Ctrl+C also work)");
    println!("  /dump             print current session memory/transcript");
    println!("  /approve <id>     approve a pending HITL request");
    println!("Anything else is sent to the agent as a new turn.");
}

fn print_banner(endpoint: &str, agent: &str, session: Option<&str>) {
    match session {
        Some(s) => println!("connected to {endpoint} — agent: {agent} — session: {s}"),
        None => println!("connected to {endpoint} — agent: {agent} — new session"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_inline_command_prompts_plain_text() {
        assert_eq!(
            parse_inline_command("hello world"),
            InlineCommand::Prompt("hello world".to_owned())
        );
        assert_eq!(
            parse_inline_command("   spaced   "),
            InlineCommand::Prompt("spaced".to_owned())
        );
        assert_eq!(
            parse_inline_command(""),
            InlineCommand::Prompt("".to_owned())
        );
    }

    #[test]
    fn parse_inline_command_help_variants() {
        assert_eq!(parse_inline_command("/help"), InlineCommand::Help);
        assert_eq!(parse_inline_command("/?"), InlineCommand::Help);
    }

    #[test]
    fn parse_inline_command_quit_variants() {
        assert_eq!(parse_inline_command("/quit"), InlineCommand::Quit);
        assert_eq!(parse_inline_command("/exit"), InlineCommand::Quit);
        assert_eq!(parse_inline_command("/q"), InlineCommand::Quit);
    }

    #[test]
    fn parse_inline_command_dump() {
        assert_eq!(parse_inline_command("/dump"), InlineCommand::Dump);
    }

    #[test]
    fn parse_inline_command_approve_with_id() {
        assert_eq!(
            parse_inline_command("/approve abc-123"),
            InlineCommand::Approve("abc-123".to_owned())
        );
        assert_eq!(
            parse_inline_command("/approve    padded-id  "),
            InlineCommand::Approve("padded-id".to_owned())
        );
    }

    #[test]
    fn parse_inline_command_approve_without_id_is_unknown() {
        match parse_inline_command("/approve") {
            InlineCommand::Unknown(msg) => assert!(msg.contains("id")),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn parse_inline_command_unknown_slash() {
        match parse_inline_command("/foo bar") {
            InlineCommand::Unknown(head) => assert_eq!(head, "/foo"),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn turn_payload_includes_stream_true() {
        let payload = turn_payload("sera", None, "hi");
        assert_eq!(payload["agent"], "sera");
        assert_eq!(payload["message"], "hi");
        assert_eq!(payload["stream"], true);
        assert!(payload.get("session_id").is_none());
    }

    #[test]
    fn turn_payload_threads_session_id() {
        let payload = turn_payload("sera", Some("sess-1"), "hi");
        assert_eq!(payload["session_id"], "sess-1");
        assert_eq!(payload["sessionId"], "sess-1");
    }
}
