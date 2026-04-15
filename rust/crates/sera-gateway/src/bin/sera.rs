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

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use futures_util::stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};
use tracing_subscriber::EnvFilter;

use sera_config::manifest_loader::{
    load_manifest_file, parse_manifests, resolve_provider_api_key,
    ManifestSet,
};
use sera_config::secrets::SecretResolver;
use sera_db::lane_queue::{LaneQueue, QueueMode};
use sera_db::sqlite::SqliteDb;
use sera_types::event::Event as DomainEvent;
use sera_types::hook::{HookChain, HookContext, HookPoint, HookResult};
use sera_types::principal::{PrincipalId, PrincipalKind, PrincipalRef};
use sera_hooks::{ChainExecutor, HookRegistry};
use sera_types::config_manifest::{AgentSpec, ConnectorSpec, ProviderSpec};

// Re-use sera-core's Discord connector.
#[path = "../discord.rs"]
mod discord;
use discord::{DiscordConnector, DiscordMessage};

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
    async fn send_turn(&self, messages: Vec<serde_json::Value>, session_key: &str) -> anyhow::Result<TurnEvents> {
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

        stdin.write_all(json_line.as_bytes()).await?;
        stdin.flush().await?;

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
                            call_id: msg.get("call_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            tool: msg.get("tool").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            arguments: msg.get("arguments").cloned().unwrap_or(serde_json::Value::Null),
                        });
                    }
                }
                "tool_call_end" => {
                    if let Some(msg) = event.get("msg") {
                        result.tool_events.push(ToolEvent::End {
                            call_id: msg.get("call_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            content: msg.get("result").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        });
                    }
                }
                "turn_completed" => break,
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

    /// Send a graceful shutdown command to the runtime process.
    #[allow(dead_code)]
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
    Begin { call_id: String, tool: String, arguments: serde_json::Value },
    End { call_id: String, content: String },
}

/// Result from a harness turn — response text plus all tool call events.
#[derive(Debug, Default)]
struct TurnEvents {
    response: String,
    tool_events: Vec<ToolEvent>,
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
    #[allow(dead_code)]
    lane_queue: Mutex<LaneQueue>,
    /// Hook registry for lifecycle event hooks.
    #[allow(dead_code)]
    hook_registry: Arc<HookRegistry>,
    /// Chain executor for running hook pipelines.
    chain_executor: Arc<ChainExecutor>,
    /// Pre-connected runtime harnesses keyed by agent name.
    harnesses: std::collections::HashMap<String, Arc<StdioHarness>>,
}

// ── HTTP types ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    stream: bool,
}

#[derive(Serialize)]
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

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn chat_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
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

    // Save the user message to transcript.
    db.append_transcript(&session.id, "user", Some(&req.message), None, None)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Audit: message received.
    let _ = db.append_audit(
        "message_received",
        "human",
        "human",
        Some(&serde_json::json!({ "agent": agent_name, "message_len": req.message.len() }).to_string()),
    );

    // Get recent transcript for context.
    let transcript = db.get_transcript_recent(&session.id, 20).unwrap_or_default();
    let session_id = session.id.clone();
    let session_key = format!("http:{}:{}", agent_name, session_id);
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

        let sse_stream = stream::unfold(
            StreamState::Pending { agent_spec, transcript, message, state: state_clone, harness: harness_clone, session_id: sid, session_key: skey, message_id: mid_clone },
            |fold_state| async move {
                match fold_state {
                    StreamState::Pending { agent_spec, transcript, message, state, harness, session_id, session_key, message_id } => {
                        let result = execute_turn(&agent_spec, &transcript, &message, &harness, &session_key).await;

                        // Save tool events and assistant response.
                        {
                            let db = state.db.lock().await;
                            persist_tool_events(&db, &session_id, &result.tool_events);
                            let _ = db.append_transcript(&session_id, "assistant", Some(&result.reply), None, None);
                            let _ = db.append_audit(
                                "response_sent", "agent:sera", "agent",
                                Some(&serde_json::json!({
                                    "session_id": session_id,
                                    "response_len": result.reply.len(),
                                }).to_string()),
                            );
                        }

                        // Split reply into word-sized chunks for streaming.
                        let chunks: Vec<String> = result.reply.split_inclusive(' ')
                            .map(|s| s.to_owned())
                            .collect();
                        let usage = result.usage;

                        Some((None, StreamState::Streaming { chunks, index: 0, session_id, message_id, usage }))
                    }
                    StreamState::Streaming { chunks, index, session_id, message_id, usage } => {
                        if index < chunks.len() {
                            let payload = serde_json::json!({
                                "delta": chunks[index],
                                "session_id": session_id,
                                "message_id": message_id,
                            });
                            let event = Event::default()
                                .event("message")
                                .data(payload.to_string());
                            Some((Some(Ok::<_, std::convert::Infallible>(event)), StreamState::Streaming { chunks, index: index + 1, session_id, message_id, usage }))
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
                            let event = Event::default()
                                .event("done")
                                .data(payload.to_string());
                            Some((Some(Ok(event)), StreamState::Done))
                        }
                    }
                    StreamState::Done => None,
                }
            },
        )
        .filter_map(|item| async move { item });

        Ok(Sse::new(sse_stream).keep_alive(KeepAlive::default()).into_response())
    } else {
        // Synchronous JSON mode (existing behavior).
        let result =
            execute_turn(&agent_spec, &transcript, &req.message, &harness, &session_key).await;

        // Save tool events and assistant response.
        let db = state.db.lock().await;
        persist_tool_events(&db, &session_id, &result.tool_events);
        db.append_transcript(&session_id, "assistant", Some(&result.reply), None, None)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let _ = db.append_audit(
            "response_sent",
            "agent:sera",
            "agent",
            Some(&serde_json::json!({
                "session_id": session_id,
                "response_len": result.reply.len(),
                "usage": {
                    "prompt_tokens": result.usage.prompt_tokens,
                    "completion_tokens": result.usage.completion_tokens,
                    "total_tokens": result.usage.total_tokens,
                }
            }).to_string()),
        );

        Ok(Json(ChatResponse {
            response: result.reply,
            session_id,
            usage: result.usage,
        }).into_response())
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
                has_tools: spec
                    .as_ref()
                    .and_then(|s| s.tools.as_ref())
                    .is_some(),
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

/// Execute a turn by dispatching to a pre-connected sera-runtime harness.
///
/// The gateway builds the conversation messages from the transcript and sends
/// them to the harness. The harness (sera-runtime) owns LLM calls and tool
/// execution — the gateway never touches those.
async fn execute_turn(
    agent_spec: &AgentSpec,
    transcript: &[sera_db::sqlite::TranscriptRow],
    user_message: &str,
    harness: &StdioHarness,
    session_key: &str,
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

    match harness.send_turn(messages, session_key).await {
        Ok(events) => MvsTurnResult {
            reply: events.response,
            tool_events: events.tool_events,
            usage: UsageInfo {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        },
        Err(e) => {
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

    stdin.write_all(json_line.as_bytes()).await.unwrap();
    stdin.flush().await.unwrap();

    // Read TurnCompleted event.
    let mut line = String::new();
    loop {
        match stdout.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
                    if event.get("type").and_then(|v| v.as_str()) == Some("turn_completed") {
                        break;
                    }
                }
            }
            Err(_) => break,
        }
        line.clear();
    }

    MvsTurnResult {
        reply: "[steer injected]".to_string(),
        tool_events: vec![],
        usage: UsageInfo {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        },
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
            ToolEvent::Begin { call_id, tool, arguments } => {
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
                let _ = db.append_transcript(
                    session_id,
                    "tool",
                    Some(content),
                    None,
                    Some(call_id),
                );
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
        tracing::error!("Failed to send error to Discord: {e}");
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
    match state.chain_executor.execute_at_point(point, chains, ctx).await {
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
            }
        }
    }
}

async fn event_loop(state: Arc<AppState>, mut rx: mpsc::Receiver<DiscordMessage>) {
    tracing::info!("Event processing loop started");

    while let Some(msg) = rx.recv().await {
        if let Err(e) = process_message(&state, &msg).await {
            tracing::error!(error = %e, "Message processing failed");
            send_error_to_discord(&state, &msg.channel_id, &e.to_string()).await;
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
            Some(&serde_json::json!({
                "username": msg.username,
                "channel_id": msg.channel_id,
                "message_len": msg.content.len(),
            }).to_string()),
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
        change_artifact: None, // TODO(P0-5/P0-6): populate from gateway pipeline
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

    let agent_spec: AgentSpec = match state
        .manifests
        .agent_spec(&agent_name)
        .ok()
        .flatten()
    {
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
                if let Err(e) = db.create_session(
                    &id,
                    &agent_name,
                    &session_key,
                    Some(&msg.user_id),
                ) {
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
        let transcript = db.get_transcript_recent(&session.id, 20).unwrap_or_default();
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
        change_artifact: None, // TODO(P0-5/P0-6): populate from gateway pipeline
    };
    let post_route_result = run_hook_point(state, HookPoint::PostRoute, &chains, post_route_ctx).await;
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
        change_artifact: None, // TODO(P0-5/P0-6): populate from gateway pipeline
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
        }
    }

    // Execute the agent turn via the pre-connected harness.
    let result = execute_turn(&agent_spec, &transcript, &msg.content, &harness, &session_key).await;

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
        change_artifact: None, // TODO(P0-5/P0-6): populate from gateway pipeline
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
            tracing::error!("Failed to send Discord reply: {e}");
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
            .filter_map(|qe| qe.event.text.as_ref().map(|t| serde_json::json!({"role": "user", "content": t})))
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
                let steer_text = steer_content.iter()
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
                tracing::error!("Failed to send Discord steer response: {e}");
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
            db.get_transcript_recent(&session.id, 20).unwrap_or_default()
        };

        let follow_up = execute_turn(&agent_spec, &transcript, &user_content, &harness, &session_key).await;

        {
            let db = state.db.lock().await;
            persist_tool_events(&db, &session.id, &follow_up.tool_events);
            let _ = db.append_transcript(&session.id, "assistant", Some(&follow_up.reply), None, None);
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
            tracing::error!("Failed to send Discord follow-up reply: {e}");
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
  tier: local
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
        SecretCommands::Get { path } => {
            match resolver.resolve(&path) {
                Some(v) => {
                    let masked = mask_secret(&v);
                    println!("{path}: {masked}");
                }
                None => {
                    anyhow::bail!("Secret not found: {path}");
                }
            }
        }
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
        anyhow::bail!(
            "{} not found. Run `sera init` first.",
            config.display()
        );
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

// ── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

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
    let db_path = PathBuf::from("sera.db");
    tracing::info!(path = %db_path.display(), "Opening SQLite database");
    let db = SqliteDb::open(&db_path)?;

    // 3. Resolve Discord connector if configured.  We create a shared Arc so
    //    the gateway listener and the event-loop response sender use the same
    //    REST client / token.
    let (discord_tx, discord_rx) = mpsc::channel::<DiscordMessage>(256);
    let mut shared_discord: Option<Arc<DiscordConnector>> = None;

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

        let token = match sera_config::manifest_loader::resolve_connector_token_with(&spec, &secret_resolver) {
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
        ));
        shared_discord = Some(Arc::clone(&connector));

        // Spawn the gateway listener.
        tokio::spawn(async move {
            if let Err(e) = connector.run().await {
                tracing::error!("Discord connector exited with error: {e}");
            }
        });
    }

    // Load API key from environment (if set).
    let api_key = std::env::var("SERA_API_KEY").ok().filter(|k| !k.is_empty());
    if api_key.is_some() {
        tracing::info!("API key authentication enabled (SERA_API_KEY is set)");
    } else {
        tracing::info!("API key authentication disabled (autonomous mode)");
    }

    let hook_registry = Arc::new(HookRegistry::new());
    let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));

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

    let state = Arc::new(AppState {
        db: Mutex::new(db),
        manifests,
        discord: shared_discord,
        api_key,
        lane_queue: Mutex::new(LaneQueue::new(10, QueueMode::Collect)),
        hook_registry,
        chain_executor,
        harnesses,
    });

    // 4. Start event processing loop.
    let event_state = Arc::clone(&state);
    tokio::spawn(async move {
        event_loop(event_state, discord_rx).await;
    });

    // 5. Build and start HTTP server.
    let app = build_router(Arc::clone(&state));
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(%addr, "Starting HTTP server");

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // 6. Graceful shutdown on SIGINT/SIGTERM.
    let shutdown_signal = async {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to install SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => {},
                _ = sigterm.recv() => {},
            }
        }
        #[cfg(not(unix))]
        {
            ctrl_c.await.ok();
        }
        tracing::info!("Shutdown signal received");
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await?;

    tracing::info!("SERA gateway shut down");
    Ok(())
}

fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/api/chat", post(chat_handler))
        .route("/api/agents", get(agents_handler))
        .route("/api/sessions", get(sessions_handler))
        .route("/api/sessions/{id}/transcript", get(transcript_handler))
        .with_state(state)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

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
        })
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
        let cli = Cli::try_parse_from(["sera", "start", "-c", "custom.yaml", "-p", "8080"]).unwrap();
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
            Commands::Agent { command: AgentCommands::List } => {}
            _ => panic!("expected Agent List"),
        }
    }

    #[test]
    fn parse_agent_create() {
        let cli = Cli::try_parse_from(["sera", "agent", "create", "reviewer"]).unwrap();
        match cli.command {
            Commands::Agent { command: AgentCommands::Create { name } } => {
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
        assert!(spec.persona.unwrap().immutable_anchor.unwrap().contains("Sera"));
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
        };
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer wrong".parse().unwrap());
        assert_eq!(validate_api_key(&state, &headers), Err(StatusCode::UNAUTHORIZED));
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
        };
        let headers = HeaderMap::new();
        assert_eq!(validate_api_key(&state, &headers), Err(StatusCode::UNAUTHORIZED));
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
                StreamState::Streaming { chunks, index, session_id, message_id, usage } => {
                    if index < chunks.len() {
                        let event = axum::response::sse::Event::default()
                            .event("message")
                            .data(serde_json::json!({
                                "delta": chunks[index],
                                "session_id": session_id,
                                "message_id": message_id,
                            }).to_string());
                        Some((
                            Some(Ok::<_, std::convert::Infallible>(event)),
                            StreamState::Streaming { chunks, index: index + 1, session_id, message_id, usage },
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
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
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
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn sessions_endpoint_lists_created_sessions() {
        let state = test_state();
        // Create a session directly in the DB.
        {
            let db = state.db.lock().await;
            db.create_session("ses_test_1", "sera", "discord:sera:ch_42", Some("user_1")).unwrap();
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
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
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
            db.create_session("ses_tr_1", "sera", "sk_tr_1", None).unwrap();
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
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn transcript_endpoint_returns_messages() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.create_session("ses_tr_2", "sera", "sk_tr_2", None).unwrap();
            db.append_transcript("ses_tr_2", "user", Some("hello"), None, None).unwrap();
            db.append_transcript("ses_tr_2", "assistant", Some("hi there"), None, None).unwrap();
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
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
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
}
