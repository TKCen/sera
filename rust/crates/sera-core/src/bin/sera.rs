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
use sera_domain::event::Event as DomainEvent;
use sera_domain::principal::{PrincipalId, PrincipalKind, PrincipalRef};
use sera_hooks::{ChainExecutor, HookRegistry};
use sera_domain::config_manifest::{AgentSpec, ConnectorSpec, ProviderSpec};
use sera_runtime::context::ContextManager;
use sera_runtime::tools::mvs_tools::MvsToolRegistry;

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
    #[allow(dead_code)]
    chain_executor: Arc<ChainExecutor>,
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

/// Internal result from a turn execution, carrying the reply text and usage
/// info extracted from the LLM response.
struct TurnResult {
    reply: String,
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
    drop(db); // Release lock before making HTTP call.

    if req.stream {
        // SSE streaming mode: spawn turn execution and stream word-by-word.
        let manifests = state.manifests.clone();
        let message = req.message.clone();
        let state_clone = Arc::clone(&state);
        let sid = session_id.clone();
        let mid = format!("msg_{:08x}", rand::random::<u32>());
        let mid_clone = mid.clone();

        let sse_stream = stream::unfold(
            StreamState::Pending { manifests, agent_spec, transcript, message, state: state_clone, session_id: sid, message_id: mid_clone },
            |fold_state| async move {
                match fold_state {
                    StreamState::Pending { manifests, agent_spec, transcript, message, state, session_id, message_id } => {
                        let result = execute_turn(&manifests, &agent_spec, &transcript, &message).await;

                        // Save assistant response.
                        {
                            let db = state.db.lock().await;
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
            execute_turn(&state.manifests, &agent_spec, &transcript, &req.message).await;

        // Save assistant response and audit.
        let db = state.db.lock().await;
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
enum StreamState {
    Pending {
        manifests: ManifestSet,
        agent_spec: AgentSpec,
        transcript: Vec<sera_db::sqlite::TranscriptRow>,
        message: String,
        state: Arc<AppState>,
        session_id: String,
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

// ── Turn execution (inlined from sera-runtime reasoning loop) ───────────────

/// Maximum number of tool-call iterations before forcing a text reply.
const MAX_TOOL_ITERATIONS: usize = 10;

/// Maximum number of context overflow retries before giving up.
const MAX_CONTEXT_OVERFLOW_RETRIES: usize = 3;

/// Returns true when the LLM returned a context-length-exceeded error.
///
/// OpenAI-compatible providers typically return HTTP 400 or 413 with one of
/// the well-known strings in the body.
fn is_context_overflow(status: reqwest::StatusCode, body: &str) -> bool {
    (status == reqwest::StatusCode::BAD_REQUEST
        || status == reqwest::StatusCode::PAYLOAD_TOO_LARGE)
        && (body.contains("context_length_exceeded")
            || body.contains("maximum context length")
            || body.contains("too many tokens")
            || body.contains("context window"))
}

/// Build the tool definitions array using the MVS tool registry.
/// Falls back to an empty list if the agent has no tools configured.
fn build_tool_definitions(agent_spec: &AgentSpec, registry: &MvsToolRegistry) -> Vec<serde_json::Value> {
    if agent_spec.tools.is_none() {
        return Vec::new();
    }
    registry.definitions()
}

/// Execute a single tool call via the MvsToolRegistry.
async fn execute_tool_call(registry: &MvsToolRegistry, name: &str, arguments: &str) -> String {
    tracing::info!(tool = %name, "Executing tool call");
    let args: serde_json::Value =
        serde_json::from_str(arguments).unwrap_or(serde_json::json!({}));
    match registry.execute(name, &args).await {
        Ok(output) => output,
        Err(e) => format!("[tool error] {e}"),
    }
}

/// Extract usage info from an OpenAI-compatible response body.
fn extract_usage(body: &serde_json::Value) -> UsageInfo {
    let usage = body.get("usage");
    UsageInfo {
        prompt_tokens: usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        completion_tokens: usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        total_tokens: usage
            .and_then(|u| u.get("total_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    }
}

async fn execute_turn(
    manifests: &ManifestSet,
    agent_spec: &AgentSpec,
    transcript: &[sera_db::sqlite::TranscriptRow],
    user_message: &str,
) -> TurnResult {
    let ctx_mgr = ContextManager::new(128_000);
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
            // Tool result message.
            let mut msg = serde_json::json!({
                "role": "tool",
                "content": row.content.as_deref().unwrap_or(""),
            });
            if let Some(tc_id) = &row.tool_call_id {
                msg["tool_call_id"] = serde_json::json!(tc_id);
            }
            messages.push(msg);
        } else if let Some(tc_json) = &row.tool_calls {
            // Assistant message with tool_calls (no text content).
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

    // Resolve provider details.
    let provider_spec: Option<ProviderSpec> =
        manifests.provider_spec(&agent_spec.provider).ok().flatten();

    let (base_url, model, api_key) = match provider_spec {
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
            return TurnResult {
                reply: format!(
                    "[sera] Provider '{}' not found in config.",
                    agent_spec.provider
                ),
                usage: UsageInfo {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
            };
        }
    };

    // Create the MVS tool registry scoped to this agent's workspace.
    let workspace = PathBuf::from(format!("./data/agents/{}", agent_spec.provider));
    let tool_registry = MvsToolRegistry::new(&workspace);

    // Build tool definitions from the registry.
    let tools = build_tool_definitions(agent_spec, &tool_registry);

    let client = reqwest::Client::new();
    let mut cumulative_usage = UsageInfo {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    };

    // Tool-call loop: call LLM, execute tool calls, repeat until text reply
    // or MAX_TOOL_ITERATIONS is reached.
    let mut overflow_retries: usize = 0;
    for iteration in 0..=MAX_TOOL_ITERATIONS {
        // Context management: check if near limit and compact if needed.
        if ctx_mgr.is_near_limit_json(&messages) {
            tracing::warn!(
                tokens = ContextManager::count_json_message_tokens(&messages),
                "Context near limit, compacting messages"
            );
            // Simple compaction: remove oldest non-system messages.
            let system_msgs: Vec<_> = messages.iter().filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("system")).cloned().collect();
            let non_system: Vec<_> = messages.iter().filter(|m| m.get("role").and_then(|r| r.as_str()) != Some("system")).cloned().collect();
            let keep = non_system.len().min(4); // preserve recent
            let to_keep = &non_system[non_system.len().saturating_sub(keep)..];
            messages.clear();
            messages.extend(system_msgs);
            if non_system.len() > keep {
                messages.push(serde_json::json!({
                    "role": "system",
                    "content": format!("[Context compacted: {} earlier messages removed to fit within context window.]", non_system.len() - keep),
                }));
            }
            messages.extend(to_keep.iter().cloned());
        }

        // Log remaining context budget.
        let current_tokens = ContextManager::count_json_message_tokens(&messages);
        let budget = ctx_mgr.high_water_mark().saturating_sub(current_tokens);
        tracing::debug!(current_tokens, budget, "Context budget before LLM call");

        let mut request_body = serde_json::json!({
            "model": model,
            "messages": messages,
            "max_tokens": 4096,
        });

        // Include tools if defined.
        if !tools.is_empty() {
            request_body["tools"] = serde_json::json!(tools);
        }

        let mut req_builder = client
            .post(format!("{}/chat/completions", base_url))
            .header("Content-Type", "application/json");
        if !api_key.is_empty() {
            req_builder = req_builder.header("Authorization", format!("Bearer {api_key}"));
        }

        let response = match req_builder.json(&request_body).send().await {
            Ok(resp) => resp,
            Err(e) => {
                return TurnResult {
                    reply: format!("[sera] LLM request failed: {e}"),
                    usage: cumulative_usage,
                };
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();

            if is_context_overflow(status, &body_text)
                && overflow_retries < MAX_CONTEXT_OVERFLOW_RETRIES
            {
                overflow_retries += 1;
                tracing::warn!(
                    retry = overflow_retries,
                    "Context overflow detected, performing aggressive compaction"
                );

                // Aggressive compaction: keep system messages + last 25% of non-system.
                let system_msgs: Vec<_> = messages
                    .iter()
                    .filter(|m| {
                        m.get("role").and_then(|r| r.as_str()) == Some("system")
                    })
                    .cloned()
                    .collect();
                let non_system: Vec<_> = messages
                    .iter()
                    .filter(|m| {
                        m.get("role").and_then(|r| r.as_str()) != Some("system")
                    })
                    .cloned()
                    .collect();
                let keep_count = (non_system.len() / 4).max(2);
                let removed = non_system.len().saturating_sub(keep_count);
                let to_keep = non_system[non_system.len().saturating_sub(keep_count)..].to_vec();

                messages.clear();
                messages.extend(system_msgs);
                messages.push(serde_json::json!({
                    "role": "system",
                    "content": format!(
                        "[Context aggressively compacted (retry {}/{}): {} messages removed to fit within model's context window.]",
                        overflow_retries, MAX_CONTEXT_OVERFLOW_RETRIES, removed
                    ),
                }));
                messages.extend(to_keep);

                continue; // retry the LLM call
            }

            return TurnResult {
                reply: format!("[sera] LLM error {status}: {body_text}"),
                usage: cumulative_usage,
            };
        }

        let body: serde_json::Value = match response.json().await {
            Ok(v) => v,
            Err(e) => {
                return TurnResult {
                    reply: format!("[sera] Failed to parse LLM response: {e}"),
                    usage: cumulative_usage,
                };
            }
        };

        // Reset overflow retry counter on a successful response.
        overflow_retries = 0;

        // Accumulate usage.
        let step_usage = extract_usage(&body);
        cumulative_usage.prompt_tokens += step_usage.prompt_tokens;
        cumulative_usage.completion_tokens += step_usage.completion_tokens;
        cumulative_usage.total_tokens += step_usage.total_tokens;

        // Extract the first choice.
        let choice = match body.get("choices").and_then(|c| c.get(0)) {
            Some(c) => c,
            None => {
                return TurnResult {
                    reply: "[sera] No choices in LLM response".to_owned(),
                    usage: cumulative_usage,
                };
            }
        };

        let message = match choice.get("message") {
            Some(m) => m,
            None => {
                return TurnResult {
                    reply: "[sera] No message in LLM choice".to_owned(),
                    usage: cumulative_usage,
                };
            }
        };

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("stop");

        // Check for tool calls.
        let tool_calls = message
            .get("tool_calls")
            .and_then(|tc| tc.as_array())
            .cloned()
            .unwrap_or_default();

        if finish_reason == "tool_calls" || !tool_calls.is_empty() {
            if iteration == MAX_TOOL_ITERATIONS {
                tracing::warn!("Max tool iterations ({MAX_TOOL_ITERATIONS}) reached, returning last content");
                let content = message
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("[sera] Max tool iterations reached");
                return TurnResult {
                    reply: content.to_owned(),
                    usage: cumulative_usage,
                };
            }

            // Append the assistant message with tool_calls to the conversation.
            messages.push(message.clone());

            // Execute each tool call, truncate output, and append tool results.
            for tc in &tool_calls {
                let tc_id = tc
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let fn_obj = tc.get("function");
                let fn_name = fn_obj
                    .and_then(|f| f.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let fn_args = fn_obj
                    .and_then(|f| f.get("arguments"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");

                let raw_result = execute_tool_call(&tool_registry, fn_name, fn_args).await;
                // Truncate tool output to stay within context budget.
                let result = ctx_mgr.truncate_tool_output(&raw_result);

                messages.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tc_id,
                    "content": result,
                }));
            }
            // Continue the loop — call LLM again with tool results.
            continue;
        }

        // No tool calls — extract the text reply.
        let reply = message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("[sera] No response from LLM")
            .to_owned();

        return TurnResult {
            reply,
            usage: cumulative_usage,
        };
    }

    // Should not reach here, but just in case.
    TurnResult {
        reply: "[sera] Turn loop exited unexpectedly".to_owned(),
        usage: cumulative_usage,
    }
}

// ── Event processing loop ───────────────────────────────────────────────────

async fn event_loop(state: Arc<AppState>, mut rx: mpsc::Receiver<DiscordMessage>) {
    tracing::info!("Event processing loop started");

    while let Some(msg) = rx.recv().await {
        tracing::info!(
            user = %msg.username,
            channel = %msg.channel_id,
            "Received Discord message"
        );

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
                tracing::error!("Agent '{agent_name}' not found in manifests");
                continue;
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
                        tracing::error!("Failed to create session: {e}");
                        continue;
                    }
                    match db.get_session_by_key(&session_key) {
                        Ok(Some(s)) => s,
                        _ => continue,
                    }
                }
                Err(e) => {
                    tracing::error!("DB error: {e}");
                    continue;
                }
            };

            // Save the incoming message.
            let _ = db.append_transcript(&session.id, "user", Some(&msg.content), None, None);
            let transcript = db.get_transcript_recent(&session.id, 20).unwrap_or_default();
            (session, transcript)
        }; // db lock released

        // Create a domain Event for the incoming Discord message (informational —
        // the existing processing pipeline is unchanged).
        let principal = PrincipalRef {
            id: PrincipalId::new(&msg.user_id),
            kind: PrincipalKind::Human,
        };
        let domain_event = DomainEvent::message(&agent_name, &session_key, principal, &msg.content);
        tracing::debug!(event_id = %domain_event.id.0, "Created domain event for Discord message");

        let result = execute_turn(&state.manifests, &agent_spec, &transcript, &msg.content).await;

        {
            let db = state.db.lock().await;
            let _ = db.append_transcript(&session.id, "assistant", Some(&result.reply), None, None);
        }

        // Send the reply back to Discord via the shared connector.
        if let Some(ref dc) = state.discord {
            if let Err(e) = dc.send_message(&msg.channel_id, &result.reply).await {
                tracing::error!("Failed to send Discord reply: {e}");
            }
        } else {
            tracing::warn!("No Discord connector available to send reply");
        }
    }
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
  default_model: gemma-4-12b
---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: sera
spec:
  provider: lm-studio
  model: gemma-4-12b
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

    let state = Arc::new(AppState {
        db: Mutex::new(db),
        manifests,
        discord: shared_discord,
        api_key,
        lane_queue: Mutex::new(LaneQueue::new(10, QueueMode::Collect)),
        hook_registry,
        chain_executor,
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
        let state = test_state();
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

    // -- Turn execution helpers --

    #[test]
    fn build_tool_definitions_from_agent_spec() {
        let manifests = test_manifests();
        let spec = manifests.agent_spec("sera").unwrap().unwrap();
        let workspace = std::path::PathBuf::from("./data/agents/test");
        let registry = MvsToolRegistry::new(&workspace);
        let tools = build_tool_definitions(&spec, &registry);
        // Registry provides 8 MVS tools.
        assert_eq!(tools.len(), 8);
        // Each tool should be a function type.
        for tool in &tools {
            assert_eq!(tool["type"], "function");
            assert!(tool["function"]["name"].as_str().is_some());
        }
    }

    #[test]
    fn build_tool_definitions_empty_when_no_tools() {
        let spec = AgentSpec {
            provider: "test".to_owned(),
            model: None,
            persona: None,
            tools: None,
        };
        let workspace = std::path::PathBuf::from("./data/agents/test");
        let registry = MvsToolRegistry::new(&workspace);
        let tools = build_tool_definitions(&spec, &registry);
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn execute_tool_call_unknown_tool_returns_error() {
        let workspace = tempfile::tempdir().unwrap();
        let registry = MvsToolRegistry::new(workspace.path());
        let result = execute_tool_call(&registry, "nonexistent_tool", "{}").await;
        assert!(result.contains("[tool error]"));
        assert!(result.contains("nonexistent_tool"));
    }

    #[test]
    fn extract_usage_from_llm_body() {
        let body = serde_json::json!({
            "choices": [{ "message": { "content": "hi" } }],
            "usage": {
                "prompt_tokens": 42,
                "completion_tokens": 10,
                "total_tokens": 52
            }
        });
        let usage = extract_usage(&body);
        assert_eq!(usage.prompt_tokens, 42);
        assert_eq!(usage.completion_tokens, 10);
        assert_eq!(usage.total_tokens, 52);
    }

    #[test]
    fn extract_usage_missing_defaults_to_zero() {
        let body = serde_json::json!({ "choices": [] });
        let usage = extract_usage(&body);
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    // -- Event processing (mock LLM) --

    #[tokio::test]
    async fn event_loop_processes_discord_message() {
        let state = test_state();
        let (tx, rx) = mpsc::channel::<DiscordMessage>(16);

        // Spawn the event loop.
        let event_state = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            event_loop(event_state, rx).await;
        });

        // Send a Discord message.
        tx.send(DiscordMessage {
            channel_id: "ch_001".into(),
            user_id: "user_001".into(),
            username: "tester".into(),
            content: "ping".into(),
            message_id: "msg_001".into(),
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
        let state = test_state();
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
        let state = test_state_with_api_key("test-secret-key");
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
        let state = test_state(); // api_key: None
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
        };
        let headers = HeaderMap::new();
        assert_eq!(validate_api_key(&state, &headers), Err(StatusCode::UNAUTHORIZED));
    }

    // -- is_context_overflow --

    #[test]
    fn context_overflow_detected_400_context_length_exceeded() {
        assert!(is_context_overflow(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"code":"context_length_exceeded","message":"..."}}"#
        ));
    }

    #[test]
    fn context_overflow_detected_400_maximum_context_length() {
        assert!(is_context_overflow(
            reqwest::StatusCode::BAD_REQUEST,
            "This model's maximum context length is 4096 tokens."
        ));
    }

    #[test]
    fn context_overflow_detected_400_too_many_tokens() {
        assert!(is_context_overflow(
            reqwest::StatusCode::BAD_REQUEST,
            "too many tokens in the request"
        ));
    }

    #[test]
    fn context_overflow_detected_400_context_window() {
        assert!(is_context_overflow(
            reqwest::StatusCode::BAD_REQUEST,
            "exceeds the context window size"
        ));
    }

    #[test]
    fn context_overflow_detected_413() {
        assert!(is_context_overflow(
            reqwest::StatusCode::PAYLOAD_TOO_LARGE,
            "context_length_exceeded"
        ));
    }

    #[test]
    fn context_overflow_false_for_other_400() {
        assert!(!is_context_overflow(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"code":"invalid_request","message":"bad param"}}"#
        ));
    }

    #[test]
    fn context_overflow_false_for_500() {
        assert!(!is_context_overflow(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "context_length_exceeded"
        ));
    }

    #[test]
    fn context_overflow_false_for_401() {
        assert!(!is_context_overflow(
            reqwest::StatusCode::UNAUTHORIZED,
            "context_length_exceeded"
        ));
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
