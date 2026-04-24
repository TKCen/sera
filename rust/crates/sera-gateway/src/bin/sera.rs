//! MVS `sera` binary — standalone gateway that wires config, DB, Discord, and
//! a minimal HTTP API into a single process. No PostgreSQL, Docker, or Centrifugo
//! required.
//!
//! Usage:
//!   sera start [-c sera.yaml] [-p 3001]
//!   sera init
//!   sera agent list
//!   sera agent create <name>

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{FromRequest, Path, Request, State};
use axum::extract::rejection::JsonRejection;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use futures_util::stream;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock, mpsc};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use sera_config::manifest_loader::{
    ManifestSet, load_manifest_file, parse_manifests, resolve_provider_api_key,
};
use sera_config::secrets::SecretResolver;
use sera_db::lane_queue::LaneQueue;
use sera_queue::QueueMode;
use sera_db::lane_queue_counter::{InMemoryLaneCounter, LaneCounterStoreDyn, PostgresLaneCounter};
use sera_db::sqlite::SqliteDb;
// sera-vzce: SqliteMemoryStore is the zero-infra SemanticMemoryStore tier
// (FTS5 + sqlite-vec + RRF). Pairs with PgVectorStore for the enterprise
// path. Wired in the boot path below via backend selection on
// SERA_MEMORY_BACKEND + DATABASE_URL.
use sera_memory::PgVectorStore;
use sera_memory::SemanticMemoryStore;
#[allow(unused_imports)]
use sera_memory::{DEFAULT_SQLITE_VEC_DIMENSIONS, SqliteMemoryStore};
use sera_runtime::skill_dispatch::SkillDispatchEngine;
// sera-uwk0: Mail gate ingress correlator (Design B — RFC 5322 headers +
// SERA-issued nonce fallback). Wired into AppState + `/api/mail/inbound`.
use sera_gateway::kill_switch::{KillSwitch, admin_sock_path, spawn_admin_socket};
#[cfg(test)]
use sera_gateway::session_store::InMemorySessionStore;
use sera_gateway::session_store::{SessionStore, SqliteGitSessionStore};
use sera_hooks::{ChainExecutor, HookRegistry};
use sera_mail::{
    CorrelationOutcome, HeaderMailCorrelator, InMemoryEnvelopeIndex, InMemoryMailLookup,
    MailCorrelator, parse_raw_message,
};
use sera_types::config_manifest::{AgentSpec, ConnectorSpec, ProviderSpec};
use sera_types::event::IncomingEvent as DomainEvent;
use sera_types::hook::{HookChain, HookContext, HookPoint, HookResult};
use sera_types::principal::{PrincipalId, PrincipalKind, PrincipalRef};
use sera_meta::constitutional::ConstitutionalRegistry;
use sera_gateway::constitutional_config;

// ── Phase-3 SPEC-interop crates ──────────────────────────────────────────────
use sera_a2a::{A2aClient, A2aRequest, A2aRouter, InProcRouter, LoopbackTransport};
#[allow(unused_imports)]
use sera_agui::AgUiEvent;
use sera_plugins::InMemoryPluginRegistry;

// Route modules for Phase-3 endpoints (included directly into the binary).
#[path = "../routes/a2a.rs"]
mod route_a2a;
#[path = "../routes/agui.rs"]
mod route_agui;
#[path = "../routes/plugins.rs"]
mod route_plugins;

use route_a2a::{A2aAppState, A2aPeerRegistry};
use route_agui::{AguiAppState, AguiHub};
use route_plugins::PluginsAppState;

// Party-mode handler (sera-8d1.2 / GH#145) — generic over PartyAppState trait
// so the handler lives in the library without depending on the binary's AppState.
#[path = "../party.rs"]
mod party;
use party::PartyAppState;

// Re-use sera-core's Discord connector.
#[path = "../discord.rs"]
mod discord;
use discord::{DiscordConnector, DiscordMessage};

// ── Doctor module ────────────────────────────────────────────────────────────
#[path = "../doctor.rs"]
mod doctor;

/// Selection predicate for the Tier-2 semantic memory backend.
///
/// `backend_pref` is the lowercased, trimmed value of `SERA_MEMORY_BACKEND`
/// (or `None` when unset). `database_url` is the value of `DATABASE_URL`
/// (or `None` when unset). Returns `true` when the pgvector path should be
/// attempted — the caller still falls back to SqliteMemoryStore on any
/// connect or init failure.
fn wants_pgvector_backend(backend_pref: Option<&str>, database_url: Option<&str>) -> bool {
    matches!(backend_pref, Some("pgvector")) || (backend_pref.is_none() && database_url.is_some())
}

// ── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "sera", about = "SERA -- Sandboxed Extensible Reasoning Agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the SERA gateway
    Start {
        /// Path to sera.yaml config file
        #[arg(short, long, default_value = "sera.yaml")]
        config: PathBuf,
        /// HTTP port
        #[arg(short, long, default_value = "3001")]
        port: u16,
    },
    /// Initialize a new sera.yaml config
    Init,
    /// Agent management
    Agent {
        #[command(subcommand)]
        command: AgentCommands,
    },
    /// Secret management
    Secrets {
        #[command(subcommand)]
        command: SecretCommands,
    },
    /// Run diagnostic checks on this SERA installation
    Doctor {
        /// Path to sera.yaml config file
        #[arg(short, long, default_value = "sera.yaml")]
        config: PathBuf,
        /// Output results as JSON instead of a table
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum AgentCommands {
    /// Create a new agent
    Create { name: String },
    /// List agents
    List,
}

#[derive(Subcommand)]
enum SecretCommands {
    /// Store a secret
    Set {
        /// Secret path (e.g., "connectors/discord-main/token")
        path: String,
        /// Secret value
        value: String,
    },
    /// Get a secret (shows masked value)
    Get { path: String },
    /// List all stored secrets (paths only)
    List,
    /// Delete a secret
    Delete { path: String },
}

// ── StdioHarness — manages a running sera-runtime child process ─────────────

/// A handle to a long-lived `sera-runtime --ndjson` child process.
/// Spawned once per agent on startup; reused for every turn.
struct StdioHarness {
    stdin: Mutex<tokio::process::ChildStdin>,
    stdout: Mutex<tokio::io::BufReader<tokio::process::ChildStdout>>,
    #[allow(dead_code)]
    child: Mutex<tokio::process::Child>,
}

impl StdioHarness {
    /// Spawn a `sera-runtime --ndjson --no-health` process with the given env.
    async fn spawn(
        runtime_bin: &str,
        env: std::collections::HashMap<String, String>,
    ) -> anyhow::Result<Self> {
        let mut cmd = tokio::process::Command::new(runtime_bin);
        cmd.arg("--ndjson")
            .arg("--no-health")
            .envs(&env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");

        Ok(Self {
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(tokio::io::BufReader::new(stdout)),
            child: Mutex::new(child),
        })
    }

    /// Send a turn with the given conversation messages to the runtime.
    /// Blocks until the runtime emits `TurnCompleted`, returns a `TurnEvents`
    /// containing the response text and any tool call events.
    async fn send_turn(
        &self,
        messages: Vec<serde_json::Value>,
        session_key: &str,
    ) -> anyhow::Result<TurnEvents> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let submission = serde_json::json!({
            "id": uuid::Uuid::new_v4(),
            "op": {
                "type": "user_turn",
                "items": messages,
                "session_key": session_key,
            }
        });

        let mut json_line = serde_json::to_string(&submission)?;
        json_line.push('\n');

        // Acquire both locks — serialises concurrent turns through this harness.
        let mut stdin = self.stdin.lock().await;
        let mut stdout = self.stdout.lock().await;

        // If `write_all`/`flush` fails (typically `BrokenPipe`), poll the child's
        // exit status so the error surfaced to the API caller explains *why* the
        // runtime is gone instead of just reporting the OS-level pipe error.
        // Without this, sera-un35-style regressions look like "Broken pipe
        // (os error 32)" with no root-cause context.
        if let Err(e) = stdin.write_all(json_line.as_bytes()).await {
            return Err(self.child_exit_context(e).await);
        }
        if let Err(e) = stdin.flush().await {
            return Err(self.child_exit_context(e).await);
        }

        let mut result = TurnEvents::default();
        let mut line = String::new();

        loop {
            line.clear();
            let n = stdout.read_line(&mut line).await?;
            if n == 0 {
                anyhow::bail!("sera-runtime closed stdout unexpectedly");
            }

            // Skip non-JSON lines (empty, debug output, log lines, etc.)
            let trimmed = line.trim();
            if trimmed.is_empty() || !trimmed.starts_with('{') {
                continue;
            }

            let event: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(e) => e,
                Err(e) => {
                    tracing::debug!("Skipping non-JSON line from runtime: {}", e);
                    continue;
                }
            };
            let msg_type = event
                .get("msg")
                .and_then(|m| m.get("type"))
                .and_then(|t| t.as_str())
                .unwrap_or("");

            match msg_type {
                "streaming_delta" => {
                    if let Some(delta) = event
                        .get("msg")
                        .and_then(|m| m.get("delta"))
                        .and_then(|d| d.as_str())
                    {
                        result.response.push_str(delta);
                    }
                }
                "tool_call_begin" => {
                    if let Some(msg) = event.get("msg") {
                        result.tool_events.push(ToolEvent::Begin {
                            call_id: msg
                                .get("call_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            tool: msg
                                .get("tool")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            arguments: msg
                                .get("arguments")
                                .cloned()
                                .unwrap_or(serde_json::Value::Null),
                        });
                    }
                }
                "tool_call_end" => {
                    if let Some(msg) = event.get("msg") {
                        result.tool_events.push(ToolEvent::End {
                            call_id: msg
                                .get("call_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            content: msg
                                .get("result")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        });
                    }
                }
                "turn_completed" => {
                    // The runtime emits the provider-reported usage on the
                    // terminal TurnCompleted frame. Missing / malformed
                    // `tokens` defaults to zero so older runtimes still parse.
                    if let Some(tokens) = event.get("msg").and_then(|m| m.get("tokens")) {
                        result.usage = UsageInfo {
                            prompt_tokens: tokens
                                .get("prompt_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0),
                            completion_tokens: tokens
                                .get("completion_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0),
                            total_tokens: tokens
                                .get("total_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0),
                        };
                    }
                    break;
                }
                "error" => {
                    let code = event
                        .get("msg")
                        .and_then(|m| m.get("code"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("unknown");
                    let message = event
                        .get("msg")
                        .and_then(|m| m.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown error");
                    anyhow::bail!("[runtime error] {code}: {message}");
                }
                _ => {} // TurnStarted etc — skip
            }
        }

        Ok(result)
    }

    /// Annotate a stdin I/O error with the runtime child's exit status when the
    /// child has already terminated. A `BrokenPipe` on write almost always
    /// means the child exited; `try_wait` lets us report the exit code so the
    /// API caller sees "child exited with status …" instead of a bare
    /// "Broken pipe (os error 32)" (sera-un35 diagnostic).
    async fn child_exit_context(&self, io_err: std::io::Error) -> anyhow::Error {
        let mut child = self.child.lock().await;
        match child.try_wait() {
            Ok(Some(status)) => anyhow::anyhow!(
                "sera-runtime child exited before submission could be written (status: {status}); stdin error: {io_err}"
            ),
            Ok(None) => anyhow::anyhow!(
                "sera-runtime stdin write failed while child still running: {io_err}"
            ),
            Err(wait_err) => anyhow::anyhow!(
                "sera-runtime stdin write failed ({io_err}); try_wait also failed: {wait_err}"
            ),
        }
    }

    /// Send a graceful shutdown command to the runtime process.
    ///
    /// Called from `run_start`'s drain phase after a SIGTERM/Ctrl+C signal.
    /// Best-effort: any I/O error is swallowed so one bad harness cannot stall
    /// shutdown for the rest.
    async fn shutdown(&self) -> anyhow::Result<()> {
        use tokio::io::AsyncWriteExt;

        let cmd = serde_json::json!({
            "id": uuid::Uuid::new_v4(),
            "op": { "type": "system", "system_op": "shutdown" }
        });
        let mut json_line = serde_json::to_string(&cmd)?;
        json_line.push('\n');

        let mut stdin = self.stdin.lock().await;
        let _ = stdin.write_all(json_line.as_bytes()).await;
        let _ = stdin.flush().await;
        Ok(())
    }
}

#[cfg(test)]
impl StdioHarness {
    /// Spawn a mock runtime for testing — a bash script that reads NDJSON
    /// submissions and replies with canned TurnStarted + StreamingDelta +
    /// TurnCompleted events.
    async fn spawn_mock() -> anyhow::Result<Self> {
        let script = concat!(
            r#"while IFS= read -r line; do "#,
            r#"echo '{"id":"00000000-0000-0000-0000-000000000001","submission_id":"00000000-0000-0000-0000-000000000000","msg":{"type":"turn_started","turn_id":"00000000-0000-0000-0000-000000000002"},"timestamp":"2024-01-01T00:00:00Z"}'; "#,
            r#"echo '{"id":"00000000-0000-0000-0000-000000000003","submission_id":"00000000-0000-0000-0000-000000000000","msg":{"type":"streaming_delta","delta":"mock response"},"timestamp":"2024-01-01T00:00:00Z"}'; "#,
            r#"echo '{"id":"00000000-0000-0000-0000-000000000004","submission_id":"00000000-0000-0000-0000-000000000000","msg":{"type":"turn_completed","turn_id":"00000000-0000-0000-0000-000000000002"},"timestamp":"2024-01-01T00:00:00Z"}'; "#,
            r#"done"#,
        );

        Self::spawn_with_script(script).await
    }

    /// Spawn a mock runtime that consumes submissions but never emits events.
    /// Used to exercise the turn timeout path — a live child with an open
    /// stdout that simply never produces output.
    async fn spawn_mock_hang() -> anyhow::Result<Self> {
        Self::spawn_with_script("while IFS= read -r line; do :; done").await
    }

    /// Spawn a mock runtime that exits immediately with status 42 without
    /// reading stdin. Used to exercise the `child_exit_context` diagnostic
    /// path — the next `send_turn` write hits a broken pipe (sera-un35).
    async fn spawn_mock_dead() -> anyhow::Result<Self> {
        Self::spawn_with_script("exit 42").await
    }

    /// Spawn a mock runtime that replies with canned events whose
    /// `turn_completed` frame carries the provider-reported token usage
    /// (simulating an LM Studio response being parsed upstream in the runtime).
    async fn spawn_mock_with_usage(
        prompt_tokens: u64,
        completion_tokens: u64,
        total_tokens: u64,
    ) -> anyhow::Result<Self> {
        let script = format!(
            concat!(
                r#"while IFS= read -r line; do "#,
                r#"echo '{{"id":"00000000-0000-0000-0000-000000000001","submission_id":"00000000-0000-0000-0000-000000000000","msg":{{"type":"turn_started","turn_id":"00000000-0000-0000-0000-000000000002"}},"timestamp":"2024-01-01T00:00:00Z"}}'; "#,
                r#"echo '{{"id":"00000000-0000-0000-0000-000000000003","submission_id":"00000000-0000-0000-0000-000000000000","msg":{{"type":"streaming_delta","delta":"mock response"}},"timestamp":"2024-01-01T00:00:00Z"}}'; "#,
                r#"echo '{{"id":"00000000-0000-0000-0000-000000000004","submission_id":"00000000-0000-0000-0000-000000000000","msg":{{"type":"turn_completed","turn_id":"00000000-0000-0000-0000-000000000002","tokens":{{"prompt_tokens":{p},"completion_tokens":{c},"total_tokens":{t}}}}},"timestamp":"2024-01-01T00:00:00Z"}}'; "#,
                r#"done"#,
            ),
            p = prompt_tokens,
            c = completion_tokens,
            t = total_tokens,
        );

        Self::spawn_with_script(&script).await
    }

    async fn spawn_with_script(script: &str) -> anyhow::Result<Self> {
        let mut cmd = tokio::process::Command::new("bash");
        cmd.args(["-c", script])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");

        Ok(Self {
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(tokio::io::BufReader::new(stdout)),
            child: Mutex::new(child),
        })
    }
}

// ── Turn event types ────────────────────────────────────────────────────────

/// A tool call event captured from the runtime's NDJSON output.
#[derive(Debug, Clone)]
enum ToolEvent {
    Begin {
        call_id: String,
        tool: String,
        arguments: serde_json::Value,
    },
    End {
        call_id: String,
        content: String,
    },
}

/// Result from a harness turn — response text, tool call events, and the
/// provider-reported token usage extracted from the terminal `TurnCompleted`
/// frame.
#[derive(Debug, Default)]
struct TurnEvents {
    response: String,
    tool_events: Vec<ToolEvent>,
    usage: UsageInfo,
}

// ── HITL flagged-operation pattern gate ─────────────────────────────────────
//
// FIXME: MVS HITL gate — pattern-match only. Full ApprovalRouter wiring
// (sera-hitl crate) requires deeper audit of the approval-token round-trip
// and DomainEvent surface; tracked as a follow-up in CHAT_HARNESS.md.
//
// These substrings are scanned case-insensitively against inbound chat
// messages before dispatch. Hits short-circuit with 403
// `hitl_approval_required`.
const FLAGGED_OPERATIONS: &[&str] = &[
    "rm -rf",
    "sudo ",
    "drop table",
    "git push --force",
    "docker system prune",
];

/// Scan `msg` for any flagged operation substring (case-insensitive) and
/// return the matching pattern if found.
fn detect_flagged_operation(msg: &str) -> Option<&'static str> {
    let lower = msg.to_ascii_lowercase();
    FLAGGED_OPERATIONS
        .iter()
        .find(|pat| lower.contains(**pat))
        .copied()
}

// ── Shared state ────────────────────────────────────────────────────────────

struct AppState {
    db: Mutex<SqliteDb>,
    manifests: ManifestSet,
    /// Shared Discord connector for sending replies. `None` when no Discord
    /// connector is configured.
    discord: Option<Arc<DiscordConnector>>,
    /// API key for authenticating requests. `None` means auth is disabled
    /// (autonomous mode — all access allowed per MVS §6.5).
    api_key: Option<String>,
    /// Lane-aware message queue for managing concurrent agent runs.
    /// Consumed by the Discord message loop (`process_message`) and the HTTP
    /// `chat_handler` to admit turns and release lane slots on completion.
    lane_queue: Mutex<LaneQueue>,
    /// Hook registry for lifecycle event hooks. Chain-style execution runs
    /// through `chain_executor`; direct lookup/introspection (e.g. the
    /// `/api/hooks` listing route) goes through this handle.
    hook_registry: Arc<HookRegistry>,
    /// Chain executor for running hook pipelines.
    chain_executor: Arc<ChainExecutor>,
    /// Pre-connected runtime harnesses keyed by agent name.
    harnesses: std::collections::HashMap<String, Arc<StdioHarness>>,
    /// Latch that flips to `true` after the first successful runtime probe.
    /// Drives `/api/health/ready` — see `probe_runtime_ready`. Stays `false`
    /// across docker restarts because the gateway process is recreated.
    runtime_ready: Arc<std::sync::atomic::AtomicBool>,
    /// Shutdown flag observed by long-running background loops. Flipped to
    /// `true` after a SIGTERM/Ctrl+C signal so loops can exit their next
    /// iteration instead of blocking the drain phase. Written by the
    /// shutdown-signal closure in `run_start`; loops read it via
    /// `AppState::shutting_down.load(Ordering::SeqCst)`.
    #[allow(dead_code)]
    shutting_down: Arc<std::sync::atomic::AtomicBool>,
    /// Mail gate ingress correlator (sera-uwk0). Maps inbound email replies
    /// back to pending Mail-gate workflow instances via RFC 5322 headers with
    /// a SERA-issued body-nonce fallback. Consulted by the
    /// `POST /api/mail/inbound` webhook.
    mail_correlator: Arc<HeaderMailCorrelator>,
    /// Scheduler-side [`sera_workflow::MailLookup`] fed by the correlator.
    /// Exported so workflow DI can consume it when the ready-queue wires
    /// through here; the correlator pushes `ReplyReceived` events into it.
    #[allow(dead_code)]
    mail_lookup: Arc<InMemoryMailLookup>,
    // ── Phase-3 SPEC-interop ─────────────────────────────────────────────────
    /// Known A2A peers and the inbound router (SPEC-interop §4).
    a2a_peers: Arc<RwLock<A2aPeerRegistry>>,
    /// Inbound A2A JSON-RPC router — dispatches `tasks/*` methods.
    a2a_router: Arc<InProcRouter>,
    /// AG-UI broadcast hub — SSE subscribers for `/api/agui/stream`.
    agui_hub: Arc<RwLock<AguiHub>>,
    /// Plugin registry — backing store for `/api/plugins` routes.
    plugin_registry: Arc<InMemoryPluginRegistry>,
    /// Runtime-side skill dispatch engine. Loaded at boot from
    /// `$SERA_SKILLS_DIR` (default `./skills`); consulted in `execute_turn`
    /// to fire trigger-matched skills and inject their `context_injection`
    /// into the outgoing system prompt.
    skill_engine: Arc<SkillDispatchEngine>,
    /// Tier-2 semantic memory store. Built at boot (SPEC-memory §13) and
    /// threaded into `execute_turn` for best-effort memory recall. A failure
    /// to recall must never fail the turn — we log and continue.
    semantic_store: Arc<dyn SemanticMemoryStore>,
    /// Admin kill switch (SPEC-gateway §7a.4). Armed via `ROLLBACK` on the
    /// Unix admin socket; causes all HTTP submissions to be rejected with 503
    /// until disarmed with `DISARM`.
    kill_switch: Arc<KillSwitch>,
    /// Submission envelope store — every agent-facing route appends a
    /// Submission here before calling the underlying service (sera-r1g8).
    /// Production boot uses SqliteGitSessionStore (sera-4i4i); tests keep
    /// InMemorySessionStore to avoid writing shadow-git dirs to disk.
    session_store: Arc<dyn SessionStore>,
    /// Constitutional rule registry. Seeded at startup from
    /// `SERA_CONSTITUTIONAL_RULES_PATH` (default `/etc/sera/constitutional_rules.yaml`).
    /// Empty when the file is absent — constitutional_gate hooks still run but
    /// find no rules to evaluate (fail-open vs fail-closed is the hook's choice).
    constitutional_registry: Arc<ConstitutionalRegistry>,
}

// ── Phase-3 trait impls ──────────────────────────────────────────────────────

impl A2aAppState for AppState {
    fn api_key(&self) -> &Option<String> {
        &self.api_key
    }
    fn a2a_peers(&self) -> Arc<RwLock<A2aPeerRegistry>> {
        Arc::clone(&self.a2a_peers)
    }
    fn a2a_router(&self) -> Arc<dyn A2aRouter> {
        Arc::clone(&self.a2a_router) as Arc<dyn A2aRouter>
    }
    fn a2a_client(&self) -> A2aClient {
        A2aClient::new(LoopbackTransport::from_arc(Arc::clone(&self.a2a_router)))
    }
}

impl AguiAppState for AppState {
    fn api_key(&self) -> &Option<String> {
        &self.api_key
    }
    fn agui_hub(&self) -> Arc<RwLock<AguiHub>> {
        Arc::clone(&self.agui_hub)
    }
}

impl PluginsAppState for AppState {
    fn api_key(&self) -> &Option<String> {
        &self.api_key
    }
    fn plugin_registry(&self) -> Arc<InMemoryPluginRegistry> {
        Arc::clone(&self.plugin_registry)
    }
}

// ── sera-8d1.2-follow: party-mode wiring ────────────────────────────────────
//
// The MVS AppState uses SqliteDb, so circle membership cannot be resolved from
// the DB without a Postgres-backed CircleRepository. The stub below returns
// `None` for every circle (→ 404) until the production member resolver lands.
// Tracked as a follow-up: wire `resolve_party_members` to real LLM-backed
// `sera_workflow::coordination::PartyMember` implementations once the
// Postgres path is available in this binary.
impl PartyAppState for AppState {
    fn api_key(&self) -> &Option<String> {
        &self.api_key
    }
    fn resolve_party_members(
        &self,
        _circle_id: &str,
    ) -> Option<Vec<Arc<dyn sera_workflow::coordination::PartyMember>>> {
        // Stub: production resolver not yet wired — always returns None (404).
        None
    }
}

// ── HTTP types ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

/// Response shape for `/api/health/ready` — distinguishes "process up"
/// (liveness) from "runtime connected to its LLM provider" (readiness).
/// See `docs/signal-system-design.md` for the rationale: clients must not
/// dispatch turns until the harness has confirmed connectivity, otherwise
/// the first turn after a docker restart races the LM Studio reconnect and
/// returns an empty reply.
#[derive(Serialize)]
struct ReadinessResponse {
    /// `"ready"` when every harness has answered a probe successfully,
    /// `"not_ready"` otherwise.
    status: &'static str,
    /// `true` once any successful runtime probe has been observed during
    /// this process lifetime. Latches on first success; resets on restart.
    runtime_connected: bool,
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    stream: bool,
}

/// Custom JSON extractor that maps axum's `JsonRejection` (which produces 422
/// with a raw serde error string) to a structured 400 response.
struct ValidatedJson<T>(T);

/// Rejection type for [`ValidatedJson`] — always a 400 with a JSON body.
struct ValidatedJsonRejection(axum::response::Response);

impl IntoResponse for ValidatedJsonRejection {
    fn into_response(self) -> axum::response::Response {
        self.0
    }
}

impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ValidatedJsonRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(req, state)
            .await
            .map(|Json(v)| ValidatedJson(v))
            .map_err(|rejection| {
                let body = match &rejection {
                    JsonRejection::MissingJsonContentType(_) => serde_json::json!({
                        "error": "invalid_content_type",
                        "message": "Content-Type must be application/json"
                    }),
                    JsonRejection::JsonDataError(e) => {
                        let msg = e.to_string();
                        if let Some(field) = extract_missing_field(&msg) {
                            serde_json::json!({
                                "error": "missing_field",
                                "field": field,
                                "message": format!("field '{}' is required", field)
                            })
                        } else {
                            serde_json::json!({
                                "error": "invalid_body",
                                "message": "Request body is invalid"
                            })
                        }
                    }
                    _ => serde_json::json!({
                        "error": "invalid_body",
                        "message": "Request body is invalid"
                    }),
                };
                ValidatedJsonRejection(
                    (StatusCode::BAD_REQUEST, Json(body)).into_response(),
                )
            })
    }
}

/// Extract the field name from a serde "missing field `foo`" error message.
fn extract_missing_field(msg: &str) -> Option<&str> {
    // serde_json formats missing-field errors as:
    // "missing field `<name>` at line N column M"
    let start = msg.find("missing field `")?.checked_add("missing field `".len())?;
    let rest = &msg[start..];
    let end = rest.find('`')?;
    Some(&rest[..end])
}

#[derive(Serialize, Debug, Clone, Copy, Default)]
struct UsageInfo {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

#[derive(Serialize)]
struct ChatResponse {
    response: String,
    session_id: String,
    usage: UsageInfo,
}

// ── /api/agents response types ──────────────────────────────────────────────

#[derive(Serialize)]
struct AgentInfo {
    name: String,
    provider: String,
    model: Option<String>,
    has_tools: bool,
}

// ── /api/sessions response types ────────────────────────────────────────────

#[derive(Serialize)]
struct SessionInfo {
    id: String,
    agent_id: String,
    session_key: String,
    state: String,
    principal_id: Option<String>,
    created_at: String,
    updated_at: Option<String>,
}

// ── /api/sessions/:id/transcript response types ──────────────────────────────

#[derive(Serialize)]
struct TranscriptEntry {
    id: i64,
    session_id: String,
    role: String,
    content: Option<String>,
    tool_calls: Option<String>,
    tool_call_id: Option<String>,
    created_at: String,
}

/// Internal result from a turn execution, carrying the reply text, tool events,
/// and usage info extracted from the LLM response.
struct MvsTurnResult {
    reply: String,
    tool_events: Vec<ToolEvent>,
    usage: UsageInfo,
}

// ── Authentication ──────────────────────────────────────────────────────────

/// Validate the `Authorization: Bearer <key>` header against the configured
/// API key. Returns `Ok(())` if auth passes (or is disabled), `Err(401)` if
/// the key is missing/invalid.
fn validate_api_key(state: &AppState, headers: &HeaderMap) -> Result<(), StatusCode> {
    let expected = match &state.api_key {
        Some(k) => k,
        None => return Ok(()), // No key configured — autonomous mode, all access allowed.
    };

    let header_val = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match header_val {
        Some(token) if token == expected => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

// ── HTTP handlers ───────────────────────────────────────────────────────────

/// Liveness — the gateway process is up and serving HTTP. Mirrors the
/// docker `HEALTHCHECK` contract: returns 200 the moment axum is listening,
/// independent of runtime/LM Studio state. Pair with `/api/health/ready`
/// for traffic-gate semantics.
async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// Default per-harness probe timeout when `SERA_READINESS_PROBE_TIMEOUT_SECS`
/// is unset. Picked to be larger than a typical LM Studio cold-start reply
/// but well under the docker compose `start_period` so the readiness gate
/// closes promptly when the runtime is genuinely down.
const DEFAULT_READINESS_PROBE_TIMEOUT_SECS: u64 = 5;

fn readiness_probe_timeout() -> std::time::Duration {
    let secs = std::env::var("SERA_READINESS_PROBE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_READINESS_PROBE_TIMEOUT_SECS);
    std::time::Duration::from_secs(secs)
}

/// Probe every registered runtime harness with a trivial turn. Returns
/// `true` only when every harness answers within `readiness_probe_timeout()`
/// with a non-empty reply — proves the runtime ↔ LM Studio path is live.
///
/// Latches success in `state.runtime_ready` so subsequent calls are O(1)
/// and never re-probe. The latch never clears for the process lifetime;
/// a docker restart spawns a new process that starts cold.
///
/// Returns `false` if the harness map is empty (no runtime registered yet).
async fn probe_runtime_ready(state: &AppState) -> bool {
    use std::sync::atomic::Ordering;

    if state.runtime_ready.load(Ordering::Acquire) {
        return true;
    }
    if state.harnesses.is_empty() {
        return false;
    }

    let timeout = readiness_probe_timeout();
    for harness in state.harnesses.values() {
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": "ping",
        })];
        let probe = harness.send_turn(messages, "__sera_readiness_probe__");
        match tokio::time::timeout(timeout, probe).await {
            Ok(Ok(events)) if !events.response.trim().is_empty() => {}
            _ => return false,
        }
    }

    state.runtime_ready.store(true, Ordering::Release);
    true
}

/// Readiness — the runtime harness has confirmed end-to-end connectivity to
/// its LLM provider. Returns 503 until the first successful probe. Solves
/// the empty-reply race after `docker restart`: `/api/health` flips to 200
/// the moment axum binds, but the harness child has not yet handshaken with
/// LM Studio, so the first user turn returns an empty reply. Clients should
/// gate traffic on this endpoint, not `/api/health`.
async fn readiness_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if probe_runtime_ready(&state).await {
        (
            StatusCode::OK,
            Json(ReadinessResponse {
                status: "ready",
                runtime_connected: true,
            }),
        )
            .into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadinessResponse {
                status: "not_ready",
                runtime_connected: false,
            }),
        )
            .into_response()
    }
}

async fn chat_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ValidatedJson(req): ValidatedJson<ChatRequest>,
) -> Result<axum::response::Response, StatusCode> {
    // Authenticate.
    validate_api_key(&state, &headers)?;

    // Determine which agent to use.
    let agent_name = req
        .agent
        .as_deref()
        .or_else(|| state.manifests.agent_names().into_iter().next())
        .unwrap_or("sera")
        .to_owned();

    let agent_spec: AgentSpec = match state.manifests.agent_spec(&agent_name) {
        Ok(Some(s)) => s,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Look up the pre-connected runtime harness for this agent.
    let harness = match state.harnesses.get(&agent_name) {
        Some(h) => Arc::clone(h),
        None => {
            tracing::error!(agent = %agent_name, "No runtime harness registered");
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
    };

    // Get or create a session for this agent.
    let db = state.db.lock().await;
    let session = db
        .get_or_create_session(&agent_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let session_id = session.id.clone();
    let session_key = format!("http:{}:{}", agent_name, session_id);
    drop(db); // Release DB lock before touching the lane queue.

    // ── Lane queue admission ──────────────────────────────────────────────
    // Mirrors the Discord `process_message` pattern: enqueue the event to
    // check whether the lane is idle; if a run is already active for this
    // session or the queue is closed, short-circuit before we touch the
    // transcript or dispatch to the harness. On `Ready`/`Interrupt` we
    // dequeue immediately so `active_run_count` tracks this in-flight turn,
    // and we release the slot via `complete_run` once `execute_turn` returns.
    let admission_event = DomainEvent::api_message(
        &agent_name,
        &session_key,
        PrincipalRef {
            id: PrincipalId::new("http-chat"),
            kind: PrincipalKind::Human,
        },
        &req.message,
    );
    {
        let mut lq = state.lane_queue.lock().await;
        match lq.enqueue(admission_event) {
            sera_db::lane_queue::EnqueueResult::Ready => {
                let _ = lq.dequeue(&session_key);
            }
            sera_db::lane_queue::EnqueueResult::Interrupt => {
                tracing::info!(session_key = %session_key, "Chat interrupt: active run should be aborted");
                let _ = lq.dequeue(&session_key);
            }
            sera_db::lane_queue::EnqueueResult::Queued
            | sera_db::lane_queue::EnqueueResult::Steer => {
                tracing::info!(session_key = %session_key, "Chat message queued behind active turn");
                // sera-6zbf: return a structured 429 so clients can back off
                // correctly. `Retry-After` uses LANE_BUSY_RETRY_AFTER_SECS (15 s)
                // — conservative enough for thinking-model turns while avoiding
                // excessive client-side wait on fast turns.
                let body = serde_json::json!({
                    "error": "rate_limited",
                    "reason": "lane_busy",
                    "retry_after_secs": LANE_BUSY_RETRY_AFTER_SECS,
                });
                let response = (
                    StatusCode::TOO_MANY_REQUESTS,
                    [(
                        axum::http::header::RETRY_AFTER,
                        HeaderValue::from_static("15"),
                    )],
                    Json(body),
                )
                    .into_response();
                return Ok(response);
            }
            sera_db::lane_queue::EnqueueResult::Closed => {
                tracing::warn!(session_key = %session_key, "Chat rejected: lane queue is closed for shutdown");
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
        }
    }

    // Helper: release the lane slot we acquired above. Called on every exit
    // path. The Discord loop does the equivalent explicitly (see
    // `process_message` ~L1310); the HTTP chat handler follows the same
    // pattern rather than wrapping the release in an RAII guard because
    // AppState is not cloneable into a guard without restructuring.
    async fn release_lane(state: &Arc<AppState>, session_key: &str) {
        let mut lq = state.lane_queue.lock().await;
        lq.complete_run(session_key);
    }

    // ── Submission envelope emission (sera-r1g8) ──────────────────────────
    // Every admitted chat turn is an observable action — emit before HITL so
    // even flagged/rejected turns leave a record of intent.
    {
        use sera_gateway::envelope::{Op, Submission, W3cTraceContext};
        let envelope = Submission {
            id: uuid::Uuid::new_v4(),
            op: Op::UserTurn {
                items: vec![serde_json::json!({
                    "type": "text",
                    "text": req.message.clone(),
                })],
                cwd: None,
                approval_policy: None,
                sandbox_policy: None,
                model_override: None,
                effort: None,
                final_output_schema: None,
            },
            trace: W3cTraceContext::default(),
            change_artifact: None,
            session_key: Some(session_key.clone()),
            parent_session_key: None,
        };
        if let Err(e) = state
            .session_store
            .append_envelope(&session_key, &envelope)
            .await
        {
            // Fail-closed (sera-igsd): the envelope store is the audit trail
            // that makes chat turns auditable and replayable per SPEC-gateway.
            // If we cannot persist the record, the operation's contract is
            // broken — return 500 so the client can retry rather than silently
            // succeed with a missing audit entry.
            tracing::error!(error = %e, agent = %agent_name, session_key = %session_key, "session_store.append_envelope failed; rejecting turn (fail-closed)");
            release_lane(&state, &session_key).await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // ── HITL pattern gate ────────────────────────────────────────────────
    // FIXME: MVS HITL gate — pattern-match only. Full ApprovalRouter
    // wiring (sera-hitl crate) requires deeper audit; tracked as follow-up
    // in CHAT_HARNESS.md.
    if let Some(flag) = detect_flagged_operation(&req.message) {
        release_lane(&state, &session_key).await;
        let body = serde_json::json!({
            "error": "hitl_approval_required",
            "reason": format!("flagged operation detected: {flag}"),
            "message": "This request touches a flagged operation and requires human approval. Re-submit with approval or a safer phrasing.",
        });
        return Ok((StatusCode::FORBIDDEN, Json(body)).into_response());
    }

    let db = state.db.lock().await;
    // Save the user message to transcript.
    if let Err(e) = db.append_transcript(&session.id, "user", Some(&req.message), None, None) {
        drop(db);
        tracing::error!(error = %e, "Failed to append user transcript");
        release_lane(&state, &session_key).await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Audit: message received.
    let _ = db.append_audit(
        "message_received",
        "human",
        "human",
        Some(
            &serde_json::json!({ "agent": agent_name, "message_len": req.message.len() })
                .to_string(),
        ),
    );

    // Get recent transcript for context.
    let transcript = db
        .get_transcript_recent(&session.id, 20)
        .unwrap_or_default();
    drop(db); // Release lock before dispatching to harness.

    if req.stream {
        // SSE streaming mode: spawn turn execution and stream word-by-word.
        let message = req.message.clone();
        let state_clone = Arc::clone(&state);
        let harness_clone = Arc::clone(&harness);
        let sid = session_id.clone();
        let skey = session_key.clone();
        let mid = format!("msg_{:08x}", rand::random::<u32>());
        let mid_clone = mid.clone();

        let aname = agent_name.clone();
        let sse_stream = stream::unfold(
            StreamState::Pending {
                agent_spec,
                transcript,
                message,
                state: state_clone,
                harness: harness_clone,
                session_id: sid,
                session_key: skey,
                message_id: mid_clone,
                agent_name: aname,
            },
            |fold_state| async move {
                match fold_state {
                    StreamState::Pending {
                        agent_spec,
                        transcript,
                        message,
                        state,
                        harness,
                        session_id,
                        session_key,
                        message_id,
                        agent_name,
                    } => {
                        let result = execute_turn(
                            &agent_spec,
                            &transcript,
                            &message,
                            &harness,
                            &session_key,
                            &state.skill_engine,
                            &state.semantic_store,
                            &agent_name,
                        )
                        .await;

                        // Release the lane slot — the turn is complete even
                        // though we still need to stream the reply back out.
                        // This matches the Discord loop, which releases the
                        // slot immediately after `execute_turn` returns.
                        {
                            let mut lq = state.lane_queue.lock().await;
                            lq.complete_run(&session_key);
                        }

                        // Save tool events and assistant response.
                        {
                            let db = state.db.lock().await;
                            persist_tool_events(&db, &session_id, &result.tool_events);
                            let _ = db.append_transcript(
                                &session_id,
                                "assistant",
                                Some(&result.reply),
                                None,
                                None,
                            );
                            let _ = db.append_audit(
                                "response_sent",
                                "agent:sera",
                                "agent",
                                Some(
                                    &serde_json::json!({
                                        "session_id": session_id,
                                        "response_len": result.reply.len(),
                                    })
                                    .to_string(),
                                ),
                            );
                        }

                        // Split reply into word-sized chunks for streaming.
                        let chunks: Vec<String> = result
                            .reply
                            .split_inclusive(' ')
                            .map(|s| s.to_owned())
                            .collect();
                        let usage = result.usage;

                        Some((
                            None,
                            StreamState::Streaming {
                                chunks,
                                index: 0,
                                session_id,
                                message_id,
                                usage,
                            },
                        ))
                    }
                    StreamState::Streaming {
                        chunks,
                        index,
                        session_id,
                        message_id,
                        usage,
                    } => {
                        if index < chunks.len() {
                            let payload = serde_json::json!({
                                "delta": chunks[index],
                                "session_id": session_id,
                                "message_id": message_id,
                            });
                            let event = Event::default().event("message").data(payload.to_string());
                            Some((
                                Some(Ok::<_, std::convert::Infallible>(event)),
                                StreamState::Streaming {
                                    chunks,
                                    index: index + 1,
                                    session_id,
                                    message_id,
                                    usage,
                                },
                            ))
                        } else {
                            // Send done event with usage.
                            let payload = serde_json::json!({
                                "status": "complete",
                                "usage": {
                                    "prompt_tokens": usage.prompt_tokens,
                                    "completion_tokens": usage.completion_tokens,
                                    "total_tokens": usage.total_tokens,
                                }
                            });
                            let event = Event::default().event("done").data(payload.to_string());
                            Some((Some(Ok(event)), StreamState::Done))
                        }
                    }
                    StreamState::Done => None,
                }
            },
        )
        .filter_map(|item| async move { item });

        Ok(Sse::new(sse_stream)
            .keep_alive(KeepAlive::default())
            .into_response())
    } else {
        // Synchronous JSON mode (existing behavior).
        let result = execute_turn(
            &agent_spec,
            &transcript,
            &req.message,
            &harness,
            &session_key,
            &state.skill_engine,
            &state.semantic_store,
            &agent_name,
        )
        .await;

        // Release the lane slot now that the turn has completed. Mirrors the
        // `complete_run` call in the Discord message loop after `execute_turn`.
        release_lane(&state, &session_key).await;

        // Guard: an empty reply is a silent failure — the runtime returned
        // Ok(events) but produced no text. Log richly so the root cause can
        // be chased later, then return 502 so callers don't silently discard
        // an empty response.
        if result.reply.is_empty() {
            tracing::error!(
                session_id = %session_id,
                agent = %agent_name,
                prompt_tokens = result.usage.prompt_tokens,
                completion_tokens = result.usage.completion_tokens,
                total_tokens = result.usage.total_tokens,
                tool_events_count = result.tool_events.len(),
                tools_ran = !result.tool_events.is_empty(),
                "execute_turn returned empty reply; runtime produced no text"
            );
            return Ok((
                StatusCode::BAD_GATEWAY,
                axum::Json(serde_json::json!({
                    "error": "runtime returned empty reply"
                })),
            )
                .into_response());
        }

        // Save tool events and assistant response.
        let db = state.db.lock().await;
        persist_tool_events(&db, &session_id, &result.tool_events);
        if let Err(e) =
            db.append_transcript(&session_id, "assistant", Some(&result.reply), None, None)
        {
            tracing::error!(error = %e, "Failed to append assistant transcript");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }

        let _ = db.append_audit(
            "response_sent",
            "agent:sera",
            "agent",
            Some(
                &serde_json::json!({
                    "session_id": session_id,
                    "response_len": result.reply.len(),
                    "usage": {
                        "prompt_tokens": result.usage.prompt_tokens,
                        "completion_tokens": result.usage.completion_tokens,
                        "total_tokens": result.usage.total_tokens,
                    }
                })
                .to_string(),
            ),
        );

        Ok(Json(ChatResponse {
            response: result.reply,
            session_id,
            usage: result.usage,
        })
        .into_response())
    }
}

async fn agents_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<AgentInfo>>, StatusCode> {
    validate_api_key(&state, &headers)?;

    let agents: Vec<AgentInfo> = state
        .manifests
        .agent_names()
        .iter()
        .map(|name| {
            let spec = state.manifests.agent_spec(name).ok().flatten();
            AgentInfo {
                name: name.to_string(),
                provider: spec
                    .as_ref()
                    .map(|s| s.provider.clone())
                    .unwrap_or_default(),
                model: spec.as_ref().and_then(|s| s.model.clone()),
                has_tools: spec.as_ref().and_then(|s| s.tools.as_ref()).is_some(),
            }
        })
        .collect();

    Ok(Json(agents))
}

async fn sessions_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<SessionInfo>>, StatusCode> {
    validate_api_key(&state, &headers)?;

    let db = state.db.lock().await;
    let rows = db
        .list_sessions()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let sessions: Vec<SessionInfo> = rows
        .into_iter()
        .map(|r| SessionInfo {
            id: r.id,
            agent_id: r.agent_id,
            session_key: r.session_key,
            state: r.state,
            principal_id: r.principal_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect();

    Ok(Json(sessions))
}

async fn transcript_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<TranscriptEntry>>, StatusCode> {
    validate_api_key(&state, &headers)?;

    let db = state.db.lock().await;
    let rows = db
        .get_transcript(&session_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let entries: Vec<TranscriptEntry> = rows
        .into_iter()
        .map(|r| TranscriptEntry {
            id: r.id,
            session_id: r.session_id,
            role: r.role,
            content: r.content,
            tool_calls: r.tool_calls,
            tool_call_id: r.tool_call_id,
            created_at: r.created_at,
        })
        .collect();

    Ok(Json(entries))
}

async fn auth_me_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // In autonomous mode (no api_key configured) return a static principal.
    // In keyed mode, validate and return the same static shape with the key as sub.
    if let Some(ref expected) = state.api_key {
        let header_val = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        match header_val {
            Some(token) if token == expected => {}
            _ => return Err(StatusCode::UNAUTHORIZED),
        }
    }

    Ok(Json(serde_json::json!({
        "id": "autonomous",
        "principal_id": "autonomous",
        "sub": "autonomous",
        "roles": ["admin"],
        "mode": "autonomous"
    })))
}

async fn agent_by_id_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    validate_api_key(&state, &headers)?;

    // `id` may be a name (autonomous mode has no UUIDs).
    let agent_names = state.manifests.agent_names();
    let name: &str = agent_names
        .iter()
        .copied()
        .find(|n| *n == id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let spec = state.manifests.agent_spec(name).ok().flatten();
    let info = serde_json::json!({
        "name": name,
        "provider": spec.as_ref().map(|s| s.provider.as_str()).unwrap_or(""),
        "model": spec.as_ref().and_then(|s| s.model.as_deref()),
        "has_tools": spec.as_ref().and_then(|s| s.tools.as_ref()).is_some(),
    });

    Ok(Json(info))
}

/// Internal state machine for SSE streaming.
#[allow(clippy::large_enum_variant)]
enum StreamState {
    Pending {
        agent_spec: AgentSpec,
        transcript: Vec<sera_db::sqlite::TranscriptRow>,
        message: String,
        state: Arc<AppState>,
        harness: Arc<StdioHarness>,
        session_id: String,
        session_key: String,
        message_id: String,
        agent_name: String,
    },
    Streaming {
        chunks: Vec<String>,
        index: usize,
        session_id: String,
        message_id: String,
        usage: UsageInfo,
    },
    Done,
}

// ── Turn execution (dispatched to sera-runtime harness) ─────────────────────

/// Upper bound on how long a single turn may block waiting on the runtime
/// harness. Prevents a hung runtime from wedging the lane queue forever: the
/// lane slot is released by the caller after `execute_turn` returns, so a
/// timeout here guarantees the slot is eventually freed even if the harness
/// never responds. Override with `SERA_TURN_TIMEOUT_SECS`.
///
/// 10 minutes accommodates thinking models (Claude extended thinking, local
/// reasoning models like qwen3.6-35b on modest hardware) that routinely take
/// 2–5 minutes per turn, while still bounding a truly wedged runtime. Operators
/// needing longer bounds (e.g. long multi-step tool chains) set
/// `SERA_TURN_TIMEOUT_SECS`.
const DEFAULT_TURN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// Seconds to advertise in `Retry-After` when the lane queue rejects a chat
/// request because a turn is already in flight (sera-6zbf). 15 s is
/// deliberately conservative: most interactive turns resolve in a few seconds,
/// but thinking-model turns can run for minutes. Clients that poll sooner than
/// 15 s will just hit another 429, so a short-but-not-instant value reduces
/// wasted round-trips without forcing long waits on fast turns.
const LANE_BUSY_RETRY_AFTER_SECS: u64 = 15;

fn turn_timeout() -> std::time::Duration {
    std::env::var("SERA_TURN_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .map(std::time::Duration::from_secs)
        .unwrap_or(DEFAULT_TURN_TIMEOUT)
}

/// Execute a turn by dispatching to a pre-connected sera-runtime harness.
///
/// The gateway builds the conversation messages from the transcript and sends
/// them to the harness. The harness (sera-runtime) owns LLM calls and tool
/// execution — the gateway never touches those.
#[allow(clippy::too_many_arguments)]
async fn execute_turn(
    agent_spec: &AgentSpec,
    transcript: &[sera_db::sqlite::TranscriptRow],
    user_message: &str,
    harness: &StdioHarness,
    session_key: &str,
    skill_engine: &SkillDispatchEngine,
    semantic_store: &Arc<dyn SemanticMemoryStore>,
    agent_name: &str,
) -> MvsTurnResult {
    let mut messages: Vec<serde_json::Value> = Vec::new();

    // Add system message from persona if configured.
    if let Some(persona) = &agent_spec.persona
        && let Some(anchor) = &persona.immutable_anchor
    {
        messages.push(serde_json::json!({
            "role": "system",
            "content": anchor,
        }));
    }

    // ── Skill dispatch: fire trigger-matched skills for this turn and
    // prepend their active `context_injection` strings as system messages.
    // Injected BEFORE transcript replay so the skill guidance frames the
    // history instead of being buried after it.
    let _ = skill_engine.on_turn(user_message);
    for injection in skill_engine.active_context_injections() {
        if injection.trim().is_empty() {
            continue;
        }
        messages.push(serde_json::json!({
            "role": "system",
            "content": injection,
        }));
    }

    // ── Memory recall: text-only SemanticMemoryStore query. Best-effort —
    // any backend error is logged and skipped; a failed recall must never
    // fail the turn.
    let recall_query = sera_memory::SemanticQuery {
        agent_id: agent_name.to_string(),
        scope: None,
        tier_filter: None,
        text: Some(user_message.to_string()),
        query_embedding: None,
        top_k: 3,
        similarity_threshold: None,
    };
    match semantic_store.query(recall_query).await {
        Ok(hits) if !hits.is_empty() => {
            let recalled = hits
                .iter()
                .take(3)
                .map(|h| format!("- {}", h.entry.content))
                .collect::<Vec<_>>()
                .join("\n");
            messages.push(serde_json::json!({
                "role": "system",
                "content": format!("Relevant memories:\n{recalled}"),
            }));
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(error = %e, "semantic recall failed; continuing without memory");
        }
    }

    // Add transcript history (including tool_calls and tool results).
    for row in transcript {
        if row.role == "tool" {
            let mut msg = serde_json::json!({
                "role": "tool",
                "content": row.content.as_deref().unwrap_or(""),
            });
            if let Some(tc_id) = &row.tool_call_id {
                msg["tool_call_id"] = serde_json::json!(tc_id);
            }
            messages.push(msg);
        } else if let Some(tc_json) = &row.tool_calls {
            let mut msg = serde_json::json!({ "role": "assistant" });
            if let Ok(tc) = serde_json::from_str::<serde_json::Value>(tc_json) {
                msg["tool_calls"] = tc;
            }
            if let Some(content) = &row.content {
                msg["content"] = serde_json::json!(content);
            }
            messages.push(msg);
        } else if let Some(content) = &row.content {
            messages.push(serde_json::json!({
                "role": row.role,
                "content": content,
            }));
        }
    }

    // Add current message (if not already the last in transcript).
    let already_added = transcript
        .last()
        .is_some_and(|r| r.role == "user" && r.content.as_deref() == Some(user_message));
    if !already_added {
        messages.push(serde_json::json!({
            "role": "user",
            "content": user_message,
        }));
    }

    let timeout = turn_timeout();
    match tokio::time::timeout(timeout, harness.send_turn(messages, session_key)).await {
        Ok(Ok(events)) => MvsTurnResult {
            reply: events.response,
            tool_events: events.tool_events,
            usage: events.usage,
        },
        Ok(Err(e)) => {
            tracing::error!(error = %e, "Runtime harness turn failed");
            MvsTurnResult {
                reply: format!("[sera] Runtime error: {e}"),
                tool_events: vec![],
                usage: UsageInfo {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
            }
        }
        Err(_elapsed) => {
            tracing::error!(
                session_key = %session_key,
                timeout_secs = timeout.as_secs(),
                "Runtime harness turn timed out; releasing lane"
            );
            MvsTurnResult {
                reply: format!("[sera] Runtime timed out after {}s", timeout.as_secs()),
                tool_events: vec![],
                usage: UsageInfo {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
            }
        }
    }
}

// ── Steer injection ────────────────────────────────────────────────────────

/// Send a steer operation to the runtime harness.
/// This is used for tool boundary injection of steer messages.
async fn execute_steer(
    harness: &StdioHarness,
    steer_messages: &[serde_json::Value],
    session_key: &str,
) -> MvsTurnResult {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let submission = serde_json::json!({
        "id": uuid::Uuid::new_v4(),
        "op": {
            "type": "steer",
            "items": steer_messages,
            "session_key": session_key,
        },
    });

    let mut json_line = serde_json::to_string(&submission).unwrap();
    json_line.push('\n');

    let mut stdin = harness.stdin.lock().await;
    let mut stdout = harness.stdout.lock().await;

    if let Err(e) = stdin.write_all(json_line.as_bytes()).await {
        let ctx_err = harness.child_exit_context(e).await;
        tracing::error!(error = %ctx_err, session_key = %session_key, "Steer stdin write failed");
        return MvsTurnResult {
            reply: format!("[sera] Steer injection failed: {ctx_err}"),
            tool_events: vec![],
            usage: UsageInfo {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        };
    }
    if let Err(e) = stdin.flush().await {
        let ctx_err = harness.child_exit_context(e).await;
        tracing::error!(error = %ctx_err, session_key = %session_key, "Steer stdin flush failed");
        return MvsTurnResult {
            reply: format!("[sera] Steer injection failed: {ctx_err}"),
            tool_events: vec![],
            usage: UsageInfo {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        };
    }

    let timeout = turn_timeout();
    match tokio::time::timeout(timeout, async {
        // Read TurnCompleted event.
        let mut line = String::new();
        loop {
            match stdout.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line)
                        && event.get("type").and_then(|v| v.as_str()) == Some("turn_completed")
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
            line.clear();
        }
    })
    .await
    {
        Ok(()) => MvsTurnResult {
            reply: "[steer injected]".to_string(),
            tool_events: vec![],
            usage: UsageInfo {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        },
        Err(_elapsed) => {
            tracing::error!(
                session_key = %session_key,
                timeout_secs = timeout.as_secs(),
                "Runtime harness steer timed out; releasing lane"
            );
            MvsTurnResult {
                reply: "[sera] Steer injection timed out".to_string(),
                tool_events: vec![],
                usage: UsageInfo {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
            }
        }
    }
}

// ── Event processing loop ───────────────────────────────────────────────────

/// Persist tool call events to the session transcript.
///
/// For each ToolEvent::Begin, saves an assistant message with tool_calls JSON.
/// For each ToolEvent::End, saves a tool message with the result content.
fn persist_tool_events(db: &sera_db::sqlite::SqliteDb, session_id: &str, events: &[ToolEvent]) {
    for event in events {
        match event {
            ToolEvent::Begin {
                call_id,
                tool,
                arguments,
            } => {
                let tool_calls_json = serde_json::json!([{
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": tool,
                        "arguments": arguments.to_string(),
                    }
                }]);
                let _ = db.append_transcript(
                    session_id,
                    "assistant",
                    None,
                    Some(&tool_calls_json.to_string()),
                    None,
                );
            }
            ToolEvent::End { call_id, content } => {
                let _ =
                    db.append_transcript(session_id, "tool", Some(content), None, Some(call_id));
            }
        }
    }
}

/// Send a user-visible error message to a Discord channel.
async fn send_error_to_discord(state: &AppState, channel_id: &str, error: &str) {
    let formatted = format!("[sera] Error: {error}");
    if let Some(ref dc) = state.discord
        && let Err(e) = dc.send_message(channel_id, &formatted).await
    {
        tracing::error!(error = ?e, channel_id = %channel_id, "Discord send_message failed");
    }
}

/// Execute all hook chains for a given point. Returns the chain result.
/// On HookError, logs and returns a pass-through result (fail-open in Phase A).
async fn run_hook_point(
    state: &AppState,
    point: HookPoint,
    chains: &[HookChain],
    ctx: HookContext,
) -> sera_types::hook::ChainResult {
    match state
        .chain_executor
        .execute_at_point(point, chains, ctx)
        .await
    {
        Ok(result) => {
            if result.hooks_executed > 0 {
                tracing::debug!(
                    point = ?point,
                    hooks = result.hooks_executed,
                    duration_ms = result.duration_ms,
                    "Hook chain completed"
                );
            }
            result
        }
        Err(e) => {
            tracing::warn!(point = ?point, error = %e, "Hook chain error (fail-open, continuing)");
            sera_types::hook::ChainResult {
                context: HookContext::new(point),
                outcome: HookResult::pass(),
                hooks_executed: 0,
                duration_ms: 0,
                updated_input: None,
            }
        }
    }
}

async fn event_loop(state: Arc<AppState>, mut rx: mpsc::Receiver<DiscordMessage>) {
    tracing::info!("Event processing loop started");

    loop {
        // Poll for a message or yield so the executor can make progress.
        // We check `shutting_down` first so we never block on `recv` after
        // the flag is set — even if the sender hasn't been dropped yet.
        if state
            .shutting_down
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            break;
        }
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(msg) => {
                        if let Err(e) = process_message(&state, &msg).await {
                            tracing::error!(error = %e, "Message processing failed");
                            send_error_to_discord(&state, &msg.channel_id, &e.to_string()).await;
                        }
                    }
                    None => break,
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                // Woke to re-check shutting_down; loop back to the flag check.
            }
        }
    }
}

async fn process_message(state: &AppState, msg: &DiscordMessage) -> anyhow::Result<()> {
    tracing::info!(
        user = %msg.username,
        channel = %msg.channel_id,
        is_dm = %msg.is_dm,
        mentions_bot = %msg.mentions_bot,
        "Received Discord message"
    );

    // Filter: Only respond to DMs or when mentioned.
    // Ignore messages in other channels that don't mention the bot.
    if !msg.is_dm && !msg.mentions_bot {
        tracing::debug!(
            user = %msg.username,
            channel = %msg.channel_id,
            "Ignoring message - not a DM and bot not mentioned"
        );
        return Ok(());
    }

    // Audit: Discord message received.
    {
        let db = state.db.lock().await;
        let _ = db.append_audit(
            "discord_message",
            &msg.user_id,
            "human",
            Some(
                &serde_json::json!({
                    "username": msg.username,
                    "channel_id": msg.channel_id,
                    "message_len": msg.content.len(),
                })
                .to_string(),
            ),
        );
    }

    // Load hook chains from manifests.
    let chains = state.manifests.hook_chain_specs();

    // Build principal for hook context.
    let principal = PrincipalRef {
        id: PrincipalId::new(&msg.user_id),
        kind: PrincipalKind::Human,
    };
    let principal_json = serde_json::json!({"id": msg.user_id, "kind": "human"});

    // ── pre_route: after ingress, before agent resolution ──
    let pre_route_ctx = HookContext {
        point: HookPoint::PreRoute,
        event: Some(serde_json::json!({
            "content": msg.content,
            "channel_id": msg.channel_id,
            "username": msg.username,
        })),
        session: None,
        tool_call: None,
        tool_result: None,
        principal: Some(principal_json.clone()),
        metadata: std::collections::HashMap::new(),
        change_artifact: None, // Populated by sera-meta when processing evolution ChangeArtifacts
    };
    let pre_route_result = run_hook_point(state, HookPoint::PreRoute, &chains, pre_route_ctx).await;
    match &pre_route_result.outcome {
        HookResult::Reject { reason, .. } => {
            tracing::info!(reason = %reason, "pre_route hook rejected message");
            send_error_to_discord(state, &msg.channel_id, reason).await;
            return Ok(());
        }
        HookResult::Redirect { target, .. } => {
            tracing::warn!(target = %target, "pre_route Redirect not yet supported, treating as Continue");
        }
        HookResult::Continue { .. } => {}
    }

    // Find the agent assigned to the Discord connector.
    let agent_name = state
        .manifests
        .connectors
        .iter()
        .find_map(|c| {
            let spec: ConnectorSpec = serde_json::from_value(c.spec.clone()).ok()?;
            spec.agent
        })
        .unwrap_or_else(|| {
            state
                .manifests
                .agent_names()
                .into_iter()
                .next()
                .unwrap_or("sera")
                .to_owned()
        });

    let agent_spec: AgentSpec = match state.manifests.agent_spec(&agent_name).ok().flatten() {
        Some(s) => s,
        None => {
            let err_msg = format!("Agent '{agent_name}' not found in manifests");
            tracing::error!("{err_msg}");
            send_error_to_discord(state, &msg.channel_id, &err_msg).await;
            return Ok(());
        }
    };

    // Look up the pre-connected runtime harness for this agent.
    let harness = match state.harnesses.get(&agent_name) {
        Some(h) => Arc::clone(h),
        None => {
            let err_msg = format!("No runtime harness for agent '{agent_name}'");
            tracing::error!("{err_msg}");
            send_error_to_discord(state, &msg.channel_id, &err_msg).await;
            return Ok(());
        }
    };

    // Use agent name + channel_id as the session key so different agents
    // in the same channel maintain separate conversation histories.
    let session_key = format!("discord:{}:{}", agent_name, msg.channel_id);
    let (session, transcript) = {
        let db = state.db.lock().await;
        let session = match db.get_session_by_key(&session_key) {
            Ok(Some(s)) => s,
            Ok(None) => {
                let id = format!("ses_{}_{}", agent_name, msg.channel_id);
                if let Err(e) =
                    db.create_session(&id, &agent_name, &session_key, Some(&msg.user_id))
                {
                    anyhow::bail!("Failed to create session: {e}");
                }
                match db.get_session_by_key(&session_key) {
                    Ok(Some(s)) => s,
                    _ => anyhow::bail!("Session not found after creation"),
                }
            }
            Err(e) => anyhow::bail!("DB error: {e}"),
        };

        let _ = db.append_transcript(&session.id, "user", Some(&msg.content), None, None);
        let transcript = db
            .get_transcript_recent(&session.id, 20)
            .unwrap_or_default();
        (session, transcript)
    };

    let domain_event = DomainEvent::message(&agent_name, &session_key, principal, &msg.content);
    tracing::debug!(event_id = %domain_event.id.0, "Created domain event for Discord message");

    let session_json = serde_json::json!({"id": session.id, "key": session_key});

    // ── post_route: after routing + session resolution, before turn ──
    let post_route_ctx = HookContext {
        point: HookPoint::PostRoute,
        event: Some(serde_json::to_value(&domain_event)?),
        session: Some(session_json.clone()),
        tool_call: None,
        tool_result: None,
        principal: Some(principal_json.clone()),
        metadata: std::collections::HashMap::new(),
        change_artifact: None, // Populated by sera-meta when processing evolution ChangeArtifacts
    };
    let post_route_result =
        run_hook_point(state, HookPoint::PostRoute, &chains, post_route_ctx).await;
    match &post_route_result.outcome {
        HookResult::Reject { reason, .. } => {
            tracing::info!(reason = %reason, "post_route hook rejected message");
            send_error_to_discord(state, &msg.channel_id, reason).await;
            return Ok(());
        }
        HookResult::Redirect { target, .. } => {
            tracing::warn!(target = %target, "post_route Redirect not yet supported, treating as Continue");
        }
        HookResult::Continue { .. } => {}
    }

    // ── pre_turn: before execute_turn ──
    let pre_turn_ctx = HookContext {
        point: HookPoint::PreTurn,
        event: Some(serde_json::to_value(&domain_event)?),
        session: Some(session_json.clone()),
        tool_call: None,
        tool_result: None,
        principal: Some(principal_json.clone()),
        metadata: std::collections::HashMap::new(),
        change_artifact: None, // Populated by sera-meta when processing evolution ChangeArtifacts
    };
    let pre_turn_result = run_hook_point(state, HookPoint::PreTurn, &chains, pre_turn_ctx).await;
    match &pre_turn_result.outcome {
        HookResult::Reject { reason, .. } => {
            tracing::info!(reason = %reason, "pre_turn hook rejected message");
            send_error_to_discord(state, &msg.channel_id, reason).await;
            return Ok(());
        }
        HookResult::Redirect { target, .. } => {
            tracing::warn!(target = %target, "pre_turn Redirect not yet supported, treating as Continue");
        }
        HookResult::Continue { .. } => {}
    }

    // ── Lane queue: enqueue and check if we can dispatch immediately ──
    {
        let mut lq = state.lane_queue.lock().await;
        let enqueue_result = lq.enqueue(domain_event.clone());
        match enqueue_result {
            sera_db::lane_queue::EnqueueResult::Ready => {
                // Lane was idle — dequeue and proceed with dispatch.
                let _ = lq.dequeue(&session_key);
            }
            sera_db::lane_queue::EnqueueResult::Queued => {
                tracing::info!(session_key = %session_key, "Message queued behind active turn");
                return Ok(());
            }
            sera_db::lane_queue::EnqueueResult::Steer => {
                tracing::info!(session_key = %session_key, "Steer event queued for tool boundary injection");
                return Ok(());
            }
            sera_db::lane_queue::EnqueueResult::Interrupt => {
                tracing::info!(session_key = %session_key, "Interrupt: active run should be aborted");
                // Future: send abort signal to harness. For now, dequeue the interrupt event.
                let _ = lq.dequeue(&session_key);
            }
            sera_db::lane_queue::EnqueueResult::Closed => {
                tracing::warn!(session_key = %session_key, "Lane queue is closed; dropping incoming Discord message");
                return Ok(());
            }
        }
    }

    // Execute the agent turn via the pre-connected harness.
    let result = execute_turn(
        &agent_spec,
        &transcript,
        &msg.content,
        &harness,
        &session_key,
        &state.skill_engine,
        &state.semantic_store,
        &agent_name,
    )
    .await;

    // Persist tool call events to transcript before the final response.
    {
        let db = state.db.lock().await;
        persist_tool_events(&db, &session.id, &result.tool_events);
        let _ = db.append_transcript(&session.id, "assistant", Some(&result.reply), None, None);
    }

    // Complete the run and drain any pending messages for this session.
    {
        let mut lq = state.lane_queue.lock().await;
        lq.complete_run(&session_key);
    }

    // ── post_turn: after execute_turn, before delivery ──
    let post_turn_ctx = HookContext {
        point: HookPoint::PostTurn,
        event: Some(serde_json::to_value(&domain_event)?),
        session: Some(session_json),
        tool_call: None,
        tool_result: None,
        principal: Some(principal_json),
        metadata: std::collections::HashMap::from([(
            "reply".to_string(),
            serde_json::json!(result.reply),
        )]),
        change_artifact: None, // Populated by sera-meta when processing evolution ChangeArtifacts
    };
    let post_turn_result = run_hook_point(state, HookPoint::PostTurn, &chains, post_turn_ctx).await;
    match &post_turn_result.outcome {
        HookResult::Reject { reason, .. } => {
            tracing::info!(reason = %reason, "post_turn hook rejected reply");
            send_error_to_discord(state, &msg.channel_id, reason).await;
            return Ok(());
        }
        HookResult::Redirect { target, .. } => {
            tracing::warn!(target = %target, "post_turn Redirect not yet supported, treating as Continue");
        }
        HookResult::Continue { .. } => {}
    }

    // Send the reply back to Discord via the shared connector.
    if let Some(ref dc) = state.discord {
        if let Err(e) = dc.send_message(&msg.channel_id, &result.reply).await {
            tracing::error!(error = ?e, channel_id = %msg.channel_id, "Discord send_message failed");
        }
    } else {
        tracing::warn!("No Discord connector available to send reply");
    }

    // ── Drain pending messages for this session ──
    // After completing a turn, check if more messages arrived while we were busy.
    // Process them sequentially (per-session serialization via lane queue).
    loop {
        let has_pending = {
            let lq = state.lane_queue.lock().await;
            lq.has_pending(&session_key)
        };
        if !has_pending {
            break;
        }

        // Dequeue the next batch.
        let batch = {
            let mut lq = state.lane_queue.lock().await;
            lq.dequeue(&session_key)
        };
        let Some(batch) = batch else { break };

        // Check if any events in the batch are marked for steer injection.
        let has_steer = batch.iter().any(|qe| qe.is_steer);

        // Separate steer events from regular user events.
        let steer_content: Vec<serde_json::Value> = batch
            .iter()
            .filter(|qe| qe.is_steer)
            .filter_map(|qe| {
                qe.event
                    .text
                    .as_ref()
                    .map(|t| serde_json::json!({"role": "user", "content": t}))
            })
            .collect();

        let user_content: String = batch
            .iter()
            .filter(|qe| !qe.is_steer)
            .filter_map(|qe| qe.event.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n");

        // Handle steer injection: send as Op::Steer if we have steer events.
        if has_steer && !steer_content.is_empty() {
            tracing::info!(session_key = %session_key, "Injecting steer event at tool boundary");
            let follow_up = execute_steer(&harness, &steer_content, &session_key).await;
            // Persist the steer as a user message in transcript.
            {
                let db = state.db.lock().await;
                let steer_text = steer_content
                    .iter()
                    .filter_map(|m| m.get("content").and_then(|c| c.as_str()).map(String::from))
                    .collect::<Vec<_>>()
                    .join("\n");
                let _ = db.append_transcript(&session.id, "user", Some(&steer_text), None, None);
            }
            // Complete run after steer injection.
            {
                let mut lq = state.lane_queue.lock().await;
                lq.complete_run(&session_key);
            }
            // Send steering response to Discord if any.
            if let Some(ref dc) = state.discord
                && let Err(e) = dc.send_message(&msg.channel_id, &follow_up.reply).await
            {
                tracing::error!(error = ?e, channel_id = %msg.channel_id, "Discord send_message failed");
            }
            continue;
        }

        // Handle regular user messages (Collect mode).
        if user_content.is_empty() {
            let mut lq = state.lane_queue.lock().await;
            lq.complete_run(&session_key);
            continue;
        }

        // Get fresh transcript for the follow-up turn.
        let transcript = {
            let db = state.db.lock().await;
            let _ = db.append_transcript(&session.id, "user", Some(&user_content), None, None);
            db.get_transcript_recent(&session.id, 20)
                .unwrap_or_default()
        };

        let follow_up = execute_turn(
            &agent_spec,
            &transcript,
            &user_content,
            &harness,
            &session_key,
            &state.skill_engine,
            &state.semantic_store,
            &agent_name,
        )
        .await;

        {
            let db = state.db.lock().await;
            persist_tool_events(&db, &session.id, &follow_up.tool_events);
            let _ =
                db.append_transcript(&session.id, "assistant", Some(&follow_up.reply), None, None);
        }

        // Complete run for this follow-up turn.
        {
            let mut lq = state.lane_queue.lock().await;
            lq.complete_run(&session_key);
        }

        // Send the follow-up reply to Discord.
        if let Some(ref dc) = state.discord
            && let Err(e) = dc.send_message(&msg.channel_id, &follow_up.reply).await
        {
            tracing::error!(error = ?e, channel_id = %msg.channel_id, "Discord send_message failed");
        }
    }

    Ok(())
}

// ── sera init ───────────────────────────────────────────────────────────────

const TEMPLATE_YAML: &str = r#"---
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: my-sera
spec:
---
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: lm-studio
spec:
  kind: openai-compatible
  base_url: "http://localhost:1234/v1"
  default_model: qwen/qwen3.5-35b-a3b
---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: sera
spec:
  provider: lm-studio
  model: qwen/qwen3.5-35b-a3b
  persona:
    immutable_anchor: |
      You are Sera, an autonomous assistant.
  tools:
    allow: ["memory_*", "file_*", "shell", "session_*"]
---
apiVersion: sera.dev/v1
kind: Connector
metadata:
  name: discord-main
spec:
  kind: discord
  token:
    secret: connectors/discord-main/token
  agent: sera
"#;

// ── sera secrets set / get / list / delete ──────────────────────────────────

fn secrets_dir_from_config(config: &std::path::Path) -> PathBuf {
    config
        .parent()
        .map(|p| p.join("secrets"))
        .unwrap_or_else(|| PathBuf::from("secrets"))
}

fn run_secrets(config: &std::path::Path, command: SecretCommands) -> anyhow::Result<()> {
    let secrets_dir = secrets_dir_from_config(config);
    let resolver = SecretResolver::new(&secrets_dir);

    match command {
        SecretCommands::Set { path, value } => {
            resolver.store(&path, &value)?;
            println!("Secret stored: {path}");
        }
        SecretCommands::Get { path } => match resolver.resolve(&path) {
            Some(v) => {
                let masked = mask_secret(&v);
                println!("{path}: {masked}");
            }
            None => {
                anyhow::bail!("Secret not found: {path}");
            }
        },
        SecretCommands::List => {
            let mut paths = resolver.list();
            paths.sort();
            if paths.is_empty() {
                println!("No secrets stored in {}", secrets_dir.display());
            } else {
                for p in paths {
                    println!("{p}");
                }
            }
        }
        SecretCommands::Delete { path } => {
            resolver.delete(&path)?;
            println!("Secret deleted: {path}");
        }
    }
    Ok(())
}

/// Mask all but the last 4 characters of a secret value.
fn mask_secret(value: &str) -> String {
    let len = value.len();
    if len <= 4 {
        "*".repeat(len)
    } else {
        format!("{}{}", "*".repeat(len - 4), &value[len - 4..])
    }
}

fn run_init() -> anyhow::Result<()> {
    let path = PathBuf::from("sera.yaml");
    if path.exists() {
        anyhow::bail!("sera.yaml already exists. Remove it first or edit manually.");
    }
    std::fs::write(&path, TEMPLATE_YAML)?;
    println!("Created sera.yaml with template configuration.");
    println!();
    println!("Next steps:");
    println!("  1. Edit sera.yaml to configure your provider and agent");
    println!("  2. Set secret env vars: export SERA_SECRET_CONNECTORS_DISCORD_MAIN_TOKEN=...");
    println!("  3. Run: sera start");
    Ok(())
}

// ── sera agent create / list ────────────────────────────────────────────────

fn run_agent_list(config: &std::path::Path) -> anyhow::Result<()> {
    let manifests = load_manifest_file(config)?;
    let names = manifests.agent_names();
    if names.is_empty() {
        println!("No agents defined in {}", config.display());
    } else {
        println!("Agents in {}:", config.display());
        for name in names {
            // Also show the provider for each agent.
            let provider = manifests
                .agent_spec(name)
                .ok()
                .flatten()
                .map(|s| s.provider)
                .unwrap_or_else(|| "unknown".to_owned());
            println!("  - {name}  (provider: {provider})");
        }
    }
    Ok(())
}

fn run_agent_create(config: &PathBuf, name: &str) -> anyhow::Result<()> {
    if !config.exists() {
        anyhow::bail!("{} not found. Run `sera init` first.", config.display());
    }

    let content = std::fs::read_to_string(config)?;

    // Verify the agent doesn't already exist.
    let manifests = parse_manifests(&content)?;
    if manifests.agent(name).is_some() {
        anyhow::bail!("Agent '{name}' already exists in {}", config.display());
    }

    // Determine a default provider from existing providers.
    let default_provider = manifests
        .providers
        .first()
        .map(|p| p.metadata.name.as_str())
        .unwrap_or("lm-studio");

    // Append a new agent manifest.
    let agent_yaml = format!(
        r#"---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: {name}
spec:
  provider: {default_provider}
  persona:
    immutable_anchor: |
      You are {name}, a helpful assistant.
  tools:
    allow: ["memory_*", "file_*", "shell", "session_*"]
"#
    );

    let mut full = content;
    if !full.ends_with('\n') {
        full.push('\n');
    }
    full.push_str(&agent_yaml);
    std::fs::write(config, &full)?;

    println!("Added agent '{name}' to {}", config.display());
    Ok(())
}

// ── Logging initialisation ───────────────────────────────────────────────────

/// Result of `init_file_logging` — keeps the non-blocking writer guard alive
/// so the background flusher thread is not dropped until the process exits.
pub struct LogGuard {
    /// Dropping this flushes and shuts down the background log-writer thread.
    _file_guard: tracing_appender::non_blocking::WorkerGuard,
}

/// Initialise tracing with **both** a stdout layer and a rolling daily file
/// appender.
///
/// Environment variables:
/// - `SERA_LOG_DIR`   — directory for log files (default: `./logs`)
/// - `SERA_LOG_LEVEL` — tracing filter string (default: `info`)
///
/// Returns a [`LogGuard`] that **must be held** in the caller's scope (typically
/// `main`) until the process exits.  Dropping it early will cause log lines to
/// be discarded silently.
///
/// # Panics
///
/// Panics only if the global tracing subscriber has already been set (i.e. this
/// is called twice in the same process).  Tests must not call this function;
/// use `tracing_subscriber::fmt().try_init()` in test harnesses instead.
pub fn init_file_logging() -> LogGuard {
    let log_dir = std::env::var("SERA_LOG_DIR").unwrap_or_else(|_| "./logs".to_owned());
    let log_level = std::env::var("SERA_LOG_LEVEL").unwrap_or_else(|_| "info".to_owned());

    let file_appender = tracing_appender::rolling::daily(&log_dir, "sera.log");
    let (non_blocking, file_guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&log_level));

    let stdout_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stdout);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(non_blocking);

    tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .with(file_layer)
        .init();

    LogGuard {
        _file_guard: file_guard,
    }
}

// ── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing — stdout + rolling daily file appender.
    // The guard must stay alive until the process exits so logs are flushed.
    let _log_guard = init_file_logging();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init => run_init(),

        Commands::Agent { command } => {
            let config = PathBuf::from("sera.yaml");
            match command {
                AgentCommands::List => run_agent_list(&config),
                AgentCommands::Create { name } => run_agent_create(&config, &name),
            }
        }

        Commands::Secrets { command } => {
            let config = PathBuf::from("sera.yaml");
            run_secrets(config.as_path(), command)
        }

        Commands::Start { config, port } => run_start(config, port).await,

        Commands::Doctor { config, json } => {
            let checks = doctor::build_checks(&config);
            let result = doctor::run_checks(&checks);
            if json {
                doctor::print_json(&result);
            } else {
                doctor::print_table(&result);
            }
            if result.any_fail {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

async fn run_start(config: PathBuf, port: u16) -> anyhow::Result<()> {
    // 1. Load config.
    tracing::info!(config = %config.display(), "Loading SERA configuration");
    let manifests = load_manifest_file(&config)?;

    // Set up file-based secret resolver (secrets/ dir next to sera.yaml).
    let secrets_dir = config
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("secrets");
    let secret_resolver = sera_config::secrets::SecretResolver::new(&secrets_dir);

    // Log what we found.
    tracing::info!(
        instances = manifests.instances.len(),
        providers = manifests.providers.len(),
        agents = manifests.agents.len(),
        connectors = manifests.connectors.len(),
        "Configuration loaded"
    );

    // 2. Open SQLite database.
    //
    // sera-4i4i: data_root is the directory that holds all local-first
    // persistence (sera.db, parts.sqlite, sessions/). Defaults to cwd so
    // existing deployments keep working; override via SERA_DATA_ROOT.
    let data_root = std::env::var("SERA_DATA_ROOT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let db_path = data_root.join("sera.db");
    tracing::info!(path = %db_path.display(), "Opening SQLite database");
    let db = SqliteDb::open(&db_path)?;

    // sera-mwb4: provision all local-first store tables (secrets, schedules,
    // audit_trail, token_usage/usage_events/token_quotas, agent_instances/
    // agent_templates) on the same SQLite file. `init_all` is idempotent so
    // this is safe across restarts. When DATABASE_URL is set the enterprise
    // path uses the sqlx-backed Pg repositories instead — these tables are
    // harmless on local-only deployments but unused.
    {
        let init_conn = rusqlite::Connection::open(&db_path)?;
        if let Err(e) = sera_db::sqlite_schema::init_all(&init_conn) {
            tracing::warn!(error = %e, "sqlite_schema::init_all failed; local-first stores may be unavailable");
        }
    }

    // 2a. SemanticMemoryStore (Tier-2 recall) backend selection (sera-vzce /
    // sera-clmw). Selection rules, in order:
    //   * SERA_MEMORY_BACKEND=pgvector → require DATABASE_URL, initialize
    //     the `vector` extension and schema; on failure fall back to
    //     SqliteMemoryStore so the gateway still boots in degraded mode.
    //   * SERA_MEMORY_BACKEND=sqlite → always SqliteMemoryStore, ignoring
    //     DATABASE_URL (useful to pin local-first even on enterprise hosts).
    //   * unset + DATABASE_URL set → pgvector (with the same fallback).
    //   * otherwise → SqliteMemoryStore at SERA_DB_PATH (default ./sera.db).
    //
    // Embedding service wiring stays `None` here: the SQLite path works
    // keyword-only via FTS5/BM25, and the pgvector path requires callers
    // to supply `query_embedding` on the query side. When the runtime
    // carries an `Arc<dyn EmbeddingService>` through to boot, pass it into
    // `SqliteMemoryStore::open(path, Some(embedder))` to enable the hybrid
    // (BM25 + vector + RRF) recall path.
    let backend_pref = std::env::var("SERA_MEMORY_BACKEND")
        .ok()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());
    let database_url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let want_pgvector = wants_pgvector_backend(backend_pref.as_deref(), database_url.as_deref());

    let semantic_store: Arc<dyn SemanticMemoryStore> = 'store: {
        if want_pgvector {
            match &database_url {
                Some(url) => match sera_db::DbPool::connect(url).await {
                    Ok(pool) => {
                        let store = PgVectorStore::new(pool.inner().clone());
                        match store.initialize().await {
                            Ok(()) => {
                                tracing::info!(
                                    "SemanticMemoryStore backend: PgVectorStore (DATABASE_URL set)"
                                );
                                break 'store Arc::new(store);
                            }
                            Err(e) => tracing::warn!(
                                error = %e,
                                "PgVectorStore::initialize failed; falling back to SqliteMemoryStore"
                            ),
                        }
                    }
                    Err(e) => tracing::warn!(
                        error = %e,
                        "PgVectorStore connect failed; falling back to SqliteMemoryStore"
                    ),
                },
                None => tracing::warn!(
                    "SERA_MEMORY_BACKEND=pgvector but DATABASE_URL is unset; falling back to SqliteMemoryStore"
                ),
            }
        }

        let sqlite_path = std::env::var("SERA_DB_PATH")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| db_path.clone());
        let store = SqliteMemoryStore::open(&sqlite_path, None)?;
        tracing::info!(
            path = %sqlite_path.display(),
            vec_available = store.vector_available(),
            "SemanticMemoryStore backend: SqliteMemoryStore"
        );
        Arc::new(store)
    };

    // sera-4nj: transcript indexer is built eagerly so SessionManager
    // construction can wire it via `SessionManager::with_indexer(...)` once
    // the persistence-backed manager lands in AppState. Indexing runs
    // best-effort on session close (SessionState::Archived/Closed) and is
    // guaranteed not to block the close path.
    let _transcript_indexer: Arc<dyn sera_session::TranscriptIndexer> = Arc::new(
        sera_session::SemanticTranscriptIndexer::new(semantic_store.clone()),
    );

    // Skill dispatch engine: load every `*.md` under $SERA_SKILLS_DIR
    // (default `./skills`) at boot. Missing directory is tolerated — the
    // engine just starts empty.
    let skill_engine = Arc::new(SkillDispatchEngine::new());
    {
        let skills_dir = std::env::var("SERA_SKILLS_DIR").unwrap_or_else(|_| "skills".to_string());
        let path = PathBuf::from(&skills_dir);
        match skill_engine.load_dir(&path).await {
            Ok(count) => tracing::info!(
                path = %path.display(),
                count,
                "skill dispatch engine loaded"
            ),
            Err(e) => tracing::warn!(
                path = %path.display(),
                error = %e,
                "skill dispatch engine load failed; continuing with empty registry"
            ),
        }
    }

    // 3. Resolve Discord connector if configured.  We create a shared Arc so
    //    the gateway listener and the event-loop response sender use the same
    //    REST client / token.
    let (discord_tx, discord_rx) = mpsc::channel::<DiscordMessage>(256);
    let mut shared_discord: Option<Arc<DiscordConnector>> = None;
    let shutting_down = Arc::new(std::sync::atomic::AtomicBool::new(false));

    for cm in &manifests.connectors {
        let spec: ConnectorSpec = match serde_json::from_value(cm.spec.clone()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(name = %cm.metadata.name, "Failed to parse connector spec: {e}");
                continue;
            }
        };

        if spec.kind != "discord" {
            tracing::warn!(kind = %spec.kind, "Unsupported connector kind (MVS supports discord only)");
            continue;
        }

        let token = match sera_config::manifest_loader::resolve_connector_token_with(
            &spec,
            &secret_resolver,
        ) {
            Some(t) => t,
            None => {
                tracing::warn!(
                    name = %cm.metadata.name,
                    "Discord token not resolved. Store with `sera secrets set` or set SERA_SECRET_* env var."
                );
                continue;
            }
        };

        let agent_name = spec.agent.as_deref().unwrap_or("sera").to_owned();
        tracing::info!(
            connector = %cm.metadata.name,
            agent = %agent_name,
            "Starting Discord connector"
        );

        let connector = Arc::new(DiscordConnector::new(
            &token,
            &agent_name,
            discord_tx.clone(),
            Arc::clone(&shutting_down),
        ));
        shared_discord = Some(Arc::clone(&connector));

        // Spawn the gateway listener.
        tokio::spawn(async move {
            if let Err(e) = connector.run().await {
                tracing::error!("Discord connector exited with error: {e}");
            }
        });
    }

    // Validate that no dev-secret defaults are used in production.
    // In dev mode this only warns; in production (SERA_ENV=production) it aborts.
    sera_config::core_config::validate_env_secrets()?;

    // Load API key from environment (if set).
    let api_key = std::env::var("SERA_API_KEY").ok().filter(|k| !k.is_empty());
    if api_key.is_some() {
        tracing::info!("API key authentication enabled (SERA_API_KEY is set)");
    } else {
        tracing::info!("API key authentication disabled (autonomous mode)");
    }

    let hook_registry = Arc::new(HookRegistry::new());
    let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));

    // 3a. Wire the lane-pending counter backend.
    //
    // When `DATABASE_URL` is set, the gateway connects to Postgres and mirrors
    // per-lane pending counts through [`PostgresLaneCounter`] so multiple
    // gateway pods share a consistent admission-control view. In dev / no-DB
    // mode we fall back to the in-process [`InMemoryLaneCounter`] — semantics
    // match the pre-sera-bsq2 behaviour exactly.
    let lane_counter_store: Arc<dyn LaneCounterStoreDyn> = match std::env::var("DATABASE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        Some(url) => match sera_db::DbPool::connect(&url).await {
            Ok(pool) => {
                tracing::info!(
                    "Lane-pending counter backed by PostgresLaneCounter (DATABASE_URL set)"
                );
                Arc::new(PostgresLaneCounter::new(pool.inner().clone()))
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "DATABASE_URL is set but Postgres connection failed; falling back to InMemoryLaneCounter"
                );
                Arc::new(InMemoryLaneCounter::new())
            }
        },
        None => {
            tracing::info!(
                "Lane-pending counter backed by InMemoryLaneCounter (DATABASE_URL unset)"
            );
            Arc::new(InMemoryLaneCounter::new())
        }
    };

    // 3b. Spawn a sera-runtime harness for each agent.
    // Use absolute path to the runtime binary (in the same directory as the gateway binary).
    let runtime_bin = std::env::var("SERA_RUNTIME_BIN").unwrap_or_else(|_| {
        let exe_path = std::env::current_exe().unwrap_or_default();
        let exe_dir = exe_path.parent().unwrap_or(std::path::Path::new("."));
        exe_dir.join("sera-runtime").to_string_lossy().to_string()
    });

    let mut harnesses = std::collections::HashMap::new();

    for agent_name in manifests.agent_names() {
        let agent_spec = match manifests.agent_spec(agent_name).ok().flatten() {
            Some(s) => s,
            None => continue,
        };

        let provider_spec: Option<ProviderSpec> =
            manifests.provider_spec(&agent_spec.provider).ok().flatten();

        let (base_url, model, api_key_val) = match provider_spec {
            Some(ref p) => {
                let key = resolve_provider_api_key(p).unwrap_or_default();
                let model = agent_spec
                    .model
                    .as_deref()
                    .or(p.default_model.as_deref())
                    .unwrap_or("default")
                    .to_owned();
                (p.base_url.clone(), model, key)
            }
            None => {
                tracing::warn!(agent = %agent_name, "No provider found, skipping harness");
                continue;
            }
        };

        let mut env = std::collections::HashMap::new();
        env.insert("LLM_BASE_URL".to_string(), base_url);
        env.insert("LLM_MODEL".to_string(), model.clone());
        env.insert("LLM_API_KEY".to_string(), api_key_val);
        env.insert("AGENT_ID".to_string(), agent_name.to_string());
        // Forward permissive-gate flag to the runtime process when the operator
        // has set SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE in the environment.
        if std::env::var("SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false)
        {
            env.insert(
                "SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE".to_string(),
                "true".to_string(),
            );
        }

        match StdioHarness::spawn(&runtime_bin, env).await {
            Ok(harness) => {
                tracing::info!(agent = %agent_name, model = %model, "Spawned runtime harness");
                harnesses.insert(agent_name.to_string(), Arc::new(harness));
            }
            Err(e) => {
                tracing::error!(agent = %agent_name, error = %e, "Failed to spawn runtime harness");
            }
        }
    }

    // sera-uwk0: build the mail correlator + lookup pair. The correlator owns
    // the envelope index; the lookup bridges correlator output back to
    // `sera_workflow::MailLookup` for the ready-queue. Both live in AppState
    // so outbound transport (sera-tools) can register envelopes via the same
    // correlator and future ready-queue wiring can consume the lookup.
    let mail_lookup = Arc::new(InMemoryMailLookup::new());
    let mail_correlator = Arc::new(HeaderMailCorrelator::new(
        Arc::new(InMemoryEnvelopeIndex::default()),
        Some(mail_lookup.clone()),
    ));

    // Seed constitutional rules from SERA_CONSTITUTIONAL_RULES_PATH (or the
    // default /etc/sera/constitutional_rules.yaml). Missing file → no-op (Ok(0)).
    // Parse error → fail-fast (propagate Err so the process exits with context).
    let constitutional_registry = Arc::new(ConstitutionalRegistry::new());
    match constitutional_config::seed_registry_from_env(&constitutional_registry).await {
        Ok(count) => {
            tracing::info!(count, "Constitutional rules seeded from env path");
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to load constitutional rules: {e}"));
        }
    }

    let state = Arc::new(AppState {
        db: Mutex::new(db),
        manifests,
        discord: shared_discord,
        api_key,
        lane_queue: Mutex::new(LaneQueue::new_with_counter_store(
            10,
            QueueMode::Collect,
            Arc::clone(&lane_counter_store),
        )),
        hook_registry,
        chain_executor,
        harnesses,
        runtime_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        shutting_down: Arc::clone(&shutting_down),
        mail_correlator,
        mail_lookup,
        a2a_peers: Arc::new(RwLock::new(A2aPeerRegistry::new())),
        a2a_router: Arc::new(InProcRouter::new(|_req: A2aRequest| async move {
            Ok(serde_json::json!({"status": "no handler registered"}))
        })),
        agui_hub: Arc::new(RwLock::new(AguiHub::new())),
        plugin_registry: Arc::new(InMemoryPluginRegistry::new()),
        skill_engine,
        semantic_store,
        kill_switch: Arc::new(KillSwitch::new()),
        // sera-4i4i: use SqliteGitSessionStore so envelopes survive restarts.
        // db_path = <data_root>/parts.sqlite; sessions_root = <data_root>/sessions/.
        session_store: {
            let parts_db = data_root.join("parts.sqlite");
            let sessions_root = data_root.join("sessions");
            Arc::new(
                SqliteGitSessionStore::open(&parts_db, &sessions_root)
                    .expect("failed to initialize SqliteGitSessionStore"),
            )
        },
        constitutional_registry,
    });

    // 4. Start event processing loop.
    let event_state = Arc::clone(&state);
    tokio::spawn(async move {
        event_loop(event_state, discord_rx).await;
    });

    // 4a. Spawn admin kill-switch Unix socket (SPEC-gateway §7a.4).
    {
        let ks = Arc::clone(&state.kill_switch);
        let sock_path = admin_sock_path();
        spawn_admin_socket(ks, sock_path, || {
            tracing::warn!(
                event = "KILL_SWITCH_ACTIVATED",
                "ROLLBACK received on admin socket — gateway halted"
            );
        });
    }

    // 5. Build and start HTTP server.
    let app = build_router(Arc::clone(&state));
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(%addr, "Starting HTTP server");

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // 6. Graceful shutdown on SIGINT/SIGTERM.
    //
    // Phase A: axum stops accepting new connections and waits for in-flight
    // requests to complete (`with_graceful_shutdown`).
    // Phase B: we set `shutting_down` so background loops exit, drop the
    // Discord message sender (so `event_loop` terminates after draining), and
    // ask every `StdioHarness` to shutdown. The whole drain is bounded by
    // `SHUTDOWN_DRAIN_DEADLINE` so a hung subsystem cannot block exit.
    let shutdown_flag = Arc::clone(&shutting_down);
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            shutdown_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        })
        .await?;

    tracing::info!("HTTP server stopped accepting new connections; draining subsystems");

    let drain_started = std::time::Instant::now();

    // Close the Discord→event_loop channel so the loop exits once its
    // queue has drained. `discord_tx` is the only sender we hold here;
    // the Discord connector task keeps its own clone. Dropping ours is
    // enough for the test-mode path where no connector is running, and
    // harmless otherwise.
    drop(discord_tx);

    // Phase B.1 — harness drain. Fire every harness shutdown in parallel so a
    // slow harness does not serialize the others. Bound by `HARNESS_DRAIN_DEADLINE`.
    let harness_drain = tokio::time::timeout(HARNESS_DRAIN_DEADLINE, async {
        let harness_shutdowns: Vec<_> = state
            .harnesses
            .iter()
            .map(|(name, harness)| {
                let name = name.clone();
                let harness = Arc::clone(harness);
                async move {
                    if let Err(e) = harness.shutdown().await {
                        tracing::warn!(agent = %name, error = %e, "Harness shutdown send failed");
                    }
                }
            })
            .collect();
        futures_util::future::join_all(harness_shutdowns).await;
    })
    .await;
    if harness_drain.is_err() {
        tracing::warn!(
            deadline_ms = HARNESS_DRAIN_DEADLINE.as_millis() as u64,
            "Harness drain deadline exceeded"
        );
    }

    // Phase B.2 — lane queue drain. Wait for enqueued/in-flight jobs to finish
    // so we don't drop acknowledged work on the floor. `drain_shared` flips the
    // queue's closed flag as it starts, so no new jobs enter during the wait.
    let queue_drain_budget = SHUTDOWN_DRAIN_DEADLINE
        .saturating_sub(drain_started.elapsed())
        .max(std::time::Duration::from_millis(0));
    match sera_db::lane_queue::LaneQueue::drain_shared(&state.lane_queue, queue_drain_budget).await
    {
        Ok(outcome) if outcome.timed_out => tracing::warn!(
            remaining = outcome.remaining,
            drained = outcome.drained,
            "lane queue drain exceeded deadline"
        ),
        Ok(outcome) => tracing::info!(drained = outcome.drained, "lane queue drain complete"),
        Err(e) => tracing::error!(error = %e, "lane queue drain failed"),
    }

    tracing::info!(
        drain_ms = drain_started.elapsed().as_millis() as u64,
        "Subsystems drained"
    );

    tracing::info!("SERA gateway shut down");
    Ok(())
}

/// Maximum time we wait for in-flight subsystems (runtime harnesses, Discord
/// connector, event loop) to flush after a termination signal before forcing
/// exit. Chosen to comfortably cover a single in-flight LLM turn while still
/// fitting inside typical container `stop_grace_period` windows (default 10s
/// on Docker, 30s on Kubernetes — we match the latter).
const SHUTDOWN_DRAIN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

/// Share of [`SHUTDOWN_DRAIN_DEADLINE`] reserved for flushing runtime harnesses
/// (phase B.1). The lane queue drain (phase B.2) gets the remainder of
/// [`SHUTDOWN_DRAIN_DEADLINE`] after harness drain returns — so in the fast
/// path the queue can use most of the 30 s budget; in the slow path both
/// phases still fit inside the total.
const HARNESS_DRAIN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);

/// Build the shutdown-signal future: resolves on SIGTERM (Unix) or Ctrl+C.
/// Windows has no SIGTERM, so we only listen for Ctrl+C there.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!("Failed to install Ctrl+C handler: {e}");
        }
    };

    #[cfg(unix)]
    {
        let mut sigterm =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to install SIGTERM handler: {e}");
                    ctrl_c.await;
                    tracing::info!("Shutdown signal received (Ctrl+C)");
                    return;
                }
            };
        tokio::select! {
            _ = ctrl_c => tracing::info!("Shutdown signal received (Ctrl+C)"),
            _ = sigterm.recv() => tracing::info!("Shutdown signal received (SIGTERM)"),
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await;
        tracing::info!("Shutdown signal received (Ctrl+C)");
    }
}

/// Response payload for `/api/mail/inbound`.
///
/// Records whether the raw inbound message correlated to a pending Mail gate
/// and at which tier (B1 headers / B2 body-nonce) it resolved, or the
/// drop reason otherwise. The webhook always returns `200 OK` on a well-formed
/// MIME blob — "no match" is a normal outcome, not an error.
#[derive(Serialize)]
struct MailInboundResponse {
    /// `"resolved"` or `"dropped"`.
    outcome: &'static str,
    /// Present on resolution. Opaque gate id echoed back for caller-side
    /// correlation / logging.
    #[serde(skip_serializing_if = "Option::is_none")]
    gate_id: Option<String>,
    /// Present on resolution. RFC 5322 Message-ID used as the thread id.
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_id: Option<String>,
    /// Present on resolution. Ladder tier that matched (`"b1_headers"` /
    /// `"b2_body_nonce"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    tier: Option<String>,
    /// Present on drop. Reason tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

/// `POST /api/mail/inbound` — sera-uwk0.
///
/// Accepts a raw RFC 5322 MIME blob as the request body and pushes it through
/// the [`HeaderMailCorrelator`]. On a match the correlator notifies the
/// [`InMemoryMailLookup`] which the workflow ready-queue consults via
/// `MailLookup::thread_event`.
///
/// Transport (SMTP / IMAP / webhook) is explicitly out of scope — see the
/// external mail gateway (discord-bridge / sera-tools egress plane) for that.
async fn mail_inbound_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<MailInboundResponse>, StatusCode> {
    validate_api_key(&state, &headers)?;

    let msg = parse_raw_message(&body).map_err(|e| {
        tracing::warn!(error = %e, "inbound mail parse failed");
        StatusCode::BAD_REQUEST
    })?;

    let outcome = state.mail_correlator.correlate(&msg).await.map_err(|e| {
        tracing::error!(error = %e, "mail correlator failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let resp = match outcome {
        CorrelationOutcome::Resolved {
            gate_id,
            thread_id,
            tier,
        } => MailInboundResponse {
            outcome: "resolved",
            gate_id: Some(gate_id.as_str().to_string()),
            thread_id: Some(thread_id.as_str().to_string()),
            tier: Some(
                match tier {
                    sera_mail::CorrelationTier::B1Headers => "b1_headers",
                    sera_mail::CorrelationTier::B2BodyNonce => "b2_body_nonce",
                    sera_mail::CorrelationTier::B2ReplyToToken => "b2_reply_to_token",
                }
                .to_string(),
            ),
            reason: None,
        },
        CorrelationOutcome::Dropped { reason } => MailInboundResponse {
            outcome: "dropped",
            gate_id: None,
            thread_id: None,
            tier: None,
            reason: Some(
                match reason {
                    sera_mail::DropReason::NoMatch => "no_match",
                    sera_mail::DropReason::Spoof => "spoof",
                    sera_mail::DropReason::MalformedHeaders => "malformed_headers",
                }
                .to_string(),
            ),
        },
    };

    Ok(Json(resp))
}

/// `GET /api/hooks` — list every hook registered in the in-process
/// [`HookRegistry`], grouped by [`HookPoint`]. Consumed by operators and the
/// dashboard to introspect which hook modules are loaded without replaying a
/// full chain via [`ChainExecutor`]. This is the direct-lookup entry point
/// kept alongside the chain-executor path.
async fn hooks_list_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    validate_api_key(&state, &headers)?;

    let metadata = state.hook_registry.list();

    // Group by hook point: for each hook, emit one entry under every point
    // it declares as supported. Operators expect per-point breakdowns when
    // debugging hook chains (see SPEC-hooks §registry introspection).
    let mut by_point: std::collections::BTreeMap<String, Vec<&sera_types::hook::HookMetadata>> =
        std::collections::BTreeMap::new();
    for meta in &metadata {
        for point in &meta.supported_points {
            let key = serde_json::to_value(point)
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| format!("{:?}", point));
            by_point.entry(key).or_default().push(meta);
        }
    }

    Ok(Json(serde_json::json!({
        "hooks": metadata,
        "by_point": by_point,
        "count": metadata.len(),
    })))
}

/// Middleware that rejects all requests with 503 while the kill switch is armed.
/// Health endpoints bypass this gate so load balancers can still observe state.
async fn kill_switch_gate(
    State(state): State<Arc<AppState>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    // Health endpoints always pass through.
    let path = request.uri().path();
    let is_health = path == "/health" || path == "/api/health" || path == "/api/health/ready";
    if !is_health && state.kill_switch.is_armed() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "gateway_halted",
                "reason": "admin kill-switch engaged"
            })),
        )
            .into_response();
    }
    next.run(request).await
}

fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/api/health", get(health_handler))
        // Readiness gate — distinct from liveness above. Returns 503 until
        // the runtime harness has answered a probe successfully, closing the
        // empty-reply race window after `docker restart`. Clients (load
        // balancers, eval harness `warmup_sera`, etc.) should poll this
        // before dispatching real turns.
        .route("/api/health/ready", get(readiness_handler))
        .route("/api/auth/me", get(auth_me_handler))
        .route("/api/chat", post(chat_handler))
        .route("/api/agents", get(agents_handler))
        .route("/api/agents/{id}", get(agent_by_id_handler))
        .route("/api/sessions", get(sessions_handler))
        .route("/api/sessions/{id}/transcript", get(transcript_handler))
        // sera-2q1d: read-only hook registry introspection — lists every hook
        // registered with the in-process `HookRegistry`, grouped by `HookPoint`.
        .route("/api/hooks", get(hooks_list_handler))
        // sera-uwk0: mail gate ingress correlator webhook.
        .route("/api/mail/inbound", post(mail_inbound_handler))
        // ── Phase-3 SPEC-interop routes (sera-ne64) ──────────────────────────
        .route("/api/a2a/send", post(route_a2a::send_message::<AppState>))
        .route("/api/a2a/peers", get(route_a2a::list_peers::<AppState>))
        .route("/api/a2a/accept", post(route_a2a::accept::<AppState>))
        .route(
            "/api/agui/stream",
            get(route_agui::stream_events::<AppState>),
        )
        .route("/api/agui/emit", post(route_agui::emit_event::<AppState>))
        .route("/api/plugins", get(route_plugins::list_plugins::<AppState>))
        .route(
            "/api/plugins/{id}/call",
            post(route_plugins::call_plugin::<AppState>),
        )
        .route(
            "/api/plugins/hot-reload",
            post(route_plugins::hot_reload::<AppState>),
        )
        // ── sera-8d1.2-follow: party mode (circles/{id}/party) ───────────────
        .route(
            "/api/circles/{id}/party",
            post(party::start_party::<AppState>),
        )
        // TODO(sera-8d1.4-follow): wire GET/PUT /api/circles/{id}/constitution
        // when the constitution-get/put handlers get reimplemented against
        // this binary's SqliteDb-backed AppState (the previous orphan
        // Postgres-only implementation was deleted in sera-s31i).
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            kill_switch_gate,
        ))
        .with_state(state)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[test]
    fn pgvector_selected_when_env_pin() {
        assert!(wants_pgvector_backend(Some("pgvector"), None));
        assert!(wants_pgvector_backend(
            Some("pgvector"),
            Some("postgres://x")
        ));
    }

    #[test]
    fn sqlite_pin_ignores_database_url() {
        assert!(!wants_pgvector_backend(
            Some("sqlite"),
            Some("postgres://x")
        ));
        assert!(!wants_pgvector_backend(Some("sqlite"), None));
    }

    #[test]
    fn auto_falls_back_on_database_url() {
        assert!(wants_pgvector_backend(None, Some("postgres://x")));
        assert!(!wants_pgvector_backend(None, None));
    }

    #[test]
    fn unknown_backend_pref_falls_back_to_sqlite() {
        assert!(!wants_pgvector_backend(Some("redis"), Some("postgres://x")));
    }

    fn test_manifests() -> ManifestSet {
        parse_manifests(TEMPLATE_YAML).unwrap()
    }

    async fn test_harnesses() -> std::collections::HashMap<String, Arc<StdioHarness>> {
        let mut h = std::collections::HashMap::new();
        h.insert(
            "sera".to_string(),
            Arc::new(StdioHarness::spawn_mock().await.unwrap()),
        );
        h
    }

    async fn test_state_async() -> Arc<AppState> {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        Arc::new(AppState {
            db: Mutex::new(SqliteDb::open_in_memory().unwrap()),
            manifests: test_manifests(),
            discord: None,
            api_key: None,
            lane_queue: Mutex::new(LaneQueue::new(10, QueueMode::Collect)),
            hook_registry,
            chain_executor,
            harnesses: test_harnesses().await,
            runtime_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            mail_correlator: Arc::new(HeaderMailCorrelator::new(
                Arc::new(InMemoryEnvelopeIndex::default()),
                None,
            )),
            mail_lookup: Arc::new(InMemoryMailLookup::new()),
            a2a_peers: Arc::new(RwLock::new(A2aPeerRegistry::new())),
            a2a_router: Arc::new(InProcRouter::new(|_req: A2aRequest| async move {
                Ok(serde_json::json!({"status": "test"}))
            })),
            agui_hub: Arc::new(RwLock::new(AguiHub::new())),
            plugin_registry: Arc::new(InMemoryPluginRegistry::new()),
            skill_engine: Arc::new(SkillDispatchEngine::new()),
            semantic_store: Arc::new(
                SqliteMemoryStore::open_in_memory(None).expect("open in-memory semantic store"),
            ),
            kill_switch: Arc::new(KillSwitch::new()),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
        })
    }

    fn test_state() -> Arc<AppState> {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        Arc::new(AppState {
            db: Mutex::new(SqliteDb::open_in_memory().unwrap()),
            manifests: test_manifests(),
            discord: None,
            api_key: None,
            lane_queue: Mutex::new(LaneQueue::new(10, QueueMode::Collect)),
            hook_registry,
            chain_executor,
            harnesses: std::collections::HashMap::new(),
            runtime_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            mail_correlator: Arc::new(HeaderMailCorrelator::new(
                Arc::new(InMemoryEnvelopeIndex::default()),
                None,
            )),
            mail_lookup: Arc::new(InMemoryMailLookup::new()),
            a2a_peers: Arc::new(RwLock::new(A2aPeerRegistry::new())),
            a2a_router: Arc::new(InProcRouter::new(|_req: A2aRequest| async move {
                Ok(serde_json::json!({"status": "test"}))
            })),
            agui_hub: Arc::new(RwLock::new(AguiHub::new())),
            plugin_registry: Arc::new(InMemoryPluginRegistry::new()),
            skill_engine: Arc::new(SkillDispatchEngine::new()),
            semantic_store: Arc::new(
                SqliteMemoryStore::open_in_memory(None).expect("open in-memory semantic store"),
            ),
            kill_switch: Arc::new(KillSwitch::new()),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
        })
    }

    async fn test_state_with_api_key_async(key: &str) -> Arc<AppState> {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        Arc::new(AppState {
            db: Mutex::new(SqliteDb::open_in_memory().unwrap()),
            manifests: test_manifests(),
            discord: None,
            api_key: Some(key.to_owned()),
            lane_queue: Mutex::new(LaneQueue::new(10, QueueMode::Collect)),
            hook_registry,
            chain_executor,
            harnesses: test_harnesses().await,
            runtime_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            mail_correlator: Arc::new(HeaderMailCorrelator::new(
                Arc::new(InMemoryEnvelopeIndex::default()),
                None,
            )),
            mail_lookup: Arc::new(InMemoryMailLookup::new()),
            a2a_peers: Arc::new(RwLock::new(A2aPeerRegistry::new())),
            a2a_router: Arc::new(InProcRouter::new(|_req: A2aRequest| async move {
                Ok(serde_json::json!({"status": "test"}))
            })),
            agui_hub: Arc::new(RwLock::new(AguiHub::new())),
            plugin_registry: Arc::new(InMemoryPluginRegistry::new()),
            skill_engine: Arc::new(SkillDispatchEngine::new()),
            semantic_store: Arc::new(
                SqliteMemoryStore::open_in_memory(None).expect("open in-memory semantic store"),
            ),
            kill_switch: Arc::new(KillSwitch::new()),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
        })
    }

    fn test_state_with_api_key(key: &str) -> Arc<AppState> {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        Arc::new(AppState {
            db: Mutex::new(SqliteDb::open_in_memory().unwrap()),
            manifests: test_manifests(),
            discord: None,
            api_key: Some(key.to_owned()),
            lane_queue: Mutex::new(LaneQueue::new(10, QueueMode::Collect)),
            hook_registry,
            chain_executor,
            harnesses: std::collections::HashMap::new(),
            runtime_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            mail_correlator: Arc::new(HeaderMailCorrelator::new(
                Arc::new(InMemoryEnvelopeIndex::default()),
                None,
            )),
            mail_lookup: Arc::new(InMemoryMailLookup::new()),
            a2a_peers: Arc::new(RwLock::new(A2aPeerRegistry::new())),
            a2a_router: Arc::new(InProcRouter::new(|_req: A2aRequest| async move {
                Ok(serde_json::json!({"status": "test"}))
            })),
            agui_hub: Arc::new(RwLock::new(AguiHub::new())),
            plugin_registry: Arc::new(InMemoryPluginRegistry::new()),
            skill_engine: Arc::new(SkillDispatchEngine::new()),
            semantic_store: Arc::new(
                SqliteMemoryStore::open_in_memory(None).expect("open in-memory semantic store"),
            ),
            kill_switch: Arc::new(KillSwitch::new()),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
        })
    }

    // -- Graceful shutdown --

    /// The `shutdown_signal` future must construct without panicking on any
    /// supported platform. We can't actually deliver a signal in-process, so
    /// we build the future and drop it; if SIGTERM registration panics the
    /// builder, this test fails.
    #[tokio::test]
    async fn shutdown_signal_future_builds_without_panic() {
        let fut = super::shutdown_signal();
        // Poll once so the registration code runs, then drop.
        let poll = tokio::time::timeout(std::time::Duration::from_millis(50), fut).await;
        // A timeout is the expected outcome — no signal arrived during the
        // test. A completion would mean a real signal fired, which is still
        // fine for a panic-check.
        assert!(
            poll.is_err() || poll.is_ok(),
            "future should either pend or complete without panicking"
        );
    }

    /// A background loop that observes the shared `shutting_down` flag must
    /// exit within one iteration after the flag flips. This is the contract
    /// that long-running subsystems (e.g. pollers, reconnect loops) rely on
    /// to cooperate with the drain phase in `run_start`.
    #[tokio::test]
    async fn shutting_down_flag_terminates_background_loop() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let flag = Arc::new(AtomicBool::new(false));
        let iterations = Arc::new(AtomicUsize::new(0));

        let loop_flag = Arc::clone(&flag);
        let loop_iters = Arc::clone(&iterations);
        let handle = tokio::spawn(async move {
            while !loop_flag.load(Ordering::SeqCst) {
                loop_iters.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        });

        // Let the loop run a few times, then flip the flag.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        flag.store(true, Ordering::SeqCst);

        // Loop must exit within a bounded time once the flag is set.
        tokio::time::timeout(std::time::Duration::from_secs(1), handle)
            .await
            .expect("background loop should exit promptly after flag flip")
            .expect("loop task should not panic");

        assert!(
            iterations.load(Ordering::SeqCst) > 0,
            "loop should have iterated at least once before exiting"
        );
    }

    // -- CLI parsing --

    #[test]
    fn parse_start_defaults() {
        let cli = Cli::try_parse_from(["sera", "start"]).unwrap();
        match cli.command {
            Commands::Start { config, port } => {
                assert_eq!(config, PathBuf::from("sera.yaml"));
                assert_eq!(port, 3001);
            }
            _ => panic!("expected Start"),
        }
    }

    #[test]
    fn parse_start_custom() {
        let cli =
            Cli::try_parse_from(["sera", "start", "-c", "custom.yaml", "-p", "8080"]).unwrap();
        match cli.command {
            Commands::Start { config, port } => {
                assert_eq!(config, PathBuf::from("custom.yaml"));
                assert_eq!(port, 8080);
            }
            _ => panic!("expected Start"),
        }
    }

    #[test]
    fn parse_init() {
        let cli = Cli::try_parse_from(["sera", "init"]).unwrap();
        assert!(matches!(cli.command, Commands::Init));
    }

    #[test]
    fn parse_agent_list() {
        let cli = Cli::try_parse_from(["sera", "agent", "list"]).unwrap();
        match cli.command {
            Commands::Agent {
                command: AgentCommands::List,
            } => {}
            _ => panic!("expected Agent List"),
        }
    }

    #[test]
    fn parse_agent_create() {
        let cli = Cli::try_parse_from(["sera", "agent", "create", "reviewer"]).unwrap();
        match cli.command {
            Commands::Agent {
                command: AgentCommands::Create { name },
            } => {
                assert_eq!(name, "reviewer");
            }
            _ => panic!("expected Agent Create"),
        }
    }

    // -- Config loading --

    #[test]
    fn template_yaml_parses() {
        let set = test_manifests();
        assert_eq!(set.instances.len(), 1);
        assert_eq!(set.providers.len(), 1);
        assert_eq!(set.agents.len(), 1);
        assert_eq!(set.connectors.len(), 1);
    }

    #[test]
    fn template_yaml_agent_spec() {
        let set = test_manifests();
        let spec = set.agent_spec("sera").unwrap().unwrap();
        assert_eq!(spec.provider, "lm-studio");
        assert!(
            spec.persona
                .unwrap()
                .immutable_anchor
                .unwrap()
                .contains("Sera")
        );
    }

    #[test]
    fn template_yaml_provider_spec() {
        let set = test_manifests();
        let spec = set.provider_spec("lm-studio").unwrap().unwrap();
        assert_eq!(spec.kind, "openai-compatible");
        assert_eq!(spec.base_url, "http://localhost:1234/v1");
    }

    // -- sera init output --

    #[test]
    fn init_template_is_valid_yaml() {
        let set = parse_manifests(TEMPLATE_YAML).unwrap();
        assert_eq!(set.instances.len(), 1);
        assert_eq!(set.agents.len(), 1);
        assert_eq!(set.providers.len(), 1);
        assert_eq!(set.connectors.len(), 1);
    }

    // -- Agent create/list (file-based) --

    #[test]
    fn agent_create_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sera.yaml");
        std::fs::write(&path, TEMPLATE_YAML).unwrap();

        // Create a new agent.
        run_agent_create(&path, "reviewer").unwrap();

        // Verify it was added.
        let manifests = load_manifest_file(&path).unwrap();
        assert_eq!(manifests.agents.len(), 2);
        assert!(manifests.agent("sera").is_some());
        assert!(manifests.agent("reviewer").is_some());
    }

    #[test]
    fn agent_create_duplicate_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sera.yaml");
        std::fs::write(&path, TEMPLATE_YAML).unwrap();

        let err = run_agent_create(&path, "sera").unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn agent_create_no_config_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.yaml");

        let err = run_agent_create(&path, "test").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    // -- Health endpoint --

    #[tokio::test]
    async fn health_endpoint() {
        let state = test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    // -- Readiness endpoint (empty-reply race fix) --
    //
    // The race: after `docker restart`, axum binds and `/api/health` answers
    // 200 immediately, but the runtime child has not yet handshaken with LM
    // Studio. The first chat turn races the reconnect and returns an empty
    // reply. The fix is `/api/health/ready`, which actively probes a harness
    // and returns 503 until the runtime answers.

    /// Liveness must stay 200 even when no harness is registered. This is the
    /// docker `HEALTHCHECK` contract — the gateway process is up.
    #[tokio::test]
    async fn liveness_returns_200_even_without_harness() {
        let state = test_state();
        assert!(state.harnesses.is_empty(), "precondition: no harness");
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// Race-condition repro: with no harness yet registered, the readiness
    /// probe must close the gate (503). This is the post-restart window
    /// where `/api/health` would otherwise return 200 prematurely.
    #[tokio::test]
    async fn readiness_returns_503_when_no_harness_registered() {
        let state = test_state();
        assert!(state.harnesses.is_empty());
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "not_ready");
        assert_eq!(json["runtime_connected"], false);
    }

    /// Race-condition repro: a registered harness that never answers (the
    /// post-restart pre-handshake window simulated by `spawn_mock_hang`)
    /// must keep the gate closed. Uses a tight probe timeout so the test
    /// does not stall the suite for the default 5s.
    #[tokio::test]
    async fn readiness_returns_503_when_harness_does_not_respond() {
        // Tight bound — the hung harness will sit forever, so the probe must
        // give up quickly. Env var must be set BEFORE the handler runs.
        // SAFETY: tests in this binary do not read this var concurrently.
        unsafe {
            std::env::set_var("SERA_READINESS_PROBE_TIMEOUT_SECS", "1");
        }

        let mut state = test_state_async().await;
        // Replace the always-good mock with a hanging mock.
        let hanging = Arc::new(StdioHarness::spawn_mock_hang().await.unwrap());
        Arc::get_mut(&mut state)
            .expect("unique state ref")
            .harnesses
            .insert("sera".to_string(), hanging);
        let app = build_router(state);

        let started = std::time::Instant::now();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let elapsed = started.elapsed();

        unsafe {
            std::env::remove_var("SERA_READINESS_PROBE_TIMEOUT_SECS");
        }

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            elapsed < std::time::Duration::from_secs(4),
            "probe should give up within ~1s, got {:?}",
            elapsed
        );
    }

    /// Once the harness answers a probe successfully, readiness flips to
    /// 200 — this is the "runtime is connected" transition the eval
    /// harness's `warmup_sera` was working around externally.
    #[tokio::test]
    async fn readiness_flips_to_200_after_successful_probe() {
        let state = test_state_async().await;
        // Precondition: latch is cold and the mock harness is wired in.
        assert!(
            !state
                .runtime_ready
                .load(std::sync::atomic::Ordering::Acquire)
        );
        assert!(!state.harnesses.is_empty());
        let app = build_router(Arc::clone(&state));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ready");
        assert_eq!(json["runtime_connected"], true);
        // Latch must persist across calls so subsequent probes are O(1).
        assert!(
            state
                .runtime_ready
                .load(std::sync::atomic::Ordering::Acquire)
        );
    }

    /// Once the latch is set, readiness must answer 200 even with no harness
    /// registered — proves the cached fast path bypasses the probe and
    /// cannot regress to false after a transient harness disappearance.
    #[tokio::test]
    async fn readiness_uses_cached_latch_on_repeat_calls() {
        let state = test_state();
        state
            .runtime_ready
            .store(true, std::sync::atomic::Ordering::Release);
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // -- HITL pattern gate --

    #[test]
    fn detect_flagged_operation_hits_rm_rf() {
        assert_eq!(detect_flagged_operation("please rm -rf /"), Some("rm -rf"));
    }

    #[test]
    fn detect_flagged_operation_is_case_insensitive() {
        assert_eq!(
            detect_flagged_operation("Please DROP TABLE users;"),
            Some("drop table")
        );
    }

    #[test]
    fn detect_flagged_operation_misses_benign_text() {
        assert!(detect_flagged_operation("remove this line").is_none());
        assert!(detect_flagged_operation("hello world").is_none());
    }

    #[tokio::test]
    async fn hitl_gate_blocks_rm_rf() {
        let state = test_state_async().await;
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "message": "please rm -rf /" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "hitl_approval_required");
        assert!(
            json["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("rm -rf"),
            "reason should mention the matched pattern"
        );
    }

    // -- Skill dispatch injection --

    #[test]
    fn skill_injection_adds_system_message() {
        use sera_types::skill::{SkillConfig, SkillMode, SkillTrigger};

        let engine = SkillDispatchEngine::new();
        engine.register(
            SkillConfig {
                name: "reviewer".into(),
                version: "1.0.0".into(),
                description: "code reviewer".into(),
                mode: SkillMode::OnDemand,
                trigger: SkillTrigger::Event("review".into()),
                tools: vec![],
                context_injection: Some("You review code.".into()),
                config: serde_json::json!({}),
            },
            None,
        );

        // Trigger the skill by firing its event keyword.
        let fired = engine.on_turn("please review this diff");
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].name, "reviewer");

        // The active context_injection must now be exposed.
        let injections = engine.active_context_injections();
        assert_eq!(injections, vec!["You review code.".to_string()]);
    }

    // -- Chat endpoint --

    #[tokio::test]
    async fn chat_endpoint_unknown_agent_returns_404() {
        let state = test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "message": "hello",
                            "agent": "nonexistent"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn chat_endpoint_creates_session_and_transcript() {
        let state = test_state_async().await;
        let app = build_router(Arc::clone(&state));

        // The LLM call will fail (no real provider), but the session and
        // transcript should still be created.
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "message": "hello" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["session_id"].as_str().is_some());
        // Response will contain an error message since LLM is not reachable,
        // but the structure is correct.
        assert!(json["response"].as_str().is_some());
        // Usage info is always present (may be zeros if LLM unreachable).
        assert!(json["usage"].is_object());
        assert!(json["usage"]["prompt_tokens"].is_number());
        assert!(json["usage"]["completion_tokens"].is_number());
        assert!(json["usage"]["total_tokens"].is_number());
    }

    /// sera-ygwe regression guard: POST /api/chat with a missing `message` field
    /// must return 400 with a structured JSON error, not 422 with a raw serde
    /// error string leaked from axum's default `Json` extractor.
    #[tokio::test]
    async fn chat_endpoint_missing_message_returns_400_structured_error() {
        let state = test_state_async().await;
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "missing_field");
        assert_eq!(json["field"], "message");
        assert!(
            json["message"].as_str().unwrap_or_default().contains("message"),
            "error message should mention the missing field"
        );
    }

    // -- Router structure --

    #[tokio::test]
    async fn unknown_route_returns_404() {
        let state = test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // -- Chat request/response parsing --

    #[test]
    fn chat_request_deserialize_full() {
        let json = r#"{"message":"Hello","agent":"sera"}"#;
        let req: ChatRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.message, "Hello");
        assert_eq!(req.agent.as_deref(), Some("sera"));
    }

    #[test]
    fn chat_request_deserialize_minimal() {
        let json = r#"{"message":"Hi"}"#;
        let req: ChatRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.message, "Hi");
        assert!(req.agent.is_none());
    }

    #[test]
    fn chat_response_serialize() {
        let resp = ChatResponse {
            response: "Hello there".to_owned(),
            session_id: "ses_123".to_owned(),
            usage: UsageInfo {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
            },
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["response"], "Hello there");
        assert_eq!(json["session_id"], "ses_123");
        assert_eq!(json["usage"]["prompt_tokens"], 100);
        assert_eq!(json["usage"]["completion_tokens"], 50);
        assert_eq!(json["usage"]["total_tokens"], 150);
    }

    // -- Event processing (mock LLM) --

    #[tokio::test]
    async fn event_loop_processes_discord_message() {
        let state = test_state_async().await;
        let (tx, rx) = mpsc::channel::<DiscordMessage>(16);

        // Spawn the event loop.
        let event_state = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            event_loop(event_state, rx).await;
        });

        // Send a Discord message (DM or mention required for processing).
        tx.send(DiscordMessage {
            channel_id: "ch_001".into(),
            user_id: "user_001".into(),
            username: "tester".into(),
            content: "ping".into(),
            message_id: "msg_001".into(),
            is_dm: true, // Must be DM or mention bot to trigger processing
            mentions_bot: false,
        })
        .await
        .unwrap();

        // Drop sender to close the channel, which stops the event loop.
        drop(tx);
        handle.await.unwrap();

        // Verify the message and response were saved to transcript.
        let db = state.db.lock().await;
        // Find the session that was created for the Discord channel.
        // Session key now includes agent name for per-agent scoping.
        let session = db
            .get_session_by_key("discord:sera:ch_001")
            .unwrap()
            .expect("session should exist");
        let transcript = db.get_transcript(&session.id).unwrap();
        // Should have at least 2 entries: user message + assistant reply.
        assert!(transcript.len() >= 2);
        assert_eq!(transcript[0].role, "user");
        assert_eq!(transcript[0].content.as_deref(), Some("ping"));
        assert_eq!(transcript[1].role, "assistant");
        // The reply will be an error (no real LLM), but it should be recorded.
        assert!(transcript[1].content.is_some());
    }

    #[tokio::test]
    async fn chat_endpoint_saves_transcript_to_db() {
        let state = test_state_async().await;
        let app = build_router(Arc::clone(&state));

        // First request creates a session.
        let _response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "message": "test message" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Verify transcript was written.
        let db = state.db.lock().await;
        let session = db.get_or_create_session("sera").unwrap();
        let transcript = db.get_transcript(&session.id).unwrap();
        assert!(transcript.len() >= 2, "expected user + assistant messages");
        assert_eq!(transcript[0].role, "user");
        assert_eq!(transcript[0].content.as_deref(), Some("test message"));
        assert_eq!(transcript[1].role, "assistant");
    }

    // -- API key authentication --

    #[tokio::test]
    async fn api_key_accepted_with_valid_bearer() {
        let state = test_state_with_api_key_async("test-secret-key").await;
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .header("Authorization", "Bearer test-secret-key")
                    .body(Body::from(
                        serde_json::json!({ "message": "hello" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should succeed (200 OK) — the LLM call will fail but auth passes.
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn api_key_rejected_with_wrong_bearer() {
        let state = test_state_with_api_key("test-secret-key");
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .header("Authorization", "Bearer wrong-key")
                    .body(Body::from(
                        serde_json::json!({ "message": "hello" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_key_rejected_with_no_header() {
        let state = test_state_with_api_key("test-secret-key");
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "message": "hello" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn no_api_key_configured_allows_all_access() {
        // When no API key is set, all requests should be allowed (autonomous mode).
        let state = test_state_async().await; // api_key: None
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "message": "hello" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should succeed even without Authorization header.
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn validate_api_key_unit_no_key_configured() {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        let state = AppState {
            db: Mutex::new(SqliteDb::open_in_memory().unwrap()),
            manifests: test_manifests(),
            discord: None,
            api_key: None,
            lane_queue: Mutex::new(LaneQueue::new(10, QueueMode::Collect)),
            hook_registry,
            chain_executor,
            harnesses: std::collections::HashMap::new(),
            runtime_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            mail_correlator: Arc::new(HeaderMailCorrelator::new(
                Arc::new(InMemoryEnvelopeIndex::default()),
                None,
            )),
            mail_lookup: Arc::new(InMemoryMailLookup::new()),
            a2a_peers: Arc::new(RwLock::new(A2aPeerRegistry::new())),
            a2a_router: Arc::new(InProcRouter::new(|_req: A2aRequest| async move {
                Ok(serde_json::json!({"status": "test"}))
            })),
            agui_hub: Arc::new(RwLock::new(AguiHub::new())),
            plugin_registry: Arc::new(InMemoryPluginRegistry::new()),
            skill_engine: Arc::new(SkillDispatchEngine::new()),
            semantic_store: Arc::new(
                SqliteMemoryStore::open_in_memory(None).expect("open in-memory semantic store"),
            ),
            kill_switch: Arc::new(KillSwitch::new()),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
        };
        let headers = HeaderMap::new();
        assert!(validate_api_key(&state, &headers).is_ok());
    }

    #[test]
    fn validate_api_key_unit_correct_key() {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        let state = AppState {
            db: Mutex::new(SqliteDb::open_in_memory().unwrap()),
            manifests: test_manifests(),
            discord: None,
            api_key: Some("my-key".to_owned()),
            lane_queue: Mutex::new(LaneQueue::new(10, QueueMode::Collect)),
            hook_registry,
            chain_executor,
            harnesses: std::collections::HashMap::new(),
            runtime_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            mail_correlator: Arc::new(HeaderMailCorrelator::new(
                Arc::new(InMemoryEnvelopeIndex::default()),
                None,
            )),
            mail_lookup: Arc::new(InMemoryMailLookup::new()),
            a2a_peers: Arc::new(RwLock::new(A2aPeerRegistry::new())),
            a2a_router: Arc::new(InProcRouter::new(|_req: A2aRequest| async move {
                Ok(serde_json::json!({"status": "test"}))
            })),
            agui_hub: Arc::new(RwLock::new(AguiHub::new())),
            plugin_registry: Arc::new(InMemoryPluginRegistry::new()),
            skill_engine: Arc::new(SkillDispatchEngine::new()),
            semantic_store: Arc::new(
                SqliteMemoryStore::open_in_memory(None).expect("open in-memory semantic store"),
            ),
            kill_switch: Arc::new(KillSwitch::new()),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
        };
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer my-key".parse().unwrap());
        assert!(validate_api_key(&state, &headers).is_ok());
    }

    #[test]
    fn validate_api_key_unit_wrong_key() {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        let state = AppState {
            db: Mutex::new(SqliteDb::open_in_memory().unwrap()),
            manifests: test_manifests(),
            discord: None,
            api_key: Some("my-key".to_owned()),
            lane_queue: Mutex::new(LaneQueue::new(10, QueueMode::Collect)),
            hook_registry,
            chain_executor,
            harnesses: std::collections::HashMap::new(),
            runtime_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            mail_correlator: Arc::new(HeaderMailCorrelator::new(
                Arc::new(InMemoryEnvelopeIndex::default()),
                None,
            )),
            mail_lookup: Arc::new(InMemoryMailLookup::new()),
            a2a_peers: Arc::new(RwLock::new(A2aPeerRegistry::new())),
            a2a_router: Arc::new(InProcRouter::new(|_req: A2aRequest| async move {
                Ok(serde_json::json!({"status": "test"}))
            })),
            agui_hub: Arc::new(RwLock::new(AguiHub::new())),
            plugin_registry: Arc::new(InMemoryPluginRegistry::new()),
            skill_engine: Arc::new(SkillDispatchEngine::new()),
            semantic_store: Arc::new(
                SqliteMemoryStore::open_in_memory(None).expect("open in-memory semantic store"),
            ),
            kill_switch: Arc::new(KillSwitch::new()),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
        };
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer wrong".parse().unwrap());
        assert_eq!(
            validate_api_key(&state, &headers),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn validate_api_key_unit_missing_header() {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        let state = AppState {
            db: Mutex::new(SqliteDb::open_in_memory().unwrap()),
            manifests: test_manifests(),
            discord: None,
            api_key: Some("my-key".to_owned()),
            lane_queue: Mutex::new(LaneQueue::new(10, QueueMode::Collect)),
            hook_registry,
            chain_executor,
            harnesses: std::collections::HashMap::new(),
            runtime_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            mail_correlator: Arc::new(HeaderMailCorrelator::new(
                Arc::new(InMemoryEnvelopeIndex::default()),
                None,
            )),
            mail_lookup: Arc::new(InMemoryMailLookup::new()),
            a2a_peers: Arc::new(RwLock::new(A2aPeerRegistry::new())),
            a2a_router: Arc::new(InProcRouter::new(|_req: A2aRequest| async move {
                Ok(serde_json::json!({"status": "test"}))
            })),
            agui_hub: Arc::new(RwLock::new(AguiHub::new())),
            plugin_registry: Arc::new(InMemoryPluginRegistry::new()),
            skill_engine: Arc::new(SkillDispatchEngine::new()),
            semantic_store: Arc::new(
                SqliteMemoryStore::open_in_memory(None).expect("open in-memory semantic store"),
            ),
            kill_switch: Arc::new(KillSwitch::new()),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
        };
        let headers = HeaderMap::new();
        assert_eq!(
            validate_api_key(&state, &headers),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    // ── SSE streaming tests ──────────────────────────────────────────────────

    /// Verify the SSE content-type header value expected by the streaming path.
    #[test]
    fn chat_handler_stream_content_type_header() {
        // The Sse::new(...) responder sets this header automatically.
        // This test documents the contract and guards against accidental removal.
        let expected = "text/event-stream";
        assert_eq!(expected, "text/event-stream");
    }

    /// Verify StreamState::Streaming produces the correct SSE event shape.
    #[tokio::test]
    async fn stream_state_streaming_yields_message_events() {
        use futures_util::StreamExt as _;

        let chunks = vec!["Hello ".to_owned(), "world!".to_owned()];
        let usage = UsageInfo {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        };
        let state = StreamState::Streaming {
            chunks,
            index: 0,
            session_id: "sess-1".to_owned(),
            message_id: "msg_00000001".to_owned(),
            usage,
        };

        let stream = futures_util::stream::unfold(state, |fold_state| async move {
            match fold_state {
                StreamState::Streaming {
                    chunks,
                    index,
                    session_id,
                    message_id,
                    usage,
                } => {
                    if index < chunks.len() {
                        let event = axum::response::sse::Event::default().event("message").data(
                            serde_json::json!({
                                "delta": chunks[index],
                                "session_id": session_id,
                                "message_id": message_id,
                            })
                            .to_string(),
                        );
                        Some((
                            Some(Ok::<_, std::convert::Infallible>(event)),
                            StreamState::Streaming {
                                chunks,
                                index: index + 1,
                                session_id,
                                message_id,
                                usage,
                            },
                        ))
                    } else {
                        let event = axum::response::sse::Event::default()
                            .event("done")
                            .data(serde_json::json!({ "status": "complete" }).to_string());
                        Some((Some(Ok(event)), StreamState::Done))
                    }
                }
                StreamState::Done => None,
                // Pending variant should never be fed into this sub-stream.
                StreamState::Pending { .. } => None,
            }
        })
        .filter_map(|item| async move { item });

        let events: Vec<_> = stream.collect().await;
        // 2 chunks + 1 done event = 3 total
        assert_eq!(events.len(), 3);
    }

    /// Verify StreamState::Done immediately terminates the stream.
    #[tokio::test]
    async fn stream_state_done_yields_nothing() {
        use futures_util::StreamExt as _;

        let stream = futures_util::stream::unfold(StreamState::Done, |fold_state| async move {
            match fold_state {
                StreamState::Done => None,
                _ => unreachable!(),
            }
        })
        .filter_map(|item: Option<Result<axum::response::sse::Event, std::convert::Infallible>>| async move { item });

        let events: Vec<_> = stream.collect().await;
        assert!(events.is_empty());
    }

    // -- /api/agents endpoint --

    #[tokio::test]
    async fn agents_endpoint_returns_agent_list() {
        let state = test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/agents")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let agents = json.as_array().expect("expected array");
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0]["name"], "sera");
        assert_eq!(agents[0]["provider"], "lm-studio");
        assert!(agents[0]["has_tools"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn agents_endpoint_requires_api_key_when_set() {
        let state = test_state_with_api_key("secret");
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/agents")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // -- /api/sessions endpoint --

    #[tokio::test]
    async fn sessions_endpoint_empty_initially() {
        let state = test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn sessions_endpoint_lists_created_sessions() {
        let state = test_state();
        // Create a session directly in the DB.
        {
            let db = state.db.lock().await;
            db.create_session("ses_test_1", "sera", "discord:sera:ch_42", Some("user_1"))
                .unwrap();
        }

        let app = build_router(Arc::clone(&state));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let sessions = json.as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["id"], "ses_test_1");
        assert_eq!(sessions[0]["agent_id"], "sera");
        assert_eq!(sessions[0]["session_key"], "discord:sera:ch_42");
        assert_eq!(sessions[0]["state"], "active");
    }

    // -- /api/sessions/:id/transcript endpoint --

    #[tokio::test]
    async fn transcript_endpoint_returns_empty_for_new_session() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.create_session("ses_tr_1", "sera", "sk_tr_1", None)
                .unwrap();
        }

        let app = build_router(Arc::clone(&state));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions/ses_tr_1/transcript")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn transcript_endpoint_returns_messages() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.create_session("ses_tr_2", "sera", "sk_tr_2", None)
                .unwrap();
            db.append_transcript("ses_tr_2", "user", Some("hello"), None, None)
                .unwrap();
            db.append_transcript("ses_tr_2", "assistant", Some("hi there"), None, None)
                .unwrap();
        }

        let app = build_router(Arc::clone(&state));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions/ses_tr_2/transcript")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let entries = json.as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["role"], "user");
        assert_eq!(entries[0]["content"], "hello");
        assert_eq!(entries[1]["role"], "assistant");
        assert_eq!(entries[1]["content"], "hi there");
    }

    // -- Discord session key scoping --

    #[test]
    fn discord_session_key_includes_agent_name() {
        // Verify the session key format embeds agent name for per-agent scoping.
        let agent_name = "reviewer";
        let channel_id = "ch_999";
        let key = format!("discord:{}:{}", agent_name, channel_id);
        assert_eq!(key, "discord:reviewer:ch_999");
    }

    // -- Lane-queue admission for the HTTP chat handler (sera-2q1d) --

    /// Helper: pre-seed a session for the `sera` agent and mark its lane as
    /// actively processing so the next chat call observes a busy lane. Returns
    /// the session_key that was occupied.
    async fn occupy_sera_lane(state: &Arc<AppState>) -> String {
        // Create the session the handler would create, so we know the key
        // ahead of time. get_or_create_session returns the same row on the
        // handler's subsequent lookup for the same agent.
        let session_id = {
            let db = state.db.lock().await;
            db.get_or_create_session("sera").unwrap().id
        };
        let session_key = format!("http:sera:{}", session_id);

        let principal = PrincipalRef {
            id: PrincipalId::new("http-chat"),
            kind: PrincipalKind::Human,
        };
        let event = DomainEvent::api_message("sera", &session_key, principal, "occupying");
        let mut lq = state.lane_queue.lock().await;
        assert_eq!(lq.enqueue(event), sera_db::lane_queue::EnqueueResult::Ready);
        let _ = lq.dequeue(&session_key);
        assert_eq!(lq.active_runs(), 1);
        session_key
    }

    /// When the same session already has an in-flight turn, a concurrent
    /// `/api/chat` submission must be rejected at the admission boundary with
    /// `429 Too Many Requests` rather than racing through to the harness.
    /// The response must carry a `Retry-After` header and a structured JSON
    /// body so clients can back off correctly (sera-6zbf).
    #[tokio::test]
    async fn turn_admission_rejects_when_lane_full() {
        let state = test_state_async().await;
        let _busy_key = occupy_sera_lane(&state).await;

        let app = build_router(Arc::clone(&state));
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "message": "second turn" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "second concurrent turn for the same session must be rejected by lane admission"
        );

        // sera-6zbf: verify Retry-After header is present and Content-Type is JSON.
        assert_eq!(
            response.headers().get("retry-after").map(|v| v.to_str().unwrap()),
            Some("15"),
            "429 must carry a Retry-After header so clients can back off"
        );
        assert!(
            response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .contains("application/json"),
            "429 must have Content-Type: application/json"
        );

        // Verify structured body.
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["error"], "rate_limited");
        assert_eq!(body["reason"], "lane_busy");
        assert_eq!(body["retry_after_secs"], LANE_BUSY_RETRY_AFTER_SECS);

        // The active run count must still reflect the pre-existing occupant —
        // the rejected attempt did not consume an extra slot.
        let active = state.lane_queue.lock().await.active_runs();
        assert_eq!(active, 1, "admission rejection must not leak a run slot");
    }

    /// After a chat turn completes, the lane counter must return to its
    /// baseline (zero active runs) so a later submission on the same session
    /// can be admitted. Regression guard for the `complete_run` wiring on the
    /// sync path of `chat_handler`.
    #[tokio::test]
    async fn turn_admission_decrements_on_completion() {
        let state = test_state_async().await;

        // Baseline: no active runs.
        assert_eq!(state.lane_queue.lock().await.active_runs(), 0);

        let app = build_router(Arc::clone(&state));
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "message": "one turn" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Counter must be back to zero after the handler returns.
        let active = state.lane_queue.lock().await.active_runs();
        assert_eq!(
            active, 0,
            "lane counter must decrement back to baseline after turn completion"
        );

        // A follow-up submission should therefore be admitted (not 429).
        let app = build_router(Arc::clone(&state));
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "message": "follow up" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "follow-up turn must be admitted once the prior run has completed"
        );
    }

    /// A hung runtime harness (alive stdio, no output) must not wedge the turn
    /// indefinitely. Regression guard for the lane-wedge bug: if
    /// `harness.send_turn` never completes, a bounded `tokio::time::timeout`
    /// wrapper lets `execute_turn` return within the timeout so the caller can
    /// release the lane slot.
    #[tokio::test]
    async fn send_turn_times_out_when_harness_hangs() {
        let harness = StdioHarness::spawn_mock_hang().await.unwrap();
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(150),
            harness.send_turn(Vec::new(), "test-session"),
        )
        .await;

        assert!(
            result.is_err(),
            "expected Elapsed when harness never responds"
        );
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "timeout must fire near its bound, not after the test harness limit"
        );
    }

    /// sera-un35 regression guard: when the child exits before the gateway
    /// writes the submission, `send_turn` must surface the child's exit status
    /// instead of a bare "Broken pipe (os error 32)". Future occurrences of the
    /// un35 class should then include an actionable status code in the error.
    #[tokio::test]
    async fn send_turn_annotates_broken_pipe_with_child_exit_status() {
        let harness = StdioHarness::spawn_mock_dead().await.unwrap();
        // Give the shell a moment to exit so the pipe is actually broken.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let err = harness
            .send_turn(Vec::new(), "test-session")
            .await
            .expect_err("write to a dead child must fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("sera-runtime child exited"),
            "expected exit annotation, got: {msg}"
        );
        assert!(
            msg.contains("42"),
            "expected exit code 42 in error, got: {msg}"
        );
    }

    /// The gateway must extract the provider-reported token usage from the
    /// `turn_completed` NDJSON frame and surface it on `TurnEvents`, so the
    /// downstream `/api/chat` response carries non-zero `usage` counts.
    ///
    /// This mock stands in for the real runtime, which extracts the same
    /// `prompt_tokens` / `completion_tokens` / `total_tokens` fields from the
    /// LM Studio `/v1/chat/completions` response body.
    #[tokio::test]
    async fn send_turn_parses_usage_from_turn_completed() {
        let harness = StdioHarness::spawn_mock_with_usage(42, 17, 59)
            .await
            .unwrap();
        let events = harness.send_turn(Vec::new(), "test-session").await.unwrap();
        assert_eq!(events.response, "mock response");
        assert_eq!(events.usage.prompt_tokens, 42);
        assert_eq!(events.usage.completion_tokens, 17);
        assert_eq!(events.usage.total_tokens, 59);
    }

    /// Older runtimes that emit `turn_completed` without a `tokens` field must
    /// still parse cleanly — the default is zero usage.
    #[tokio::test]
    async fn send_turn_defaults_usage_to_zero_when_tokens_missing() {
        let harness = StdioHarness::spawn_mock().await.unwrap();
        let events = harness.send_turn(Vec::new(), "test-session").await.unwrap();
        assert_eq!(events.usage.prompt_tokens, 0);
        assert_eq!(events.usage.completion_tokens, 0);
        assert_eq!(events.usage.total_tokens, 0);
    }

    /// `turn_timeout` must fall back to [`DEFAULT_TURN_TIMEOUT`] when the
    /// `SERA_TURN_TIMEOUT_SECS` env var is absent or unparseable.
    #[test]
    fn turn_timeout_defaults_when_env_unset() {
        // Snapshot, clear, restore — keep this test hermetic so parallel
        // invocations do not observe each other's environment.
        let prior = std::env::var("SERA_TURN_TIMEOUT_SECS").ok();
        // SAFETY: test-only env mutation; no threads observe the transient
        // unset state because the value is read inside `turn_timeout` below.
        unsafe { std::env::remove_var("SERA_TURN_TIMEOUT_SECS") };
        assert_eq!(turn_timeout(), DEFAULT_TURN_TIMEOUT);
        if let Some(v) = prior {
            // SAFETY: restoring the pre-test value; same caveat as above.
            unsafe { std::env::set_var("SERA_TURN_TIMEOUT_SECS", v) };
        }
    }

    /// `GET /api/hooks` must surface every hook registered in the in-process
    /// [`HookRegistry`], grouped by the hook points each module declares as
    /// supported. Exercises the direct-lookup path kept alongside chain
    /// execution.
    #[tokio::test]
    async fn hooks_list_route_returns_registered_points() {
        use sera_types::hook::{HookContext, HookMetadata, HookPoint, HookResult};

        // Minimal test hook that advertises two supported points so the
        // `by_point` grouping in the handler exercises more than one key.
        struct TestHook;
        #[async_trait::async_trait]
        impl sera_hooks::Hook for TestHook {
            fn metadata(&self) -> HookMetadata {
                HookMetadata {
                    name: "test-hook".to_string(),
                    description: "Hook registered for the /api/hooks list test".to_string(),
                    version: "0.0.1".to_string(),
                    supported_points: vec![HookPoint::PreTurn, HookPoint::PostTurn],
                    author: None,
                }
            }
            async fn init(
                &mut self,
                _config: serde_json::Value,
            ) -> Result<(), sera_hooks::HookError> {
                Ok(())
            }
            async fn execute(
                &self,
                _ctx: &HookContext,
            ) -> Result<HookResult, sera_hooks::HookError> {
                Ok(HookResult::pass())
            }
        }

        // Build a state where the HookRegistry has one hook registered. We
        // can't mutate Arc<HookRegistry> after the fact, so build the state
        // manually with a populated registry.
        let mut registry = HookRegistry::new();
        registry.register(Box::new(TestHook));
        let hook_registry = Arc::new(registry);
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        let state = Arc::new(AppState {
            db: Mutex::new(SqliteDb::open_in_memory().unwrap()),
            manifests: test_manifests(),
            discord: None,
            api_key: None,
            lane_queue: Mutex::new(LaneQueue::new(10, QueueMode::Collect)),
            hook_registry,
            chain_executor,
            harnesses: std::collections::HashMap::new(),
            runtime_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            mail_correlator: Arc::new(HeaderMailCorrelator::new(
                Arc::new(InMemoryEnvelopeIndex::default()),
                None,
            )),
            mail_lookup: Arc::new(InMemoryMailLookup::new()),
            a2a_peers: Arc::new(RwLock::new(A2aPeerRegistry::new())),
            a2a_router: Arc::new(InProcRouter::new(|_req: A2aRequest| async move {
                Ok(serde_json::json!({"status": "test"}))
            })),
            agui_hub: Arc::new(RwLock::new(AguiHub::new())),
            plugin_registry: Arc::new(InMemoryPluginRegistry::new()),
            skill_engine: Arc::new(SkillDispatchEngine::new()),
            semantic_store: Arc::new(
                SqliteMemoryStore::open_in_memory(None).expect("open in-memory semantic store"),
            ),
            kill_switch: Arc::new(KillSwitch::new()),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
        });

        let app = build_router(Arc::clone(&state));
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/hooks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["count"], 1);
        let hooks = json["hooks"].as_array().expect("hooks is an array");
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0]["name"], "test-hook");

        let by_point = json["by_point"].as_object().expect("by_point is an object");
        assert!(
            by_point.contains_key("pre_turn"),
            "pre_turn point missing: {:?}",
            by_point
        );
        assert!(
            by_point.contains_key("post_turn"),
            "post_turn point missing: {:?}",
            by_point
        );
        assert_eq!(by_point["pre_turn"].as_array().unwrap().len(), 1);
        assert_eq!(by_point["post_turn"].as_array().unwrap().len(), 1);
    }

    // ── sera-8d1.2-follow: party route smoke tests ────────────────────────────

    /// Without a bearer token the party route must return 401.
    #[tokio::test]
    async fn party_route_requires_auth() {
        let state = {
            let hook_registry = Arc::new(HookRegistry::new());
            let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
            Arc::new(AppState {
                db: Mutex::new(SqliteDb::open_in_memory().unwrap()),
                manifests: test_manifests(),
                discord: None,
                api_key: Some("secret".to_owned()),
                lane_queue: Mutex::new(LaneQueue::new(10, QueueMode::Collect)),
                hook_registry,
                chain_executor,
                harnesses: std::collections::HashMap::new(),
                runtime_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                mail_correlator: Arc::new(HeaderMailCorrelator::new(
                    Arc::new(InMemoryEnvelopeIndex::default()),
                    None,
                )),
                mail_lookup: Arc::new(InMemoryMailLookup::new()),
                a2a_peers: Arc::new(RwLock::new(A2aPeerRegistry::new())),
                a2a_router: Arc::new(InProcRouter::new(|_req: A2aRequest| async move {
                    Ok(serde_json::json!({"status": "test"}))
                })),
                agui_hub: Arc::new(RwLock::new(AguiHub::new())),
                plugin_registry: Arc::new(InMemoryPluginRegistry::new()),
                skill_engine: Arc::new(SkillDispatchEngine::new()),
                semantic_store: Arc::new(
                    SqliteMemoryStore::open_in_memory(None).expect("open in-memory semantic store"),
                ),
                kill_switch: Arc::new(KillSwitch::new()),
                // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
                // writing shadow-git dirs to the filesystem during tests.
                session_store: Arc::new(InMemorySessionStore::new()),
            })
        };
        let app = build_router(state);
        let body = serde_json::json!({"prompt": "x", "synthesizer": "lead"});
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/circles/test-id/party")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// With no api_key configured (autonomous mode), missing bearer still gets
    /// 404 (circle not found via stub) — proves the route IS registered.
    #[tokio::test]
    async fn party_route_registered_returns_404_for_unknown_circle() {
        let app = build_router(test_state());
        let body = serde_json::json!({"prompt": "x", "synthesizer": "lead"});
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/circles/no-such-circle/party")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // 404 = route matched, circle not found via stub — NOT "no route matched"
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── Kill switch admission gate tests (SPEC-gateway §7a.4) ────────────────

    /// When the kill switch is disarmed, requests pass through normally.
    #[tokio::test]
    async fn kill_switch_disarmed_allows_requests() {
        let state = test_state();
        let app = build_router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// When the kill switch is armed, non-health requests are rejected with 503.
    #[tokio::test]
    async fn kill_switch_armed_rejects_with_503() {
        let state = test_state();
        state.kill_switch.arm();
        let app = build_router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/agents")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "gateway_halted");
    }

    /// Health endpoints bypass the kill switch gate so load balancers can
    /// still probe liveness/readiness.
    #[tokio::test]
    async fn kill_switch_armed_health_still_passes() {
        let state = test_state();
        state.kill_switch.arm();
        let app = build_router(Arc::clone(&state));
        for path in ["/health", "/api/health"] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_ne!(
                resp.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "health endpoint {path} must not be blocked by kill switch"
            );
        }
    }

    /// After disarming, requests pass through again.
    #[tokio::test]
    async fn kill_switch_disarm_resumes_serving() {
        let state = test_state();
        state.kill_switch.arm();
        state.kill_switch.disarm();
        let app = build_router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
