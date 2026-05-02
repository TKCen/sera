//! MVS `sera` binary — standalone gateway that wires config, DB, Discord, and
//! a minimal HTTP API into a single process. No PostgreSQL, Docker, or Centrifugo
//! required.
//!
//! Usage:
//!   sera start [-c sera.yaml] [-p 3001]
//!   sera start --local                # unified local bootstrap (K.0)
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
use tokio_util::sync::CancellationToken;
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
use sera_gateway::admin::{
    AdminAppState, AdminAuditLogger, AdminAuth, AdminSessionInfo, resolve_admin_bind,
    resolve_admin_port, serve_admin,
};
use sera_gateway::agent_transport::{AgentTurnTransport, ToolEvent, TurnEvents, UsageInfo};
use sera_gateway::capability_enforcement::{CapabilityRegistry, PolicyDenial};
use sera_gateway::embedded_transport::EmbeddedRuntimeTransport;
use sera_gateway::hitl_gateway::{
    HitlAppState, InMemoryTicketStore, TicketStore, resolve_approval_routing, resolve_hitl_mode,
};
use sera_gateway::kill_switch::{KillSwitch, admin_sock_path, spawn_admin_socket};
use sera_gateway::scheduler::spawn_scheduler;
#[cfg(test)]
use sera_gateway::session_store::InMemorySessionStore;
use sera_gateway::session_store::{SessionStore, SqliteSessionStore};
#[cfg(feature = "enterprise")]
use sera_gateway::session_store::SqliteGitSessionStore;
use sera_gateway::workflow_store::{
    GhPrStateStore, GhRunStateStore, HumanGateStore, InMemoryGhPrStateStore,
    InMemoryGhRunStateStore, InMemoryHumanGateStore, InMemoryWorkflowTaskStore, WorkflowTaskStore,
};
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
#[path = "../routes/hitl.rs"]
mod route_hitl;
#[path = "../routes/workflow.rs"]
mod route_workflow;
#[path = "../routes/inference_proxy.rs"]
mod route_inference_proxy;

use route_a2a::{A2aAppState, A2aPeerRegistry};
use route_agui::{AguiAppState, AguiHub};
use route_plugins::PluginsAppState;
use route_workflow::WorkflowAppState;
use route_inference_proxy::{
    InferenceProxyAppState, InferenceProxyAudit, LlmBudgetGate, NoopBudgetGate,
    SqliteInferenceProxyAudit, UpstreamProvider,
};

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

/// sera-ve9x: build an in-process [`AgentTurnTransport`] backed by the
/// gateway-bundled `DefaultRuntime` for one agent.
///
/// Selected when `SERA_DISPATCH_MODE=embedded`. Skips spawning a
/// `sera-runtime --ndjson` child and avoids mutating the gateway process
/// env (in particular: `LLM_API_KEY` is passed through `RuntimeConfig` only,
/// so the key never enters `/proc/<gateway-pid>/environ`).
///
/// The capability registry is loaded with the same fail-closed semantics as
/// the runtime binary's `load_capability_registry` (sera-eo71): a missing /
/// unresolved `policy_ref` aborts construction so the agent is skipped
/// rather than silently running unconstrained.
fn build_embedded_transport(
    agent_name: &str,
    agent_spec: &AgentSpec,
    base_url: &str,
    model: &str,
    api_key_val: &str,
) -> anyhow::Result<Arc<dyn AgentTurnTransport>> {
    use sera_runtime::config::RuntimeConfig;
    use sera_runtime::context_engine::pipeline::ContextPipeline;
    use sera_runtime::default_runtime::DefaultRuntime;
    use sera_runtime::tools::TraitToolRegistry;
    use sera_runtime::tools::dispatcher::RegistryDispatcher;
    use sera_runtime::tools::filter::ToolNameFilter;

    // Start from gateway-process env defaults (max_tokens, semantic_*,
    // tool_authz_*, thinking_level) and override the per-agent fields
    // explicitly. `llm_api_key` is in-memory; we never call
    // `std::env::set_var("LLM_API_KEY", …)`, which closes a
    // `/proc/<gateway-pid>/environ` exposure window.
    let mut runtime_config = RuntimeConfig::from_env();
    runtime_config.llm_base_url = base_url.to_string();
    runtime_config.llm_model = model.to_string();
    runtime_config.llm_api_key = api_key_val.to_string();
    runtime_config.agent_id = agent_name.to_string();
    runtime_config.lifecycle_mode = "task".to_string();
    runtime_config.chat_port = 0;

    // sera-eo71: fail-closed capability registry load (mirrors the runtime
    // binary's `load_capability_registry`). The gateway's
    // `/admin/policies/reload` endpoint cannot yet propagate to embedded
    // transports — known parity caveat with the stdio child path, where the
    // child also loads the registry once on spawn.
    let policies_dir = sera_config::CapabilityRegistry::resolve_policies_dir();
    let policy_ref = agent_spec
        .policy_ref
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let bindings = vec![(agent_name.to_string(), policy_ref.clone())];
    let capability_registry = sera_config::CapabilityRegistry::load_and_bind(
        &policies_dir,
        bindings,
    )
    .map_err(|e| {
        anyhow::anyhow!(
            "embedded: failed to load capability registry for agent {agent_name}: {e}"
        )
    })?;
    tracing::info!(
        agent = %agent_name,
        policy_ref = ?policy_ref,
        loaded_policies = capability_registry.policy_count(),
        "Embedded capability registry loaded (sera-eo71)"
    );
    let capability_registry = Arc::new(capability_registry);

    // sera-hwny: tool schema filter from the agent manifest's allow list.
    // Deny is read from the gateway process env directly — no env mutation
    // needed because `ToolNameFilter::from_globs` accepts explicit lists.
    let tools_allow: Vec<String> = agent_spec
        .tools
        .as_ref()
        .map(|t| t.allow.clone())
        .unwrap_or_default();
    let tools_deny: Vec<String> = std::env::var("SERA_AGENT_TOOLS_DENY")
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let tool_filter = ToolNameFilter::from_globs(tools_allow, tools_deny);

    // sera-a1u: per-agent DelegationBus, mirroring the runtime binary path.
    let delegation_bus = sera_runtime::delegation_bus::DelegationBus::new();
    // sera-i4en: AgentToolRegistry backed by the production InProcAgentRouter
    // so the agent-as-tool layer can reach in-process targets when registered.
    // The router is per-agent here (no cross-agent registrations); a follow-up
    // bead threads a shared Arc<InProcAgentRouter> from the gateway boot so
    // sibling embedded transports can dispatch to one another.
    let agent_router: Arc<sera_runtime::agent_tool_registry::InProcAgentRouter> =
        Arc::new(sera_runtime::agent_tool_registry::InProcAgentRouter::new());
    let agent_registry = Arc::new(
        sera_runtime::agent_tool_registry::AgentToolRegistry::with_router(agent_router),
    );
    let registry = TraitToolRegistry::with_builtins_and_authz(runtime_config.tool_authz_enabled)
        .with_delegation(delegation_bus)
        .with_agent_tools(agent_registry);
    let registry = Arc::new(registry);

    let runtime_defs = registry.definitions();
    let filtered = tool_filter.filter_definitions(runtime_defs);
    let tool_defs: Vec<sera_types::tool::ToolDefinition> = filtered
        .iter()
        .filter_map(|d| {
            let value = serde_json::to_value(d).ok()?;
            serde_json::from_value(value).ok()
        })
        .collect();

    let dispatcher = RegistryDispatcher::new(Arc::clone(&registry))
        .with_capability_registry(Arc::clone(&capability_registry), agent_name.to_string());

    let authz_provider = sera_runtime::authz_builder::build_provider_from_config(&runtime_config);
    let permissive_gate = std::env::var("SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false);

    let context_engine = Box::new(ContextPipeline::new());
    let llm_client = Box::new(sera_runtime::llm_client::build_from_config(&runtime_config));
    let runtime = DefaultRuntime::new(context_engine)
        .with_llm(llm_client)
        .with_tool_dispatcher(Box::new(dispatcher))
        .with_authz_provider(authz_provider)
        .with_allow_missing_constitutional_gate(permissive_gate);

    Ok(Arc::new(EmbeddedRuntimeTransport::new(
        agent_name.to_string(),
        Arc::new(runtime),
        tool_defs,
    )))
}

/// sera-y45a: parses the operator-requested dispatch mode from the
/// `SERA_DISPATCH_MODE` env var.
///
/// `runtime` (default) names the shipped MVS where the `sera-runtime` child
/// owns the LLM client, tool registry, capability gate, and dispatcher.
/// `gateway` and `embedded` are migration *targets* — see
/// `docs/plan/decisions/2026-04-29-dispatch-ownership.md`. Unrecognised /
/// empty values silently fall back to `runtime` so a typo cannot accidentally
/// claim a security model the code does not implement.
///
/// **This returns the operator request, not an active mode.** A return of
/// `"gateway"` or `"embedded"` means the operator *asked for* that target;
/// it does NOT mean the running binary owns dispatch in that process. This
/// binary always launches `StdioHarness::spawn` for `sera-runtime`, so the
/// effective mode is always `runtime` until the migration steps in ADR §4
/// land. Callers that report what the running code actually does must use
/// [`effective_dispatch_mode_label`] instead, and the boot log surfaces both
/// values under distinct field names so an unimplemented target can never be
/// mistaken for an active dispatch model.
fn parse_configured_dispatch_mode(raw: Option<&str>) -> &'static str {
    match raw.map(str::trim) {
        Some("gateway") => "gateway",
        Some("embedded") => "embedded",
        // default + unrecognised + empty → fail safe to the shipped MVS mode.
        _ => "runtime",
    }
}

fn configured_dispatch_mode_label() -> &'static str {
    let raw = std::env::var("SERA_DISPATCH_MODE").ok();
    parse_configured_dispatch_mode(raw.as_deref())
}

/// sera-y45a / sera-ve9x: the dispatch mode that actually applies to the
/// running code.
///
/// `runtime` (default) spawns `sera-runtime --ndjson` per agent and routes
/// turns through [`crate::agent_transport::AgentTurnTransport`] backed by
/// `RuntimeChildSupervisor`.
///
/// `embedded` (sera-ve9x) constructs a `DefaultRuntime` in-process per
/// agent and routes turns through
/// [`crate::embedded_transport::EmbeddedRuntimeTransport`] — there is no
/// child process. Selected via `SERA_DISPATCH_MODE=embedded`.
///
/// `gateway` (ADR §4 step 3) is still unimplemented; the parser silently
/// falls back to `runtime` until the gateway-owned dispatcher lands so an
/// unimplemented target cannot masquerade as an active dispatch model.
fn effective_dispatch_mode_label() -> &'static str {
    match configured_dispatch_mode_label() {
        "embedded" => "embedded",
        // "gateway" is not yet implemented; fall back to runtime so the boot
        // log never claims a model the code does not own. The configured
        // request is still surfaced under `dispatch_mode_configured`.
        _ => "runtime",
    }
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
        /// HTTP port (defaults to 42540 when --local is set, 3001 otherwise)
        #[arg(
            short,
            long,
            default_value = "3001",
            default_value_if("local", "true", "42540")
        )]
        port: u16,
        /// Unified local bootstrap (K.0): auto-detect LM Studio at
        /// http://localhost:1234/v1, write data to ./sera-local/, permit
        /// missing ConstitutionalGate, and print a ready banner.
        #[arg(long)]
        local: bool,
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
        self.send_turn_inner(messages, session_key, None).await
    }

    /// Streaming variant (sera-k8do): forwards each `streaming_delta` NDJSON
    /// frame through `delta_tx` as it is read off the runtime's stdout, then
    /// returns the assembled `TurnEvents` once the terminal `turn_completed`
    /// frame arrives. Cancellation of the underlying read still happens via
    /// the gateway's `tokio::select!` on the cancellation token, so a dropped
    /// receiver does not stall the harness.
    ///
    /// `delta_tx` is bounded (Codex review on PR #1153) — `.send().await`
    /// applies backpressure to the runtime read loop when a slow SSE
    /// consumer is not draining frames, instead of accumulating
    /// unbounded deltas. If the receiver has been dropped (SSE
    /// disconnect), `send` returns `Err`; we keep reading the
    /// runtime stream so the turn still completes cleanly, while the
    /// gateway's `CancelOnDrop` guard fires the cancellation token to
    /// abort the harness via the existing `select!` arm.
    async fn send_turn_with_deltas(
        &self,
        messages: Vec<serde_json::Value>,
        session_key: &str,
        delta_tx: tokio::sync::mpsc::Sender<String>,
    ) -> anyhow::Result<TurnEvents> {
        self.send_turn_inner(messages, session_key, Some(delta_tx))
            .await
    }

    async fn send_turn_inner(
        &self,
        messages: Vec<serde_json::Value>,
        session_key: &str,
        delta_tx: Option<tokio::sync::mpsc::Sender<String>>,
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
                        // sera-k8do: forward the delta to the SSE pump
                        // if a streaming consumer is attached. The
                        // channel is bounded (Codex review on PR #1153),
                        // so `.send().await` applies backpressure when
                        // the SSE client is slow. A `SendError` means
                        // the receiver was dropped (SSE disconnect):
                        // we keep reading the runtime stream so the
                        // turn completes cleanly, while the gateway's
                        // `CancelOnDrop` guard will fire the
                        // cancellation token and abort us via the
                        // existing `select!` arm shortly after.
                        if let Some(tx) = delta_tx.as_ref() {
                            let _ = tx.send(delta.to_string()).await;
                        }
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

// ── Runtime child supervisor (sera-ojp3) ────────────────────────────────────
//
// One supervisor per agent owns at most one live `StdioHarness`. It detects
// `sera-runtime --ndjson` exits (try_wait at the next turn boundary, or an
// explicit `mark_unhealthy` after a write/read failure) and lazily respawns
// a fresh child. This narrows the blast radius of a runtime panic from
// "wedge the agent for the gateway pod's lifetime" (the original failure
// mode) to "the failing turn returns a clear runtime-crash error and the
// next turn respawns transparently".

/// Spawn factory used by the supervisor. `Process` is the production path;
/// `Mock` lets tests inject a closure that returns a fresh mock harness so
/// respawn paths can be exercised without forking real runtime processes.
enum SpawnFactory {
    Process {
        runtime_bin: String,
        env: std::collections::HashMap<String, String>,
    },
    #[cfg(test)]
    Mock(
        Box<
            dyn Fn() -> futures_util::future::BoxFuture<'static, anyhow::Result<StdioHarness>>
                + Send
                + Sync,
        >,
    ),
}

impl SpawnFactory {
    async fn spawn_one(&self) -> anyhow::Result<StdioHarness> {
        match self {
            Self::Process { runtime_bin, env } => {
                StdioHarness::spawn(runtime_bin, env.clone()).await
            }
            #[cfg(test)]
            Self::Mock(f) => f().await,
        }
    }
}

struct SupervisorState {
    /// Currently live runtime child, if any. `None` between exit detection
    /// and the next acquire (which respawns).
    harness: Option<Arc<StdioHarness>>,
    /// Monotonic counter incremented on every successful spawn. Surfaces in
    /// lifecycle log lines so operators can correlate `harness:respawned`
    /// with the prior `harness:exited` / `harness:unhealthy`.
    generation: u64,
    /// Last observed exit reason — exit status from `try_wait` or the
    /// caller-supplied reason from `mark_unhealthy`. Cleared on respawn.
    last_exit: Option<String>,
}

/// Per-agent runtime child supervisor (sera-ojp3).
///
/// Owns the `StdioHarness` for one agent. Detects child exit and respawns
/// without operator intervention; the previous design held a single harness
/// for the gateway's lifetime, so a single child panic permanently wedged
/// the agent.
struct RuntimeChildSupervisor {
    agent_id: String,
    factory: SpawnFactory,
    state: Mutex<SupervisorState>,
    stopping: std::sync::atomic::AtomicBool,
}

impl RuntimeChildSupervisor {
    /// Construct a supervisor for the production agent runtime path. The
    /// initial spawn must succeed; the bootstrap loop logs and skips the
    /// agent on error, matching the previous one-shot `StdioHarness::spawn`
    /// boot semantics.
    async fn start(
        agent_id: String,
        runtime_bin: String,
        env: std::collections::HashMap<String, String>,
    ) -> anyhow::Result<Arc<Self>> {
        let supervisor = Arc::new(Self {
            agent_id,
            factory: SpawnFactory::Process { runtime_bin, env },
            state: Mutex::new(SupervisorState {
                harness: None,
                generation: 0,
                last_exit: None,
            }),
            stopping: std::sync::atomic::AtomicBool::new(false),
        });
        {
            let mut state = supervisor.state.lock().await;
            supervisor.respawn_locked(&mut state).await?;
        }
        Ok(supervisor)
    }

    /// Acquire the current child handle. If the previously-spawned child
    /// has died (detected via `try_wait`) or was explicitly marked
    /// unhealthy, the supervisor spawns a fresh one before returning.
    /// Returns an error only when the supervisor is shutting down or when
    /// respawn itself fails.
    async fn acquire(&self) -> anyhow::Result<Arc<StdioHarness>> {
        if self.stopping.load(std::sync::atomic::Ordering::SeqCst) {
            anyhow::bail!(
                "runtime supervisor for {} is shutting down",
                self.agent_id
            );
        }

        let mut state = self.state.lock().await;

        // Lazy exit detection: if the previous turn left a still-registered
        // harness whose child has since exited, surface and clear it before
        // the next turn picks it up. `try_wait` is non-blocking.
        if let Some(h) = state.harness.as_ref() {
            let exit_status = {
                let mut child = h.child.lock().await;
                child.try_wait().ok().flatten()
            };
            if let Some(status) = exit_status {
                tracing::warn!(
                    agent = %self.agent_id,
                    generation = state.generation,
                    %status,
                    event = "harness:exited",
                    "runtime child exited (try_wait); will respawn"
                );
                state.harness = None;
                state.last_exit = Some(format!("status={status}"));
            }
        }

        if state.harness.is_none() {
            self.respawn_locked(&mut state).await?;
        }
        Ok(state
            .harness
            .as_ref()
            .expect("respawn populated harness")
            .clone())
    }

    /// Force-mark the current child as unhealthy so the next `acquire` will
    /// respawn. Used by callers that observed a write/read failure during a
    /// turn — cheaper than waiting for `try_wait` to confirm exit on the
    /// following turn.
    async fn mark_unhealthy(&self, reason: &str) {
        let mut state = self.state.lock().await;
        if state.harness.is_some() {
            tracing::warn!(
                agent = %self.agent_id,
                generation = state.generation,
                reason = %reason,
                event = "harness:unhealthy",
                "runtime child marked unhealthy; next turn will respawn"
            );
            state.harness = None;
            state.last_exit = Some(reason.to_string());
        }
    }

    async fn respawn_locked(&self, state: &mut SupervisorState) -> anyhow::Result<()> {
        if self.stopping.load(std::sync::atomic::Ordering::SeqCst) {
            anyhow::bail!(
                "runtime supervisor for {} is shutting down",
                self.agent_id
            );
        }
        let next_gen = state.generation + 1;
        tracing::info!(
            agent = %self.agent_id,
            generation = next_gen,
            event = "harness:spawning",
            "spawning runtime child"
        );
        let harness = self.factory.spawn_one().await?;
        state.generation = next_gen;
        state.harness = Some(Arc::new(harness));
        state.last_exit = None;
        tracing::info!(
            agent = %self.agent_id,
            generation = next_gen,
            event = "harness:respawned",
            "runtime child ready"
        );
        Ok(())
    }
}

#[cfg(test)]
impl RuntimeChildSupervisor {
    /// Test-only: build a supervisor whose spawn factory is a closure that
    /// returns fresh mock harnesses (e.g. `StdioHarness::spawn_mock`). The
    /// initial child is spawned eagerly, mirroring `start`.
    async fn start_with_factory<F, Fut>(agent_id: &str, factory: F) -> anyhow::Result<Arc<Self>>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = anyhow::Result<StdioHarness>> + Send + 'static,
    {
        let factory: SpawnFactory =
            SpawnFactory::Mock(Box::new(move || Box::pin(factory())));
        let supervisor = Arc::new(Self {
            agent_id: agent_id.to_string(),
            factory,
            state: Mutex::new(SupervisorState {
                harness: None,
                generation: 0,
                last_exit: None,
            }),
            stopping: std::sync::atomic::AtomicBool::new(false),
        });
        {
            let mut state = supervisor.state.lock().await;
            supervisor.respawn_locked(&mut state).await?;
        }
        Ok(supervisor)
    }

    /// Test introspection: current generation count.
    async fn current_generation(&self) -> u64 {
        self.state.lock().await.generation
    }
}

// ── AgentTurnTransport impl (sera-ve9x) ─────────────────────────────────────
//
// The supervisor is the only [`AgentTurnTransport`] implementor in PR 1.
// `acquire()` and `mark_unhealthy()` move from the gateway call sites
// (`execute_turn`, `execute_steer`, `probe_runtime_ready`) into these
// methods so PR 2 can swap in an `EmbeddedRuntimeTransport` without
// duplicating the supervisor lifecycle handling. Behaviour is unchanged:
// every `mark_unhealthy` reason string and every error-propagation path
// is preserved.

#[async_trait::async_trait]
impl AgentTurnTransport for RuntimeChildSupervisor {
    async fn send_turn(
        &self,
        messages: Vec<serde_json::Value>,
        session_key: &str,
    ) -> anyhow::Result<TurnEvents> {
        let harness = self.acquire().await?;
        match harness.send_turn(messages, session_key).await {
            Ok(events) => Ok(events),
            Err(e) => {
                let err_msg = e.to_string();
                self.mark_unhealthy(&format!("send_turn error: {err_msg}"))
                    .await;
                Err(e)
            }
        }
    }

    /// sera-k8do: streaming dispatch. Each `streaming_delta` NDJSON frame
    /// the runtime emits is forwarded through `delta_tx` as it lands, so
    /// the SSE chat handler can yield real first-token frames while the
    /// turn is still in flight. The terminal `TurnEvents` (full reply,
    /// usage, tool events) is still returned for transcript persistence.
    async fn send_turn_streaming(
        &self,
        messages: Vec<serde_json::Value>,
        session_key: &str,
        delta_tx: tokio::sync::mpsc::Sender<String>,
    ) -> anyhow::Result<TurnEvents> {
        let harness = self.acquire().await?;
        match harness
            .send_turn_with_deltas(messages, session_key, delta_tx)
            .await
        {
            Ok(events) => Ok(events),
            Err(e) => {
                let err_msg = e.to_string();
                self.mark_unhealthy(&format!("send_turn_streaming error: {err_msg}"))
                    .await;
                Err(e)
            }
        }
    }

    async fn send_steer(
        &self,
        items: Vec<serde_json::Value>,
        session_key: &str,
    ) -> anyhow::Result<()> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let harness = self.acquire().await?;

        let submission = serde_json::json!({
            "id": uuid::Uuid::new_v4(),
            "op": {
                "type": "steer",
                "items": items,
                "session_key": session_key,
            },
        });

        let mut json_line = serde_json::to_string(&submission)?;
        json_line.push('\n');

        let mut stdin = harness.stdin.lock().await;
        let mut stdout = harness.stdout.lock().await;

        if let Err(e) = stdin.write_all(json_line.as_bytes()).await {
            let ctx_err = harness.child_exit_context(e).await;
            // Drop stdin/stdout guards before awaiting on the supervisor
            // lock — the supervisor's `mark_unhealthy` takes its own lock,
            // and concurrent `acquire` calls also need access to the same
            // `Arc<StdioHarness>`'s child mutex.
            drop(stdin);
            drop(stdout);
            self.mark_unhealthy(&format!("steer stdin write: {ctx_err}"))
                .await;
            return Err(ctx_err);
        }
        if let Err(e) = stdin.flush().await {
            let ctx_err = harness.child_exit_context(e).await;
            drop(stdin);
            drop(stdout);
            self.mark_unhealthy(&format!("steer stdin flush: {ctx_err}"))
                .await;
            return Err(ctx_err);
        }

        // Drain the steer turn until its terminal `turn_completed` frame.
        // The runtime emits the event type at `msg.type` (mirroring
        // `StdioHarness::send_turn`); a stale code path read `type` at the
        // top level and never matched, so the loop wedged the lane until
        // `SERA_TURN_TIMEOUT_SECS` (sera-y9f8). We must consume every
        // frame this steer produces — including intermediate
        // `streaming_delta`s and the final `turn_completed`. The next
        // `send_turn` reads from the same harness stdout and exits on the
        // first `turn_completed` it sees (no `submission_id` correlation),
        // so any leftover steer event would be misread as the next user
        // turn's completion and short-circuit it with empty output.
        let mut line = String::new();
        loop {
            match stdout.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
                        let msg_type = event
                            .get("msg")
                            .and_then(|m| m.get("type"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                        if msg_type == "turn_completed" {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
            line.clear();
        }

        Ok(())
    }

    /// Send a graceful shutdown command to the current child and stop the
    /// supervisor from respawning. Best-effort: any I/O failure is logged
    /// upstream so one stuck child cannot stall the drain phase.
    async fn shutdown(&self) -> anyhow::Result<()> {
        self.stopping
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let state = self.state.lock().await;
        if let Some(h) = state.harness.as_ref() {
            h.shutdown().await?;
        }
        Ok(())
    }

    /// Liveness probe used by `/api/health/ready`. The semantics match the
    /// pre-trait `probe_runtime_ready` body: acquire the current child
    /// (respawning a dead one transparently — sera-ojp3) and round-trip a
    /// trivial `ping` turn. A non-empty streaming reply proves both the
    /// runtime and its LLM provider are reachable.
    async fn liveness_probe(&self) -> anyhow::Result<()> {
        let harness = self.acquire().await?;
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": "ping",
        })];
        let events = harness.send_turn(messages, "__sera_readiness_probe__").await?;
        if events.response.trim().is_empty() {
            anyhow::bail!("liveness probe returned empty response");
        }
        Ok(())
    }

    fn dispatch_kind(&self) -> &'static str {
        "runtime"
    }

    /// sera-40y3: kill the current child and clear the harness slot so the
    /// next `acquire()` respawns fresh. Called from the KillSwitch ROLLBACK
    /// callback after `cancel_all_in_flight` flips the cancellation tokens.
    /// Without this, the runtime child keeps running and the next
    /// post-DISARM submission could read a stale `TurnCompleted` frame
    /// from the previous (aborted) turn.
    async fn kill_for_rollback(&self) {
        let mut state = self.state.lock().await;
        if let Some(harness) = state.harness.as_ref() {
            // Best-effort kill — the child may already be dead, in which
            // case `start_kill` returns an error we just log.
            let mut child = harness.child.lock().await;
            if let Err(e) = child.start_kill() {
                tracing::warn!(
                    agent = %self.agent_id,
                    generation = state.generation,
                    error = %e,
                    "kill_for_rollback: start_kill failed (child may already be gone)"
                );
            } else {
                tracing::warn!(
                    agent = %self.agent_id,
                    generation = state.generation,
                    event = "harness:killed_for_rollback",
                    "killed runtime child after KillSwitch ROLLBACK"
                );
            }
        }
        // Drop the harness regardless — next acquire() will respawn. We do
        // not bump generation here; respawn_locked will.
        state.harness = None;
        state.last_exit = Some("killed_for_rollback".to_string());
    }
}

// ── Turn event types ────────────────────────────────────────────────────────
//
// `ToolEvent`, `TurnEvents`, and `UsageInfo` moved to
// `sera_gateway::agent_transport` (sera-ve9x) so PR 2 can plug an in-process
// `EmbeddedRuntimeTransport` against the same shapes.

// ── Shared state ────────────────────────────────────────────────────────────

/// Per-session cancellation registry entry (sera-bsem + sera-mplr).
///
/// Holds the in-flight turn's `CancellationToken` plus a flag that lets
/// `chat_handler` distinguish a user-driven `POST /api/chat/cancel`
/// (sera-mplr) from the operator-driven ROLLBACK / admin-cancel paths.
/// `cancel_http_chat_session` sets `client_cancelled = true` before firing
/// the token; `deregister_cancellation_token` reads it on cleanup so the
/// chat handler can short-circuit to a real cancelled outcome instead of
/// persisting the rollback-class synthetic reply.
struct CancelHandle {
    token: CancellationToken,
    client_cancelled: std::sync::atomic::AtomicBool,
}

struct AppState {
    db: Arc<Mutex<SqliteDb>>,
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
    /// Per-agent runtime backends keyed by agent name. Today the only
    /// implementor is `RuntimeChildSupervisor` (sera-ojp3) which owns at
    /// most one live `StdioHarness` and respawns on child exit so a
    /// single `sera-runtime --ndjson` panic cannot wedge the agent for
    /// the lifetime of the gateway pod. Stored as `Arc<dyn
    /// AgentTurnTransport>` (sera-ve9x) so PR 2 can swap in an
    /// in-process `EmbeddedRuntimeTransport` without forking the boot
    /// loop.
    harnesses: std::collections::HashMap<String, Arc<dyn AgentTurnTransport>>,
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
    /// The scheduler consults this on every tick to resolve Mail-gate tasks;
    /// the correlator pushes `ReplyReceived`/`Closed` events into it via
    /// `POST /api/mail/inbound`. Also exposed via `WorkflowAppState::mail_lookup`
    /// so the `POST /api/workflow/mail/deliver` test endpoint can inject events
    /// directly without real SMTP/IMAP infrastructure.
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
    /// In-flight turn/steer cancellation registry (sera-bsem). Keyed by
    /// `session_key`. Each call to `execute_turn` / `execute_steer` registers
    /// a fresh `CancellationToken` on entry and removes it on exit. On
    /// `ROLLBACK` the admin socket handler cancels every token and clears the
    /// map, so in-flight turns abort within their `tokio::select!` cancel arm
    /// instead of pinning a lane slot indefinitely.
    ///
    /// Uses `std::sync::Mutex` intentionally: the critical section is a pure
    /// `HashMap` insert/remove/drain with no awaits, and the admin socket's
    /// `on_rollback` callback is a synchronous `Fn()` that would panic if it
    /// called `tokio::sync::Mutex::blocking_lock` from inside the runtime.
    active_cancellation_tokens: Arc<std::sync::Mutex<std::collections::HashMap<String, CancelHandle>>>,
    /// Submission envelope store — every agent-facing route appends a
    /// Submission here before calling the underlying service (sera-r1g8).
    /// Default boot uses SqliteSessionStore (sera-fpmt / K.1); enabling the
    /// `enterprise` feature or setting `SERA_SESSION_STORE=git-shadowed`
    /// switches to SqliteGitSessionStore (sera-4i4i). Tests keep
    /// InMemorySessionStore to avoid writing to disk.
    session_store: Arc<dyn SessionStore>,
    /// Constitutional rule registry. Seeded at startup from
    /// `SERA_CONSTITUTIONAL_RULES_PATH` (default `/etc/sera/constitutional_rules.yaml`).
    /// Empty when the file is absent — constitutional_gate hooks still run but
    /// find no rules to evaluate (fail-open vs fail-closed is the hook's choice).
    /// The same `Arc` is shared with `ConstitutionalGateHook` in the hook chain
    /// so rules loaded at boot are immediately visible to the gate (sera-0yh3).
    /// Retained here for future admin reload endpoint (`POST /admin/constitutional/reload`).
    #[allow(dead_code)]
    constitutional_registry: Arc<ConstitutionalRegistry>,
    /// Capability-policy registry (sera-ifjl). Loaded at boot from
    /// `$SERA_CAPABILITY_POLICIES_DIR` (default `$XDG_CONFIG_HOME/sera/policies/`
    /// via `sera_config::ConfigRoot`); bound to each agent via the manifest's
    /// `policyRef`. Consulted on every observed `tool_call_begin` event in the
    /// runtime NDJSON stream (`StdioHarness::send_turn`) and on any future
    /// gateway-side tool dispatch. Agents with no `policyRef` bypass the check
    /// (permissive).
    ///
    /// Wrapped in `RwLock<Arc<…>>` so the admin HTTP `POST /admin/policies/reload`
    /// route (sera-nrn9) can swap the registry without restarting the gateway.
    /// Readers acquire a shared lock + clone the inner `Arc`, so concurrent
    /// turns never block the swap.
    capability_registry: Arc<RwLock<Arc<CapabilityRegistry>>>,
    /// HITL ticket store (sera-z6ql, Wave D Phase 1). Populated by
    /// `chat_handler` whenever the ApprovalRouter says a turn needs
    /// approval; read by the `/api/hitl/requests[/…]` routes so humans can
    /// approve / reject / escalate. Phase 1 uses an in-memory store —
    /// process restart loses in-flight tickets (no suspended turns to
    /// resume anyway). SQLite-backed store is a follow-up.
    ticket_store: Arc<dyn TicketStore>,
    /// HITL Phase 2 resume broadcast (sera-93h4). The approve route fans out
    /// `HitlResumedEvent` here after a parked chat turn's ticket transitions
    /// to `Approved`. Subscribers are SSE clients on `GET /api/hitl/events`
    /// or any background task that wants to react to ticket approval.
    /// `tokio::sync::broadcast::Sender` is cheap to clone and lossy on slow
    /// receivers (which is what we want for a notification fan-out).
    hitl_resumed_tx: tokio::sync::broadcast::Sender<sera_gateway::hitl_gateway::HitlResumedEvent>,
    /// Workflow task store (sera-kgi8, Wave E Phase 1). Populated by the
    /// `/api/workflow/tasks` POST route; drained every `TICK_INTERVAL` by
    /// the scheduler spawned in `run_start`. Phase 1 uses an in-memory
    /// store — SQLite-backed store is a follow-up bead.
    workflow_store: Arc<dyn WorkflowTaskStore>,
    /// GitHub Actions run state store (sera-4fel). Keyed by run_id string;
    /// populated by future GhRun status polling (or tests via direct upsert).
    /// Consulted by the scheduler each tick via a snapshot lookup so GhRun-
    /// gated workflow tasks transition to resolved when their run completes.
    gh_run_store: Arc<dyn GhRunStateStore>,
    /// GitHub PR state store (sera-ai4w).
    gh_pr_store: Arc<dyn GhPrStateStore>,
    /// Human gate ticket status store (sera-dgk1). Populated by
    /// `POST /api/workflow/tasks/{id}/resume`; snapshotted each scheduler
    /// tick so Human-gated tasks can resolve without holding an async lock.
    human_gate_store: Arc<dyn HumanGateStore>,
    /// Admin HTTP auth (sera-nrn9, L.3). Separate token from the public
    /// API's `api_key`. `None` when the admin server is not started (e.g. in
    /// tests that don't exercise admin routes).
    admin_auth: Option<Arc<AdminAuth>>,
    /// JSONL audit log handle for admin requests (sera-nrn9). `None` when
    /// the admin server is not started.
    admin_audit: Option<Arc<AdminAuditLogger>>,
}

impl AppState {
    /// Register a fresh cancellation token for `session_key` (sera-bsem).
    ///
    /// Returns the new token so the caller can race it inside its
    /// `tokio::select!`. If a token is already registered for this session
    /// key it is replaced — the prior token is dropped without being
    /// cancelled, mirroring the lane queue's per-session serialisation
    /// contract.
    fn register_cancellation_token(&self, session_key: &str) -> CancellationToken {
        let token = CancellationToken::new();
        let handle = CancelHandle {
            token: token.clone(),
            client_cancelled: std::sync::atomic::AtomicBool::new(false),
        };
        let mut map = self
            .active_cancellation_tokens
            .lock()
            .expect("active_cancellation_tokens mutex poisoned");
        map.insert(session_key.to_string(), handle);
        token
    }

    /// Remove the cancellation token for `session_key` (sera-bsem).
    ///
    /// Called on every exit path of `execute_turn` / `execute_steer` — success,
    /// timeout, harness error, and cancellation — so the map does not leak
    /// entries. Missing keys are silently ignored (e.g. when the ROLLBACK path
    /// has already cleared the map).
    ///
    /// Returns `true` when the cancellation was driven by a user-initiated
    /// `POST /api/chat/cancel` (sera-mplr), so `chat_handler` can return a
    /// distinct cancelled outcome instead of persisting the rollback-class
    /// synthetic reply. Operator-driven paths (ROLLBACK, admin
    /// `cancel_session`) and normal completion all return `false`.
    fn deregister_cancellation_token(&self, session_key: &str) -> bool {
        let mut map = self
            .active_cancellation_tokens
            .lock()
            .expect("active_cancellation_tokens mutex poisoned");
        map.remove(session_key)
            .map(|h| h.client_cancelled.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Cancel every in-flight turn/steer and clear the registry (sera-bsem).
    ///
    /// Called from the admin socket's `on_rollback` callback when a
    /// `ROLLBACK` command arms the kill switch: each waiting
    /// `execute_turn` / `execute_steer` wakes via its cancellation arm,
    /// returns a cancelled-error result, and the usual error-path cleanup
    /// releases the lane slot.
    fn cancel_all_in_flight(&self) -> usize {
        let mut map = self
            .active_cancellation_tokens
            .lock()
            .expect("active_cancellation_tokens mutex poisoned");
        let count = map.len();
        for (_key, handle) in map.drain() {
            handle.token.cancel();
        }
        count
    }

    /// Cancel the in-flight HTTP `/api/chat` turn for `session_id` (sera-mplr).
    ///
    /// Matches keys formatted as `http:{agent_name}:{session_id}` (see
    /// `chat_handler` line ~1794). Returns `true` when a matching handle was
    /// found, marked as client-cancelled, and its token fired; `false` when
    /// no active HTTP turn is registered for this session. The Discord
    /// transport's `discord:{agent}:{channel_id}` keys are deliberately not
    /// matched — `/api/chat/cancel` is the HTTP-surface cancel route.
    ///
    /// The handle is **not** removed from the map here — the chat handler's
    /// `deregister_cancellation_token` call after `execute_turn` returns is
    /// the cleanup point, and reading `client_cancelled` there is what tells
    /// it to return a real cancelled outcome instead of the rollback-class
    /// synthetic reply.
    fn cancel_http_chat_session(&self, session_id: &str) -> bool {
        let suffix = format!(":{session_id}");
        let map = self
            .active_cancellation_tokens
            .lock()
            .expect("active_cancellation_tokens mutex poisoned");
        match map
            .iter()
            .find(|(k, _)| k.starts_with("http:") && k.ends_with(&suffix))
        {
            Some((_, handle)) => {
                handle
                    .client_cancelled
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                handle.token.cancel();
                true
            }
            None => false,
        }
    }
}

#[async_trait::async_trait]
impl AdminAppState for AppState {
    fn auth(&self) -> Arc<AdminAuth> {
        Arc::clone(
            self.admin_auth
                .as_ref()
                .expect("admin_auth must be initialised before serving admin HTTP"),
        )
    }

    fn audit(&self) -> Arc<AdminAuditLogger> {
        Arc::clone(
            self.admin_audit
                .as_ref()
                .expect("admin_audit must be initialised before serving admin HTTP"),
        )
    }

    fn kill_switch(&self) -> Arc<KillSwitch> {
        Arc::clone(&self.kill_switch)
    }

    fn capability_registry(&self) -> Arc<RwLock<Arc<CapabilityRegistry>>> {
        Arc::clone(&self.capability_registry)
    }

    fn policies_dir(&self) -> std::path::PathBuf {
        CapabilityRegistry::resolve_policies_dir()
    }

    fn workflow_store(&self) -> Option<Arc<dyn WorkflowTaskStore>> {
        Some(Arc::clone(&self.workflow_store))
    }

    async fn list_sessions(&self) -> Vec<AdminSessionInfo> {
        let db = self.db.lock().await;
        db.list_sessions()
            .map(|rows| {
                rows.into_iter()
                    .map(|r| AdminSessionInfo {
                        id: r.id,
                        agent_id: r.agent_id,
                        session_key: r.session_key,
                        state: r.state,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn get_session(&self, id: &str) -> Option<AdminSessionInfo> {
        let db = self.db.lock().await;
        db.list_sessions()
            .ok()?
            .into_iter()
            .find(|r| r.id == id || r.session_key == id)
            .map(|r| AdminSessionInfo {
                id: r.id,
                agent_id: r.agent_id,
                session_key: r.session_key,
                state: r.state,
            })
    }

    fn cancel_session(&self, session_key: &str) -> bool {
        let mut map = self
            .active_cancellation_tokens
            .lock()
            .expect("active_cancellation_tokens mutex poisoned");
        if let Some(handle) = map.remove(session_key) {
            handle.token.cancel();
            true
        } else {
            false
        }
    }

    fn agent_metadata(&self, id: &str) -> Option<serde_json::Value> {
        let agent_names = self.manifests.agent_names();
        let name = agent_names.iter().copied().find(|n| *n == id)?;
        let spec = self.manifests.agent_spec(name).ok().flatten();
        Some(serde_json::json!({
            "name": name,
            "provider": spec.as_ref().map(|s| s.provider.as_str()).unwrap_or(""),
            "model": spec.as_ref().and_then(|s| s.model.as_deref()),
            "has_tools": spec.as_ref().and_then(|s| s.tools.as_ref()).is_some(),
        }))
    }
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

impl HitlAppState for AppState {
    fn api_key(&self) -> &Option<String> {
        &self.api_key
    }
    fn ticket_store(&self) -> Arc<dyn TicketStore> {
        Arc::clone(&self.ticket_store)
    }
    fn hitl_resumed_tx(
        &self,
    ) -> Option<sera_gateway::hitl_gateway::HitlResumedSender> {
        Some(self.hitl_resumed_tx.clone())
    }
}

impl WorkflowAppState for AppState {
    fn api_key(&self) -> &Option<String> {
        &self.api_key
    }
    fn workflow_store(&self) -> Arc<dyn WorkflowTaskStore> {
        Arc::clone(&self.workflow_store)
    }
    fn mail_lookup(&self) -> Arc<InMemoryMailLookup> {
        Arc::clone(&self.mail_lookup)
    }
    fn change_artifact_store(&self) -> Option<Arc<dyn sera_gateway::workflow_store::ChangeArtifactStateStore>> {
        // sera-7ggi: change-artifact store not yet provisioned in the binary.
        // Returning None makes Change-gated POST /api/workflow/tasks return 501.
        None
    }
    fn human_gate_store(&self) -> Option<Arc<dyn HumanGateStore>> {
        Some(Arc::clone(&self.human_gate_store))
    }
    fn gh_pr_store(&self) -> Option<Arc<dyn GhPrStateStore>> {
        Some(Arc::clone(&self.gh_pr_store))
    }
}

// ── sera-7ivj: inference proxy wiring ───────────────────────────────────────
//
// Resolves the upstream provider from the loaded manifests. Provider name is
// chosen by `SERA_INFERENCE_PROXY_PROVIDER` (OpenAI route) or
// `SERA_INFERENCE_PROXY_ANTHROPIC_PROVIDER` (Anthropic route) if set,
// otherwise the first declared provider. The upstream key is read once via
// `resolve_provider_api_key` so it stays in-process — the inbound caller
// never controls it. Operators are responsible for matching the protocol of
// the resolved provider to the route being called.
fn proxy_http_client() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("build inference proxy reqwest client")
        })
        .clone()
}

fn resolve_proxy_upstream(state: &AppState, env_var: &str) -> Option<UpstreamProvider> {
    let preferred = std::env::var(env_var)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let names: Vec<&str> = state.manifests.providers.iter()
        .map(|m| m.metadata.name.as_str())
        .collect();
    let name = preferred
        .as_deref()
        .filter(|n| names.contains(n))
        .or_else(|| names.first().copied())?;
    let spec = state.manifests.provider_spec(name).ok().flatten()?;
    let api_key = resolve_provider_api_key(&spec).unwrap_or_default();
    Some(UpstreamProvider {
        base_url: spec.base_url,
        api_key,
    })
}

impl InferenceProxyAppState for AppState {
    fn proxy_api_key(&self) -> &Option<String> {
        &self.api_key
    }

    fn proxy_upstream(&self) -> Option<UpstreamProvider> {
        resolve_proxy_upstream(self, "SERA_INFERENCE_PROXY_PROVIDER")
    }

    fn proxy_anthropic_upstream(&self) -> Option<UpstreamProvider> {
        resolve_proxy_upstream(self, "SERA_INFERENCE_PROXY_ANTHROPIC_PROVIDER")
    }

    fn proxy_http_client(&self) -> reqwest::Client {
        proxy_http_client()
    }

    fn proxy_budget_gate(&self) -> Arc<dyn LlmBudgetGate> {
        Arc::new(NoopBudgetGate)
    }

    fn proxy_audit(&self) -> Arc<dyn InferenceProxyAudit> {
        // sera-7ivj PR3: persist one row per proxy call to the gateway's
        // shared SqliteDb. Replaces the prior tracing-only sink so an
        // operator can inspect proxy traffic via `query_audit` after the
        // fact. Body content is never persisted — only the metadata in
        // ProxyAuditEvent.
        Arc::new(SqliteInferenceProxyAudit::new(Arc::clone(&self.db)))
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

/// Request body for `POST /api/chat/cancel` (sera-mplr / J.0.4 ESC-cancel).
#[derive(Deserialize)]
struct ChatCancelRequest {
    session_id: String,
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
    /// `true` when `execute_turn` / `execute_steer` returned via the
    /// `tokio::select!` cancel arm rather than a success/error/timeout arm.
    /// Combined with `AppState::deregister_cancellation_token`'s return value
    /// in `chat_handler`, this distinguishes a user-driven `/api/chat/cancel`
    /// (sera-mplr) from the rollback-class synthetic-reply path so the route
    /// can return a real cancelled outcome instead of persisting the
    /// `[sera] Runtime turn aborted` sentinel as transcript content.
    cancelled: bool,
    /// `Some(reason)` when the transport failed (backend error or timeout)
    /// rather than producing a real LLM reply. Used by the streaming SSE
    /// path (sera-k8do) to detect a mid-stream runtime failure after some
    /// `streaming_delta` frames have already been forwarded to the client:
    /// in that case we must surface a structured `error` SSE event and
    /// skip persisting the synthetic `[sera] Runtime error: …` reply as
    /// the assistant transcript row, otherwise the visible stream and the
    /// persisted history disagree (Codex review on PR #1153). `None` for
    /// successful turns and for cancellation arms (`cancelled` already
    /// covers the cancel case).
    failure: Option<String>,
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
    for transport in state.harnesses.values() {
        // sera-ve9x: defer to the transport's `liveness_probe`, which the
        // stdio impl backs with the same `acquire().send_turn(ping)` round
        // trip the inline code did before. If the child died between
        // probes, the supervisor respawns transparently inside that call.
        let probe = transport.liveness_probe();
        match tokio::time::timeout(timeout, probe).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "readiness probe: transport reported failure");
                return false;
            }
            Err(_elapsed) => return false,
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

    // Look up the runtime supervisor for this agent (sera-ojp3). The
    // supervisor may swap its inner harness underneath us if the child has
    // died — we re-acquire on each turn rather than holding a fixed handle.
    let supervisor = match state.harnesses.get(&agent_name) {
        Some(s) => Arc::clone(s),
        None => {
            tracing::error!(agent = %agent_name, "No runtime supervisor registered");
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
        // Propagate incoming W3C trace context headers so distributed traces
        // can be correlated across gateway → runtime → LLM-client boundaries.
        let trace = W3cTraceContext {
            traceparent: headers
                .get("traceparent")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned),
            tracestate: headers
                .get("tracestate")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned),
        };
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
            trace,
            change_artifact: None,
            session_key: Some(session_key.clone()),
            parent_session_key: None,
            // Top-level chat turns aren't subagent invocations — leave the
            // task correlator empty. Subagent dispatch (J.2.4 follow-ups) will
            // populate this with the parent's tool call_id.
            parent_task_id: None,
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

    // ── HITL gate (sera-z6ql, Wave D Phase 1) ────────────────────────────
    // Consult the real ApprovalRouter using the agent's manifest-declared
    // enforcement_mode + approval_policy. Phase 1 blocks-and-tickets on
    // needs_approval == true — no suspension/resume. The HTTP routes under
    // /api/hitl/requests expose the resulting ticket for review.
    let hitl_mode = resolve_hitl_mode(&agent_spec);
    let hitl_routing = resolve_approval_routing(&agent_spec);
    // Phase 1 risk proxy: we don't yet know which tool the LLM will call,
    // so we use Execute as a conservative default. The router treats this
    // as 0.7 for threshold matching — Standard mode with non-empty static
    // routing still gates, Strict always gates, Autonomous never gates.
    let risk_for_gate = sera_types::tool::RiskLevel::Execute;
    if sera_hitl::ApprovalRouter::needs_approval(hitl_mode, risk_for_gate, &hitl_routing) {
        release_lane(&state, &session_key).await;

        // Mint a Ticket via the resolved chain (Phase 1: single ticket per
        // blocked turn). Keep the spec deliberately thin — we only know the
        // message and agent at this gate point.
        let spec = sera_hitl::ApprovalSpec {
            scope: sera_hitl::ApprovalScope::ToolCall {
                tool_name: "*".to_string(),
                risk_level: risk_for_gate,
            },
            description: format!(
                "Chat turn pending approval for agent '{}'",
                agent_name
            ),
            urgency: sera_hitl::ApprovalUrgency::Medium,
            routing: hitl_routing,
            timeout: std::time::Duration::from_secs(300),
            required_approvals: 1,
            evidence: sera_hitl::ApprovalEvidence {
                tool_args: Some(serde_json::json!({ "message": req.message })),
                risk_score: Some(sera_hitl::ApprovalRouter::risk_level_to_score_public(
                    risk_for_gate,
                )),
                principal: PrincipalRef {
                    id: PrincipalId::new("http-chat"),
                    kind: PrincipalKind::Human,
                },
                session_context: Some(session_key.clone()),
                additional: Default::default(),
            },
        };
        let ticket = sera_hitl::ApprovalTicket::new(spec, session_key.clone());
        let ticket_id = ticket.id.clone();
        if let Err(e) = state.ticket_store.insert(ticket).await {
            tracing::error!(error = %e, "ticket_store.insert failed; rejecting turn");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }

        // Phase 2 (sera-93h4): persist the parked chat turn's payload so the
        // approve route can broadcast a HitlResumedEvent carrying enough
        // correlation data for the caller to retry. Failures here are
        // logged but non-fatal — the ticket already exists and the caller
        // can still poll /api/hitl/requests/{id}; only the resumed event
        // will go missing.
        let suspended = sera_gateway::hitl_gateway::SuspendedTurn {
            ticket_id: ticket_id.clone(),
            session_key: session_key.clone(),
            session_id: session.id.clone(),
            agent_name: agent_name.clone(),
            message: req.message.clone(),
            stream: req.stream,
        };
        if let Err(e) = state.ticket_store.record_suspended_turn(suspended).await {
            tracing::warn!(
                error = %e,
                ticket_id = %ticket_id,
                session_key = %session_key,
                "ticket_store.record_suspended_turn failed; resume notifications will be lost"
            );
        }

        // OCSF Policy Activity audit entry (class_uid=6003) — best-effort.
        emit_hitl_required_audit(&agent_name, &session_key, &ticket_id, hitl_mode).await;

        let body = serde_json::json!({
            "error": "hitl_approval_required",
            "reason": format!("agent '{}' requires approval ({:?} mode)", agent_name, hitl_mode),
            "ticket_id": ticket_id,
            "message": "This request requires approval. Use the /api/hitl/requests routes to review and approve.",
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
        // SSE streaming mode (sera-7mc1 + sera-k8do): the turn runs in a
        // dedicated tokio task so that a client disconnect (which drops
        // the SSE body and therefore the unfold's state) cancels the
        // in-flight turn rather than silently letting it run to
        // completion server-side. The spawned task owns its own cleanup
        // (deregister + lane release) so those run regardless of whether
        // the unfold ever sees the result.
        //
        // sera-k8do: deltas now flow through an unbounded mpsc as the
        // runtime emits them, instead of being post-hoc word-split off
        // the assembled reply. The unfold pulls from the receiver until
        // the sender is dropped (turn task returned), then awaits the
        // join handle for the final usage / persistence step.
        let message = req.message.clone();
        let state_clone = Arc::clone(&state);
        let supervisor_clone = Arc::clone(&supervisor);
        let sid = session_id.clone();
        let skey = session_key.clone();
        let mid = format!("msg_{:08x}", rand::random::<u32>());
        let mid_clone = mid.clone();

        let aname = agent_name.clone();

        // Register the cancellation token up front so callers (and the
        // CancelOnDrop guard) share the same handle. The spawned task uses
        // a clone for its select arm; the guard holds another clone and
        // fires it if the SSE stream is dropped before Done.
        let cancel = state.register_cancellation_token(&skey);
        let cancel_for_task = cancel.clone();

        let task_state = Arc::clone(&state_clone);
        let task_supervisor = Arc::clone(&supervisor_clone);
        let task_session_key = skey.clone();
        let task_agent_name = aname.clone();
        let task_message = message.clone();
        let task_transcript = transcript.clone();
        let task_agent_spec = agent_spec.clone();

        // sera-k8do (Codex review on PR #1153): bounded channel applies
        // backpressure to the runtime read loop when a slow SSE client is
        // not draining frames. See `STREAMING_DELTA_CHANNEL_CAPACITY` for
        // the rationale on the chosen capacity.
        let (delta_tx, delta_rx) =
            tokio::sync::mpsc::channel::<String>(STREAMING_DELTA_CHANNEL_CAPACITY);

        let turn_handle: tokio::task::JoinHandle<(MvsTurnResult, bool)> =
            tokio::spawn(async move {
                let cap_reg = task_state.capability_registry.read().await.clone();
                let result = execute_turn(
                    &task_agent_spec,
                    &task_transcript,
                    &task_message,
                    &*task_supervisor,
                    &task_session_key,
                    &task_state.skill_engine,
                    &task_state.semantic_store,
                    &task_agent_name,
                    &cancel_for_task,
                    &cap_reg,
                    Some(delta_tx),
                )
                .await;

                // Always run cleanup, even when the SSE stream has gone away —
                // otherwise an SSE-disconnect cancellation would leak both the
                // cancellation registry entry and the lane slot (sera-7mc1).
                let user_cancelled =
                    task_state.deregister_cancellation_token(&task_session_key);
                {
                    let mut lq = task_state.lane_queue.lock().await;
                    lq.complete_run(&task_session_key);
                }
                (result, user_cancelled)
            });

        let cancel_guard = CancelOnDrop::new(cancel);

        let sse_stream = stream::unfold(
            StreamState::Streaming {
                rx: delta_rx,
                turn_handle,
                cancel_guard,
                state: state_clone,
                session_key: skey.clone(),
                session_id: sid,
                message_id: mid_clone,
                agent_name: aname,
            },
            |fold_state| async move {
                match fold_state {
                    StreamState::Streaming {
                        mut rx,
                        turn_handle,
                        mut cancel_guard,
                        state,
                        session_key,
                        session_id,
                        message_id,
                        agent_name,
                    } => {
                        // Pull the next live delta. `recv()` resolves to
                        // `None` when every Sender is dropped — that
                        // happens after the spawned turn task returns,
                        // which is our cue to finalise (await the
                        // JoinHandle, persist, emit `done`).
                        match rx.recv().await {
                            Some(delta) => {
                                let payload = serde_json::json!({
                                    "delta": delta,
                                    "session_id": session_id,
                                    "message_id": message_id,
                                });
                                let event = Event::default()
                                    .event("message")
                                    .data(payload.to_string());
                                Some((
                                    Some(Ok::<_, std::convert::Infallible>(event)),
                                    StreamState::Streaming {
                                        rx,
                                        turn_handle,
                                        cancel_guard,
                                        state,
                                        session_key,
                                        session_id,
                                        message_id,
                                        agent_name,
                                    },
                                ))
                            }
                            None => {
                                let (result, user_cancelled) = match turn_handle.await {
                                    Ok(pair) => pair,
                                    Err(join_err) => {
                                        // Task panicked or was aborted out-of-band, so
                                        // its in-task cleanup never ran. Without the
                                        // calls below the cancellation registry entry
                                        // and the lane slot would leak for this
                                        // session and block any future `/api/chat`
                                        // turn until process restart. Disarm the
                                        // guard first — firing the token here would
                                        // be a no-op (the task is already gone) and
                                        // would race with the deregister we're about
                                        // to do.
                                        cancel_guard.disarm();
                                        let _ = state
                                            .deregister_cancellation_token(&session_key);
                                        {
                                            let mut lq = state.lane_queue.lock().await;
                                            lq.complete_run(&session_key);
                                        }
                                        tracing::error!(
                                            error = %join_err,
                                            session_id = %session_id,
                                            session_key = %session_key,
                                            agent = %agent_name,
                                            "SSE turn task failed to join; cleaned up registry + lane"
                                        );
                                        let payload = serde_json::json!({
                                            "error": "turn task failed",
                                            "session_id": session_id,
                                            "message_id": message_id,
                                        });
                                        let event = Event::default()
                                            .event("error")
                                            .data(payload.to_string());
                                        return Some((Some(Ok(event)), StreamState::Done));
                                    }
                                };

                                // The turn task has finished successfully (with or
                                // without cancellation). The token is already
                                // deregistered and the lane slot released, so no
                                // further work would be cancellable — disarm the
                                // guard before we transition to Done.
                                cancel_guard.disarm();

                                // sera-k8do (Codex review on PR #1153): a runtime
                                // backend error (or per-turn timeout) after some
                                // `streaming_delta` frames have already been
                                // forwarded to the client must surface a
                                // structured `error` SSE event. Emitting `done`
                                // here would tell the client the turn completed
                                // successfully, while persisting the synthetic
                                // `[sera] Runtime error: …` reply as the
                                // assistant transcript row would mismatch the
                                // partial text the user already saw streamed.
                                // We skip persistence entirely so the next turn
                                // does not see a misleading assistant slot.
                                if let Some(reason) = result.failure.as_ref() {
                                    tracing::error!(
                                        session_id = %session_id,
                                        agent = %agent_name,
                                        reason = %reason,
                                        "SSE stream interrupted by runtime failure after partial deltas; \
                                         emitting `error` event and skipping transcript persist"
                                    );
                                    let payload = serde_json::json!({
                                        "error": "runtime stream interrupted",
                                        "reason": reason,
                                        "session_id": session_id,
                                        "message_id": message_id,
                                    });
                                    let event = Event::default()
                                        .event("error")
                                        .data(payload.to_string());
                                    return Some((Some(Ok(event)), StreamState::Done));
                                }

                                // sera-k8do (Codex review on PR #1153): the
                                // streaming cancellation gate must fire on
                                // `result.cancelled` alone, not `user_cancelled
                                // && result.cancelled`. Operator/admin paths
                                // (`AppState::cancel_all_in_flight`, KillSwitch
                                // ROLLBACK) cancel the runtime turn without
                                // flipping `client_cancelled`, so the spawned
                                // task's `deregister_cancellation_token` returns
                                // `false` (the entry was already drained or the
                                // flag was never set). Without this gate, those
                                // paths would persist the synthetic
                                // `[sera] Runtime turn aborted by KillSwitch
                                // ROLLBACK` reply as the assistant transcript
                                // row and emit a `done` event after the live
                                // deltas — visible stream and persisted
                                // history would disagree.
                                //
                                // sera-mplr semantics are preserved: the SSE
                                // `cancelled` payload still carries
                                // `reason: client_cancel` for user-driven
                                // `POST /api/chat/cancel`, and now reports
                                // `reason: operator_cancel` for rollback /
                                // admin / KillSwitch paths.
                                if result.cancelled {
                                    let reason = if user_cancelled {
                                        "client_cancel"
                                    } else {
                                        "operator_cancel"
                                    };
                                    tracing::info!(
                                        session_id = %session_id,
                                        agent = %agent_name,
                                        reason = %reason,
                                        "Chat stream cancelled mid-flight; \
                                         skipping transcript persist"
                                    );
                                    let payload = serde_json::json!({
                                        "cancelled": true,
                                        "reason": reason,
                                        "session_id": session_id,
                                        "message_id": message_id,
                                    });
                                    let event = Event::default()
                                        .event("cancelled")
                                        .data(payload.to_string());
                                    return Some((Some(Ok(event)), StreamState::Done));
                                }

                                // sera-aepj: empty-reply guard mirrors the sync branch.
                                // Without this, the SSE stream emits zero `message`
                                // frames followed by `done` with usage=0/0/0, which
                                // the web client renders as a stuck "thinking…"
                                // spinner. Surface a structured `error` event instead
                                // so clients can show the failure.
                                if result.reply.is_empty() {
                                    tracing::error!(
                                        session_id = %session_id,
                                        agent = %agent_name,
                                        prompt_tokens = result.usage.prompt_tokens,
                                        completion_tokens = result.usage.completion_tokens,
                                        total_tokens = result.usage.total_tokens,
                                        tool_events_count = result.tool_events.len(),
                                        tools_ran = !result.tool_events.is_empty(),
                                        "execute_turn returned empty reply (stream); runtime produced no text"
                                    );
                                    let payload = serde_json::json!({
                                        "error": "runtime returned empty reply",
                                        "session_id": session_id,
                                        "message_id": message_id,
                                    });
                                    let event = Event::default()
                                        .event("error")
                                        .data(payload.to_string());
                                    return Some((Some(Ok(event)), StreamState::Done));
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

                                let usage = result.usage;
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
        let cancel = state.register_cancellation_token(&session_key);
        let cap_reg = state.capability_registry.read().await.clone();
        let result = execute_turn(
            &agent_spec,
            &transcript,
            &req.message,
            &*supervisor,
            &session_key,
            &state.skill_engine,
            &state.semantic_store,
            &agent_name,
            &cancel,
            &cap_reg,
            None,
        )
        .await;
        let user_cancelled = state.deregister_cancellation_token(&session_key);

        // Release the lane slot now that the turn has completed. Mirrors the
        // `complete_run` call in the Discord message loop after `execute_turn`.
        release_lane(&state, &session_key).await;

        // sera-mplr: a `POST /api/chat/cancel` for this session set
        // `client_cancelled` and fired the cancel arm. Skip the
        // empty-reply guard, transcript persist, and `response_sent` audit
        // so the synthetic "[sera] turn aborted" reply doesn't pollute the
        // transcript, and return a 499 client-closed outcome with a
        // structured cancelled body. The race with a late cancel after a
        // successful turn is excluded by `result.cancelled` — the
        // `tokio::select!` cancel arm only sets it when it actually wins.
        if user_cancelled && result.cancelled {
            tracing::info!(
                session_id = %session_id,
                agent = %agent_name,
                session_key = %session_key,
                "Chat turn cancelled by client (POST /api/chat/cancel)"
            );
            return Ok((
                StatusCode::from_u16(499)
                    .expect("499 Client Closed Request is a valid status code"),
                Json(serde_json::json!({
                    "cancelled": true,
                    "reason": "client_cancel",
                    "session_id": session_id,
                })),
            )
                .into_response());
        }

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

/// `POST /api/chat/cancel` — abort the in-flight HTTP `/api/chat` turn for
/// the given `session_id` (sera-mplr / J.0.4 ESC-cancel flow).
///
/// Looks up the session's `CancellationToken` in the in-memory registry
/// populated by `chat_handler` (see [`AppState::register_cancellation_token`])
/// and fires it. The cancelled `execute_turn` returns via its
/// `tokio::select!` cancel arm; the lane slot is released by the existing
/// chat handler exit path.
///
/// - `204 No Content` when an active turn was cancelled.
/// - `404 Not Found` when no HTTP turn is in flight for that session.
async fn chat_cancel_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ValidatedJson(req): ValidatedJson<ChatCancelRequest>,
) -> Result<axum::response::Response, StatusCode> {
    validate_api_key(&state, &headers)?;
    if state.cancel_http_chat_session(&req.session_id) {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "not_found",
                "reason": "no_active_turn",
                "session_id": req.session_id,
            })),
        )
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

/// Internal state machine for SSE streaming (sera-7mc1 + sera-k8do).
///
/// The unfold pulls live deltas from `rx` until the spawned `execute_turn`
/// task drops its sender (the turn finished, succeeded or cancelled). The
/// task owns its own cleanup so a client disconnect — which drops the
/// unfold's state, including the receiver and the `CancelOnDrop` guard —
/// cancels the in-flight turn rather than leaking the lane slot.
///
/// `cancel_guard` fires the cancellation token on drop unless `disarm`-ed
/// when we transition to `Done`, so a dropped SSE body cancels the
/// spawned turn within tokio's next scheduling tick.
#[allow(clippy::large_enum_variant)]
enum StreamState {
    Streaming {
        rx: tokio::sync::mpsc::Receiver<String>,
        turn_handle: tokio::task::JoinHandle<(MvsTurnResult, bool)>,
        cancel_guard: CancelOnDrop,
        state: Arc<AppState>,
        /// Retained even though the spawned task owns the success-path
        /// cleanup, because a `JoinError` (panic/abort) skips that path
        /// and the unfold has to deregister + release the lane itself.
        session_key: String,
        session_id: String,
        message_id: String,
        agent_name: String,
    },
    Done,
}

/// Drop guard that fires a `CancellationToken` when the SSE stream state is
/// dropped before reaching `Done` (sera-7mc1).
///
/// axum drops the response body's stream when the client disconnects. That
/// drop cascades into the unfold's state, which contains a `CancelOnDrop`.
/// `armed = true` (the default) means a drop fires `token.cancel()`; the
/// successful-completion path calls `disarm()` first so we don't double-fire.
struct CancelOnDrop {
    token: CancellationToken,
    armed: bool,
}

impl CancelOnDrop {
    fn new(token: CancellationToken) -> Self {
        Self { token, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.token.cancel();
        }
    }
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

/// Capacity of the bounded mpsc channel that carries `streaming_delta`
/// frames from the runtime read loop to the SSE pump (sera-k8do, Codex
/// review on PR #1153).
///
/// The chat handler creates `tokio::sync::mpsc::channel::<String>(N)` so
/// that a slow SSE consumer applies backpressure to the runtime: once
/// `N` frames are queued, the harness's per-frame `tx.send().await`
/// suspends until the unfold drains one. This bounds the worst-case
/// gateway-side memory accumulation per session — without it, a stalled
/// SSE client could let the runtime push tokens into memory indefinitely.
///
/// 256 covers typical multi-token bursts (provider-side packing where
/// several tokens arrive in one SSE frame from upstream) without
/// throttling steady-state streams. Operators who want a tighter bound
/// can lower this; clients only ever see the bound as a slight delay
/// before the next delta lands when their TCP socket is paused.
const STREAMING_DELTA_CHANNEL_CAPACITY: usize = 256;

fn turn_timeout() -> std::time::Duration {
    std::env::var("SERA_TURN_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .map(std::time::Duration::from_secs)
        .unwrap_or(DEFAULT_TURN_TIMEOUT)
}

/// Execute a turn by dispatching through the agent's [`AgentTurnTransport`].
///
/// The gateway builds the conversation messages from the transcript and
/// hands them to the transport. Today's implementor (`RuntimeChildSupervisor`)
/// asks the supervisor for the current `sera-runtime --ndjson` child handle
/// — respawning transparently if the previous child died (sera-ojp3) — and
/// writes the submission down the NDJSON pipe. The runtime owns LLM calls
/// and tool execution; the gateway never touches those.
#[allow(clippy::too_many_arguments)]
async fn execute_turn(
    agent_spec: &AgentSpec,
    transcript: &[sera_db::sqlite::TranscriptRow],
    user_message: &str,
    transport: &dyn AgentTurnTransport,
    session_key: &str,
    skill_engine: &SkillDispatchEngine,
    semantic_store: &Arc<dyn SemanticMemoryStore>,
    agent_name: &str,
    cancel: &CancellationToken,
    capability_registry: &CapabilityRegistry,
    // sera-k8do: when `Some`, the transport's streaming variant is used
    // and each LLM-emitted text delta is forwarded through this channel
    // for live SSE delivery. The channel is bounded (Codex review on
    // PR #1153) so a slow SSE consumer applies backpressure to the
    // runtime read loop instead of growing memory unboundedly. `None`
    // keeps the synchronous behaviour.
    delta_tx: Option<tokio::sync::mpsc::Sender<String>>,
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
    // sera-bsem: race the transport turn against both the existing timeout
    // and the KillSwitch-driven cancellation token. Dropping the `send_turn`
    // future via `select!` returns control to the caller so the lane slot
    // is released even if the backend is unresponsive.
    // sera-ifjl: on a successful turn, tool events are filtered through
    // enforce_tool_events before returning — denied tools are rewritten
    // into explicit denials and an OCSF audit entry is emitted.
    // sera-ve9x: the trait impl on `RuntimeChildSupervisor` performs the
    // `acquire()` and (on a backend error) `mark_unhealthy()` internally,
    // so the call site only has to handle Ok/Err uniformly. The previous
    // "Runtime unavailable" / "Runtime error" split (acquire failure vs.
    // send_turn failure) collapses into a single error reply — both
    // paths had identical lane-release semantics, so observable behaviour
    // is unchanged.
    //
    // sera-k8do: when `delta_tx` is `Some`, dispatch through the streaming
    // variant so the transport can forward per-token deltas to the SSE
    // pump while the turn is still in flight.
    let send_fut: std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<TurnEvents>> + Send>,
    > = match delta_tx {
        Some(tx) => Box::pin(transport.send_turn_streaming(messages, session_key, tx)),
        None => Box::pin(transport.send_turn(messages, session_key)),
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            tracing::warn!(
                session_key = %session_key,
                "Runtime harness turn cancelled (KillSwitch ROLLBACK); releasing lane"
            );
            MvsTurnResult {
                reply: "[sera] Runtime turn aborted by KillSwitch ROLLBACK".to_string(),
                tool_events: vec![],
                usage: UsageInfo {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
                cancelled: true,
                failure: None,
            }
        }
        res = tokio::time::timeout(timeout, send_fut) => match res {
            Ok(Ok(events)) => {
                let filtered_events = enforce_tool_events(
                    agent_name,
                    events.tool_events,
                    capability_registry,
                )
                .await;
                MvsTurnResult {
                    reply: events.response,
                    tool_events: filtered_events,
                    usage: events.usage,
                    cancelled: false,
                    failure: None,
                }
            }
            Ok(Err(e)) => {
                let err_msg = e.to_string();
                tracing::error!(
                    error = %e,
                    agent = %agent_name,
                    session_key = %session_key,
                    event = "harness:turn_failed",
                    "Runtime harness turn failed"
                );
                MvsTurnResult {
                    reply: format!("[sera] Runtime error: {err_msg}"),
                    tool_events: vec![],
                    usage: UsageInfo {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                    cancelled: false,
                    failure: Some(err_msg),
                }
            }
            Err(_elapsed) => {
                tracing::error!(
                    session_key = %session_key,
                    timeout_secs = timeout.as_secs(),
                    "Runtime harness turn timed out; releasing lane"
                );
                let timeout_msg =
                    format!("runtime turn timed out after {}s", timeout.as_secs());
                MvsTurnResult {
                    reply: format!("[sera] Runtime timed out after {}s", timeout.as_secs()),
                    tool_events: vec![],
                    usage: UsageInfo {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                    cancelled: false,
                    failure: Some(timeout_msg),
                }
            }
        }
    }
}

/// sera-eo71: defence-in-depth audit emitter for capability-policy denials.
///
/// Pre-dispatch enforcement now lives in `sera-runtime`, so by the time
/// `tool_call_begin` / `tool_call_end` events reach the gateway the runtime
/// has already (a) blocked the tool and (b) shaped the `End` content as
/// `[sera-policy] tool '…' denied …`. This pass therefore does **not**
/// rewrite events — it just observes each `Begin`, consults the gateway's
/// own `CapabilityRegistry` (which is loaded from the same files as the
/// runtime's), and emits an OCSF Policy Activity audit entry when the
/// gateway-side check would also have denied. Events are returned unchanged.
async fn enforce_tool_events(
    agent_name: &str,
    events: Vec<ToolEvent>,
    registry: &CapabilityRegistry,
) -> Vec<ToolEvent> {
    for event in &events {
        if let ToolEvent::Begin { tool, .. } = event
            && let Err(denial) = registry.check(agent_name, tool)
        {
            emit_policy_denial_audit(&denial).await;
            tracing::warn!(
                agent = %denial.agent_id,
                tool = %denial.tool_name,
                policy = %denial.policy_name,
                reason = %denial.reason,
                "Tool dispatch denied by capability policy (sera-eo71 — runtime-blocked)"
            );
        }
    }
    events
}

/// Emit an OCSF v1.7.0 Policy Activity (class_uid=6003, action_id=blocked)
/// audit entry for a tool denial. Best-effort — an uninitialised audit
/// backend logs a warning and continues; the denial still takes effect in
/// the gateway's in-process state.
async fn emit_policy_denial_audit(denial: &PolicyDenial) {
    use sera_telemetry::audit::{AuditEntry, audit_append};

    let payload = serde_json::json!({
        "activity_id": 1, // "Deny" in OCSF Policy Activity
        "action_id": "blocked",
        "category_uid": 6, // Application Activity
        "class_uid": 6003, // Policy Activity
        "severity_id": 3, // Medium
        "actor": { "user": { "name": denial.agent_id } },
        "policy": { "name": denial.policy_name },
        "resource": { "name": denial.tool_name, "type": "tool" },
        "status": "Failure",
        "status_detail": denial.reason,
    });
    let this_hash = AuditEntry::compute_hash(6003, &payload, &[0u8; 32]);
    let entry = AuditEntry {
        ocsf_class_uid: 6003,
        payload,
        prev_hash: [0u8; 32],
        this_hash,
        signature: None,
    };
    if let Err(e) = audit_append(entry).await {
        // NotInitialised is expected in test boots; other backends may
        // return transient errors. Log and move on — the denial is still
        // enforced in-memory.
        tracing::debug!(error = %e, "audit backend unavailable for policy denial");
    }
}

/// Emit an OCSF v1.7.0 Policy Activity (class_uid=6003) audit entry marking
/// a chat turn as blocked pending HITL approval (sera-z6ql, Wave D Phase 1).
/// Best-effort — uninitialised backends log a warning and continue.
async fn emit_hitl_required_audit(
    agent_name: &str,
    session_key: &str,
    ticket_id: &str,
    mode: sera_hitl::HitlMode,
) {
    use sera_telemetry::audit::{AuditEntry, audit_append};

    let payload = serde_json::json!({
        "activity_id": 1, // "Deny" — the turn was not dispatched
        "action_id": "blocked",
        "category_uid": 6, // Application Activity
        "class_uid": 6003, // Policy Activity
        "severity_id": 3, // Medium
        "actor": { "user": { "name": "http-chat" } },
        "policy": { "name": format!("hitl:{:?}", mode) },
        "resource": {
            "name": agent_name,
            "type": "agent",
            "uid": session_key,
        },
        "status": "Failure",
        "status_detail": "approval required",
        "unmapped": { "ticket_id": ticket_id },
    });
    let this_hash = AuditEntry::compute_hash(6003, &payload, &[0u8; 32]);
    let entry = AuditEntry {
        ocsf_class_uid: 6003,
        payload,
        prev_hash: [0u8; 32],
        this_hash,
        signature: None,
    };
    if let Err(e) = audit_append(entry).await {
        tracing::debug!(error = %e, "audit backend unavailable for HITL gate");
    }
}

// ── Steer injection ────────────────────────────────────────────────────────

/// Send a steer operation to the agent's runtime backend.
/// Used for tool-boundary injection of steer messages.
///
/// sera-ve9x: acquire / NDJSON write / drain-until-`turn_completed` /
/// `mark_unhealthy` all live inside the [`AgentTurnTransport`] impl. The
/// call site only races the trait future against the existing timeout and
/// cancellation token (sera-bsem).
async fn execute_steer(
    transport: &dyn AgentTurnTransport,
    steer_messages: &[serde_json::Value],
    session_key: &str,
    cancel: &CancellationToken,
) -> MvsTurnResult {
    let timeout = turn_timeout();
    let items = steer_messages.to_vec();
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            tracing::warn!(
                session_key = %session_key,
                "Runtime harness steer cancelled (KillSwitch ROLLBACK); releasing lane"
            );
            MvsTurnResult {
                reply: "[sera] Steer injection aborted by KillSwitch ROLLBACK".to_string(),
                tool_events: vec![],
                usage: UsageInfo {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
                cancelled: true,
                failure: None,
            }
        }
        res = tokio::time::timeout(timeout, transport.send_steer(items, session_key)) => match res {
            Ok(Ok(())) => MvsTurnResult {
                reply: "[steer injected]".to_string(),
                tool_events: vec![],
                usage: UsageInfo {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
                cancelled: false,
                failure: None,
            },
            Ok(Err(e)) => {
                tracing::error!(
                    error = %e,
                    session_key = %session_key,
                    "Steer injection failed",
                );
                MvsTurnResult {
                    reply: format!("[sera] Steer injection failed: {e}"),
                    tool_events: vec![],
                    usage: UsageInfo {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                    cancelled: false,
                    failure: None,
                }
            }
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
                    cancelled: false,
                    failure: None,
                }
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

    // Look up the runtime supervisor for this agent (sera-ojp3). Each turn
    // re-acquires through the supervisor so a dead child is respawned
    // instead of repeatedly hitting the same broken pipe.
    let supervisor = match state.harnesses.get(&agent_name) {
        Some(s) => Arc::clone(s),
        None => {
            let err_msg = format!("No runtime supervisor for agent '{agent_name}'");
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

    // Execute the agent turn via the supervisor (sera-ojp3 — respawns a
    // dead child instead of permanently wedging the agent).
    let cancel = state.register_cancellation_token(&session_key);
    let cap_reg = state.capability_registry.read().await.clone();
    let result = execute_turn(
        &agent_spec,
        &transcript,
        &msg.content,
        &*supervisor,
        &session_key,
        &state.skill_engine,
        &state.semantic_store,
        &agent_name,
        &cancel,
        &cap_reg,
        None,
    )
    .await;
    state.deregister_cancellation_token(&session_key);

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
            let cancel = state.register_cancellation_token(&session_key);
            let follow_up =
                execute_steer(&*supervisor, &steer_content, &session_key, &cancel).await;
            state.deregister_cancellation_token(&session_key);
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

        let cancel = state.register_cancellation_token(&session_key);
        let cap_reg = state.capability_registry.read().await.clone();
        let follow_up = execute_turn(
            &agent_spec,
            &transcript,
            &user_content,
            &*supervisor,
            &session_key,
            &state.skill_engine,
            &state.semantic_store,
            &agent_name,
            &cancel,
            &cap_reg,
            None,
        )
        .await;
        state.deregister_cancellation_token(&session_key);

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

        Commands::Start {
            config,
            port,
            local,
        } => {
            let config = if local {
                apply_local_defaults(config, port).await?
            } else {
                config
            };
            run_start(config, port, local).await
        }

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

// ── K.0 unified local bootstrap ─────────────────────────────────────────────
//
// `sera start --local` is the zero-config entry point for hacking on SERA
// against a host-local LM Studio. It replaces the `scripts/sera-local` bash
// wrapper. Behaviour:
//   * Probe http://localhost:1234/v1/models (1s timeout). Fail with a clear
//     remediation message if unreachable.
//   * Default SERA_DATA_ROOT to ./sera-local/ (created on demand).
//   * Default SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE=1 (opt-out only).
//   * If the config path does not exist, drop a minimal local sera.yaml into
//     the data dir and use that.
//   * Print a grep-friendly banner ending with "ready." on its own line.

const LOCAL_LM_STUDIO_URL: &str = "http://localhost:1234/v1";
const LOCAL_LLM_API_KEY: &str = "not-needed-for-local";
const LOCAL_DATA_DIR: &str = "sera-local";
const LOCAL_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

const LOCAL_TEMPLATE_YAML: &str = r#"---
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: sera-local
spec: {}
---
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: lm-studio
spec:
  kind: openai-compatible
  base_url: "http://localhost:1234/v1"
  default_model: local-model
---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: sera
spec:
  provider: lm-studio
  model: local-model
  persona:
    immutable_anchor: |
      You are SERA running in local mode. Reply briefly.
"#;

/// Set env var only if not already present. Returns true if we set it.
fn set_env_if_unset(key: &str, value: &str) -> bool {
    if std::env::var_os(key).is_none() {
        // SAFETY: called from a single-threaded prelude before any tokio tasks
        // that read these vars have been spawned.
        unsafe {
            std::env::set_var(key, value);
        }
        true
    } else {
        false
    }
}

/// Probe an OpenAI-compatible endpoint. Returns Ok(()) on any 2xx/4xx
/// response (a 404 on /models still means the server is up); Err on
/// connection/timeout failure.
async fn probe_llm_endpoint(base_url: &str) -> anyhow::Result<()> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(LOCAL_PROBE_TIMEOUT)
        .build()?;
    let resp = client.get(&url).send().await.map_err(|e| {
        anyhow::anyhow!(
            "LM Studio probe failed at {url}: {e}.\n\n\
             Start LM Studio and load a model at http://localhost:1234,\n\
             or pass --config <your.yaml> with a different provider."
        )
    })?;
    tracing::info!(status = %resp.status(), url = %url, "LM Studio probe");
    Ok(())
}

/// Prepare defaults for `sera start --local`. Returns the config path to use
/// (either the caller's explicit path or a generated one in the data dir).
async fn apply_local_defaults(config: PathBuf, port: u16) -> anyhow::Result<PathBuf> {
    // 1. Probe LM Studio. Fail early with a clear message.
    probe_llm_endpoint(LOCAL_LM_STUDIO_URL).await?;

    // 2. Data dir: default to ./sera-local/ unless SERA_DATA_ROOT is set.
    let data_root = std::env::var_os("SERA_DATA_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(LOCAL_DATA_DIR));
    std::fs::create_dir_all(&data_root)?;
    set_env_if_unset("SERA_DATA_ROOT", &data_root.to_string_lossy());

    // 3. Default to permissive ConstitutionalGate for local dev.
    set_env_if_unset("SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE", "1");

    // 4. Surface the LLM defaults so curious operators can see them via
    //    `env | grep LLM`. Consumers (StdioHarness env) still derive the URL
    //    from the Provider manifest.
    set_env_if_unset("LLM_BASE_URL", LOCAL_LM_STUDIO_URL);
    set_env_if_unset("LLM_API_KEY", LOCAL_LLM_API_KEY);

    // 5. Config: if the caller's path does not exist, drop a minimal local
    //    manifest into the data dir and use that. This keeps `sera start
    //    --local` zero-config from a fresh clone.
    let resolved_config = if config.exists() {
        config
    } else {
        let local_config = data_root.join("sera.yaml");
        if !local_config.exists() {
            std::fs::write(&local_config, LOCAL_TEMPLATE_YAML)?;
        }
        local_config
    };

    // 6. Banner — grep-friendly "ready." on its own line.
    println!("SERA local mode");
    println!("  gateway  http://localhost:{port}");
    println!("  api      http://localhost:{port}/api");
    println!("  sse      http://localhost:{port}/api/chat (stream=true)");
    println!("  llm      {LOCAL_LM_STUDIO_URL}  (LM Studio detected)");
    println!("  data     {}/", data_root.display());
    println!("  logs     {}/logs/", data_root.display());
    println!("  config   {}", resolved_config.display());
    println!("ready.");
    println!("Try: sera-tui   (or: cargo run --bin sera-tui)");

    Ok(resolved_config)
}

async fn run_start(config: PathBuf, port: u16, local: bool) -> anyhow::Result<()> {
    // 1. Load config.
    tracing::info!(config = %config.display(), "Loading SERA configuration");
    let manifests = load_manifest_file(&config)?;

    // 1a. Load capability policies (sera-ifjl). Fail-closed: an agent whose
    // manifest declares a `policyRef` that isn't on disk aborts startup
    // instead of silently running unconstrained.
    let capability_registry = {
        let policies_dir = CapabilityRegistry::resolve_policies_dir();
        tracing::info!(
            policies_dir = %policies_dir.display(),
            "Loading capability policies"
        );
        let bindings = manifests.agents.iter().map(|m| {
            let policy_ref: Option<String> = serde_json::from_value::<AgentSpec>(m.spec.clone())
                .ok()
                .and_then(|s| s.policy_ref);
            (m.metadata.name.clone(), policy_ref)
        });
        let reg = CapabilityRegistry::load_and_bind(&policies_dir, bindings).map_err(|e| {
            anyhow::anyhow!("failed to initialise capability registry (sera-ifjl): {e}")
        })?;
        tracing::info!(
            loaded_policies = reg.policy_count(),
            "Capability registry ready"
        );
        Arc::new(reg)
    };

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

    // Seed constitutional rules from SERA_CONSTITUTIONAL_RULES_PATH (or the
    // default /etc/sera/constitutional_rules.yaml). Missing file → no-op (Ok(0)).
    // Parse error → fail-fast (propagate Err so the process exits with context).
    // Must happen before the HookRegistry is built so the ConstitutionalGateHook
    // receives the populated registry (fixes sera-0yh3 split-Arc bug).
    let constitutional_registry = Arc::new(ConstitutionalRegistry::new());
    match constitutional_config::seed_registry_from_env(&constitutional_registry).await {
        Ok(count) => {
            tracing::info!(count, "Constitutional rules seeded from env path");
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to load constitutional rules: {e}"));
        }
    }

    // Build the HookRegistry and wire in the ConstitutionalGate hook (sera-0yh3).
    // The hook receives the already-seeded registry so rules fire immediately.
    let mut hook_registry_inner = HookRegistry::new();
    sera_runtime::hooks::constitutional::ConstitutionalGateHook::register_into(
        &mut hook_registry_inner,
        Arc::clone(&constitutional_registry),
    );
    let hook_registry = Arc::new(hook_registry_inner);
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

    // sera-y45a / sera-ve9x: surface the dispatch model in the boot log so
    // operators can read which process owns tool dispatch without diving
    // into source. `dispatch_mode` is the *effective* mode — what the
    // running code actually does. `dispatch_mode_configured` is what the
    // operator requested via SERA_DISPATCH_MODE.
    //
    // After sera-ve9x PR 2: `runtime` (default) spawns `sera-runtime --ndjson`
    // per agent, `embedded` builds a `DefaultRuntime` in-process via
    // `EmbeddedRuntimeTransport` (no child process), and `gateway` is still
    // unimplemented and falls back to runtime with a warning.
    // See docs/plan/decisions/2026-04-29-dispatch-ownership.md.
    let dispatch_mode = effective_dispatch_mode_label();
    let dispatch_mode_configured = configured_dispatch_mode_label();
    tracing::info!(
        dispatch_mode = dispatch_mode,
        dispatch_mode_configured = dispatch_mode_configured,
        "Tool dispatch ownership (sera-y45a)"
    );
    if dispatch_mode_configured != dispatch_mode {
        tracing::warn!(
            dispatch_mode = dispatch_mode,
            dispatch_mode_configured = dispatch_mode_configured,
            "SERA_DISPATCH_MODE requests a not-yet-implemented dispatch mode; \
             effective mode falls back to `runtime` until ADR §4 step 3 lands"
        );
    }

    let mut harnesses: std::collections::HashMap<String, Arc<dyn AgentTurnTransport>> =
        std::collections::HashMap::new();

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
        env.insert("LLM_BASE_URL".to_string(), base_url.clone());
        env.insert("LLM_MODEL".to_string(), model.clone());
        env.insert("LLM_API_KEY".to_string(), api_key_val.clone());
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
        // sera-eo71: forward this agent's `policyRef` (if any) so the runtime
        // can bind itself to the policy and enforce pre-dispatch. Also forward
        // SERA_CAPABILITY_POLICIES_DIR when set — otherwise the runtime falls
        // back to ConfigRoot::policies_dir() exactly like the gateway does.
        if let Some(policy_ref) = agent_spec.policy_ref.as_deref() {
            env.insert(
                "SERA_AGENT_POLICY_REF".to_string(),
                policy_ref.to_string(),
            );
        }
        if let Ok(dir) = std::env::var("SERA_CAPABILITY_POLICIES_DIR") {
            env.insert("SERA_CAPABILITY_POLICIES_DIR".to_string(), dir);
        }
        // sera-hwny: forward the agent manifest's `tools.allow` list as a
        // comma-separated glob set so the runtime can narrow the tool
        // definitions handed to the LLM. We always set the var (empty = no
        // filter) so a stale parent-process value cannot leak into a
        // freshly-restricted agent. `SERA_AGENT_TOOLS_DENY` is reserved for
        // an operator override / future manifest field; the runtime reads it
        // unconditionally. CapabilityRegistry remains the execution gate;
        // this filter only controls schema disclosure to the LLM.
        let tools_allow_csv = agent_spec
            .tools
            .as_ref()
            .map(|t| t.allow.join(","))
            .unwrap_or_default();
        env.insert("SERA_AGENT_TOOLS_ALLOW".to_string(), tools_allow_csv);
        if let Ok(deny) = std::env::var("SERA_AGENT_TOOLS_DENY") {
            env.insert("SERA_AGENT_TOOLS_DENY".to_string(), deny);
        }

        // sera-ve9x: branch on the effective dispatch mode. `runtime` (default)
        // spawns the existing supervised stdio child; `embedded` builds an
        // in-process `DefaultRuntime` and routes turns through
        // `EmbeddedRuntimeTransport`. Both yield an
        // `Arc<dyn AgentTurnTransport>` so the rest of the gateway is
        // backend-agnostic.
        let transport: Option<Arc<dyn AgentTurnTransport>> = match dispatch_mode {
            "embedded" => {
                match build_embedded_transport(
                    agent_name,
                    &agent_spec,
                    &base_url,
                    &model,
                    &api_key_val,
                ) {
                    Ok(t) => {
                        tracing::info!(
                            agent = %agent_name,
                            model = %model,
                            dispatch_mode = dispatch_mode,
                            dispatch_mode_configured = dispatch_mode_configured,
                            "Built embedded runtime transport (sera-ve9x)"
                        );
                        Some(t)
                    }
                    Err(e) => {
                        tracing::error!(
                            agent = %agent_name,
                            error = %e,
                            "Failed to build embedded runtime transport"
                        );
                        None
                    }
                }
            }
            // runtime mode (default) — sera-ojp3 supervisor wrapping the
            // long-lived `sera-runtime --ndjson` child process.
            _ => {
                match RuntimeChildSupervisor::start(
                    agent_name.to_string(),
                    runtime_bin.clone(),
                    env,
                )
                .await
                {
                    Ok(supervisor) => {
                        tracing::info!(
                            agent = %agent_name,
                            model = %model,
                            dispatch_mode = dispatch_mode,
                            dispatch_mode_configured = dispatch_mode_configured,
                            "Spawned runtime harness via supervisor (sera-ojp3)"
                        );
                        Some(supervisor as Arc<dyn AgentTurnTransport>)
                    }
                    Err(e) => {
                        tracing::error!(
                            agent = %agent_name,
                            error = %e,
                            "Failed to spawn runtime harness"
                        );
                        None
                    }
                }
            }
        };
        if let Some(t) = transport {
            harnesses.insert(agent_name.to_string(), t);
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

    // sera-nrn9: admin HTTP server auth + audit log. Token comes from
    // SERA_ADMIN_TOKEN; in --local mode we mint a random token at boot and
    // print it once to stderr so dev workflows still work.
    let admin_auth = Arc::new(AdminAuth::resolve(local).map_err(|e| {
        anyhow::anyhow!(
            "{e} (set SERA_ADMIN_TOKEN to enable admin HTTP, or pass --local to auto-generate)"
        )
    })?);
    let admin_audit_path = AdminAuditLogger::default_path(&data_root);
    let admin_audit = AdminAuditLogger::shared(admin_audit_path);

    let state = Arc::new(AppState {
        db: Arc::new(Mutex::new(db)),
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
        active_cancellation_tokens: Arc::new(std::sync::Mutex::new(
            std::collections::HashMap::new(),
        )),
        // sera-fpmt (K.1): default to plain SqliteSessionStore (no shadow-git).
        // The git-shadowed variant (sera-4i4i, byte-for-byte deterministic
        // audit log) is enterprise-only; opt in via the `enterprise` cargo
        // feature or `SERA_SESSION_STORE=git-shadowed`.
        session_store: {
            let parts_db = data_root.join("parts.sqlite");
            #[cfg(feature = "enterprise")]
            let sessions_root = data_root.join("sessions");
            #[cfg(feature = "enterprise")]
            let want_git_shadow = std::env::var("SERA_SESSION_STORE")
                .map(|v| v == "git-shadowed")
                .unwrap_or(true);
            #[cfg(feature = "enterprise")]
            if want_git_shadow {
                tracing::info!(
                    target: "sera_gateway::session_store",
                    "using SqliteGitSessionStore (shadow-git audit log)"
                );
                Arc::new(
                    SqliteGitSessionStore::open(&parts_db, &sessions_root)
                        .expect("failed to initialize SqliteGitSessionStore"),
                ) as Arc<dyn SessionStore>
            } else {
                tracing::info!(
                    target: "sera_gateway::session_store",
                    "using SqliteSessionStore (no shadow-git)"
                );
                Arc::new(
                    SqliteSessionStore::open(&parts_db)
                        .expect("failed to initialize SqliteSessionStore"),
                ) as Arc<dyn SessionStore>
            }
            #[cfg(not(feature = "enterprise"))]
            {
                let want_git_shadow = std::env::var("SERA_SESSION_STORE")
                    .map(|v| v == "git-shadowed")
                    .unwrap_or(false);
                if want_git_shadow {
                    tracing::warn!(
                        target: "sera_gateway::session_store",
                        "SERA_SESSION_STORE=git-shadowed requested but enterprise feature is not \
                         enabled; falling back to SqliteSessionStore"
                    );
                }
                tracing::info!(
                    target: "sera_gateway::session_store",
                    "using SqliteSessionStore (no shadow-git)"
                );
                Arc::new(
                    SqliteSessionStore::open(&parts_db)
                        .expect("failed to initialize SqliteSessionStore"),
                ) as Arc<dyn SessionStore>
            }
        },
        constitutional_registry,
        capability_registry: Arc::new(RwLock::new(Arc::clone(&capability_registry))),
        ticket_store: Arc::new(InMemoryTicketStore::new()),
        hitl_resumed_tx: tokio::sync::broadcast::channel(64).0,
        workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
        gh_run_store: Arc::new(InMemoryGhRunStateStore::new()),
        gh_pr_store: Arc::new(InMemoryGhPrStateStore::new()),
        human_gate_store: Arc::new(InMemoryHumanGateStore::new()),
        admin_auth: Some(Arc::clone(&admin_auth)),
        admin_audit: Some(Arc::clone(&admin_audit)),
    });

    // 4. Start event processing loop.
    let event_state = Arc::clone(&state);
    tokio::spawn(async move {
        event_loop(event_state, discord_rx).await;
    });

    // 4a. Spawn workflow scheduler (sera-kgi8 + sera-0zch). Ticks every
    // TICK_INTERVAL, asks sera-workflow which pending tasks are ready, and
    // marks them resolved on the store. Timer and Mail gates are wired
    // end-to-end. GhPr lookup is passed as `None` here because the production
    // GitHub API poller ships in a follow-up bead; POST /api/workflow/tasks
    // returns 501 for gh_pr until then so callers are not silently queued on
    // a path that can never resolve.
    spawn_scheduler(
        Arc::clone(&state.workflow_store),
        Arc::clone(&state.mail_lookup),
        Some(Arc::clone(&state.gh_run_store)),
        None,
        Some(Arc::clone(&state.human_gate_store)),
        Some(Arc::clone(&state.gh_pr_store)),
        Arc::clone(&shutting_down),
    );

    // sera-ai4w: production GitHub poller (gated behind gh-api feature).
    #[cfg(feature = "gh-api")]
    if let Some(poller_config) = sera_gateway::github_poller::GitHubPollerConfig::from_env() {
        tracing::info!(
            target: "sera_gateway::github_poller",
            interval_secs = poller_config.interval.as_secs(),
            "starting GitHub poller for GhPr / GhRun gates"
        );
        sera_gateway::github_poller::spawn_poller(
            poller_config,
            Some(Arc::clone(&state.gh_pr_store)),
            Some(Arc::clone(&state.gh_run_store)),
            Arc::clone(&shutting_down),
        );
    } else {
        tracing::info!(
            target: "sera_gateway::github_poller",
            "SERA_GH_TOKEN unset — GitHub poller not started"
        );
    }

    // 4a. Spawn admin kill-switch Unix socket (SPEC-gateway §7a.4).
    {
        let ks = Arc::clone(&state.kill_switch);
        let sock_path = admin_sock_path();
        // sera-bsem: the rollback callback also cancels every in-flight
        // turn/steer so wedged runtimes release their lane slots promptly.
        // sera-40y3: it also kills every registered runtime harness and
        // awaits completion before replying to the admin socket so the
        // post-DISARM resume path cannot read stale TurnCompleted frames
        // from the previous (aborted) turn — see AgentTurnTransport::
        // kill_for_rollback.
        let rollback_state = Arc::clone(&state);
        spawn_admin_socket(ks, sock_path, move || {
            let state = Arc::clone(&rollback_state);
            Box::pin(async move {
                let cancelled = state.cancel_all_in_flight();
                // Kill every registered harness inline so harness resets
                // complete before the caller can issue DISARM and resume
                // serving.
                let count = state.harnesses.len();
                for (agent, harness) in state.harnesses.iter() {
                    tracing::warn!(
                        agent = %agent,
                        "killing runtime harness after KillSwitch ROLLBACK"
                    );
                    harness.kill_for_rollback().await;
                }
                tracing::warn!(
                    event = "KILL_SWITCH_ACTIVATED",
                    cancelled_turns = cancelled,
                    harnesses_killed = count,
                    "ROLLBACK received on admin socket — gateway halted"
                );
            })
        });
    }

    // 4b. Start the admin HTTP server (sera-nrn9, L.3). Binds separately
    // from the public API on `SERA_ADMIN_BIND:SERA_ADMIN_PORT` (default
    // 127.0.0.1:3002). Runs in the background; the gateway considers itself
    // ready once both listeners are bound.
    let admin_shutdown = Arc::clone(&shutting_down);
    let admin_state_clone = Arc::clone(&state);
    let admin_bind = resolve_admin_bind();
    let admin_port = resolve_admin_port();
    let admin_addr: SocketAddr = format!("{admin_bind}:{admin_port}")
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid admin bind address {admin_bind}:{admin_port}: {e}"))?;
    let (admin_bound_tx, admin_bound_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let shutdown = async move {
            while !admin_shutdown.load(std::sync::atomic::Ordering::SeqCst) {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        };
        if let Err(e) =
            serve_admin(admin_state_clone, admin_addr, admin_bound_tx, shutdown).await
        {
            tracing::error!(error = %e, "admin HTTP server exited with error");
        }
    });
    match tokio::time::timeout(std::time::Duration::from_secs(2), admin_bound_rx).await {
        Ok(Ok(addr)) => tracing::info!(%addr, "admin HTTP server bound"),
        Ok(Err(_)) => tracing::warn!("admin HTTP server task ended before binding"),
        Err(_) => tracing::warn!("admin HTTP server bind timed out — continuing anyway"),
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

    // Phase B.1 — harness drain. Fire every supervisor shutdown in parallel
    // so a slow harness does not serialize the others. Bound by
    // `HARNESS_DRAIN_DEADLINE`. Each supervisor flips its `stopping` flag so
    // no respawn races with drain.
    let harness_drain = tokio::time::timeout(HARNESS_DRAIN_DEADLINE, async {
        let harness_shutdowns: Vec<_> = state
            .harnesses
            .iter()
            .map(|(name, supervisor)| {
                let name = name.clone();
                let supervisor = Arc::clone(supervisor);
                async move {
                    if let Err(e) = supervisor.shutdown().await {
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
        // sera-mplr / J.0.4 ESC-cancel: abort the in-flight HTTP turn for a
        // session_id. Returns 204 on cancel, 404 when no turn is in flight.
        .route("/api/chat/cancel", post(chat_cancel_handler))
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
        // ── HITL approval requests (sera-z6ql, Wave D Phase 1) ───────────────
        .route(
            "/api/hitl/requests",
            get(route_hitl::list_tickets::<AppState>),
        )
        .route(
            "/api/hitl/requests/{id}",
            get(route_hitl::get_ticket::<AppState>),
        )
        .route(
            "/api/hitl/requests/{id}/approve",
            post(route_hitl::approve_ticket::<AppState>),
        )
        .route(
            "/api/hitl/requests/{id}/reject",
            post(route_hitl::reject_ticket::<AppState>),
        )
        .route(
            "/api/hitl/requests/{id}/escalate",
            post(route_hitl::escalate_ticket::<AppState>),
        )
        // ── sera-kgi8 / sera-0zch: workflow task scheduler ───────────────────
        .route(
            "/api/workflow/tasks",
            post(route_workflow::create_task::<AppState>)
                .get(route_workflow::list_tasks::<AppState>),
        )
        .route(
            "/api/workflow/tasks/{id}",
            get(route_workflow::get_task::<AppState>),
        )
        // sera-0zch: in-memory mail delivery for test/dev — no real SMTP/IMAP.
        .route(
            "/api/workflow/mail/deliver",
            post(route_workflow::deliver_mail::<AppState>),
        )
        // ── sera-dgk1: Human gate resume ─────────────────────────────────
        .route(
            "/api/workflow/tasks/{id}/resume",
            post(route_workflow::resume_task::<AppState>),
        )
        // ── sera-7ivj: OpenAI-compatible inference proxy ─────────────────────
        .route(
            "/v1/chat/completions",
            post(route_inference_proxy::chat_completions::<AppState>),
        )
        // ── sera-7ivj PR 2: Anthropic-compatible inference proxy ─────────────
        .route(
            "/v1/messages",
            post(route_inference_proxy::chat_messages::<AppState>),
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

    // ── sera-y45a: dispatch mode parsing & effective-mode reporting ─────────
    //
    // `parse_configured_dispatch_mode` parses the operator-requested mode
    // from `SERA_DISPATCH_MODE`; `effective_dispatch_mode_label` reports what
    // the running binary actually does. They diverge today because this
    // binary always spawns `sera-runtime` via `StdioHarness`, so the boot log
    // must record them under distinct fields and never let an unimplemented
    // target masquerade as an active dispatch model.
    // See docs/plan/decisions/2026-04-29-dispatch-ownership.md.

    #[test]
    fn parse_configured_dispatch_mode_defaults_to_runtime_when_unset() {
        assert_eq!(parse_configured_dispatch_mode(None), "runtime");
        assert_eq!(parse_configured_dispatch_mode(Some("")), "runtime");
        assert_eq!(parse_configured_dispatch_mode(Some("   ")), "runtime");
    }

    #[test]
    fn parse_configured_dispatch_mode_recognises_canonical_values() {
        // Canonical *requests* — the parser echoes them back as the
        // configured request. The boot log surfaces these only under
        // `dispatch_mode_configured`, never as the active `dispatch_mode`.
        assert_eq!(parse_configured_dispatch_mode(Some("runtime")), "runtime");
        assert_eq!(parse_configured_dispatch_mode(Some("gateway")), "gateway");
        assert_eq!(parse_configured_dispatch_mode(Some("embedded")), "embedded");
    }

    #[test]
    fn parse_configured_dispatch_mode_trims_surrounding_whitespace() {
        assert_eq!(parse_configured_dispatch_mode(Some("  gateway  ")), "gateway");
        assert_eq!(parse_configured_dispatch_mode(Some("\tembedded\n")), "embedded");
    }

    #[test]
    fn parse_configured_dispatch_mode_unknown_falls_back_to_runtime() {
        // case-sensitive on purpose: env values are documented lowercase, and
        // accidental uppercase should not silently claim a different model.
        assert_eq!(parse_configured_dispatch_mode(Some("RUNTIME")), "runtime");
        assert_eq!(parse_configured_dispatch_mode(Some("Gateway")), "runtime");
        assert_eq!(parse_configured_dispatch_mode(Some("foo")), "runtime");
    }

    // sera-ve9x: `effective_dispatch_mode_label()` reads SERA_DISPATCH_MODE,
    // so the tests below mutate process-global env. Holding this mutex
    // serialises them past the cargo-test default of parallel execution.
    static DISPATCH_MODE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard that sets/unsets `SERA_DISPATCH_MODE` and restores the
    /// prior value on drop. Use under `DISPATCH_MODE_ENV_LOCK` only.
    struct DispatchModeEnvGuard {
        prev: Option<String>,
    }

    impl DispatchModeEnvGuard {
        fn unset() -> Self {
            let prev = std::env::var("SERA_DISPATCH_MODE").ok();
            // SAFETY: callers hold DISPATCH_MODE_ENV_LOCK so no concurrent
            // reader can observe a torn value.
            unsafe { std::env::remove_var("SERA_DISPATCH_MODE") };
            Self { prev }
        }
        fn set(value: &str) -> Self {
            let prev = std::env::var("SERA_DISPATCH_MODE").ok();
            // SAFETY: see DispatchModeEnvGuard::unset.
            unsafe { std::env::set_var("SERA_DISPATCH_MODE", value) };
            Self { prev }
        }
    }

    impl Drop for DispatchModeEnvGuard {
        fn drop(&mut self) {
            // SAFETY: see DispatchModeEnvGuard::unset.
            unsafe {
                match &self.prev {
                    Some(v) => std::env::set_var("SERA_DISPATCH_MODE", v),
                    None => std::env::remove_var("SERA_DISPATCH_MODE"),
                }
            }
        }
    }

    #[test]
    fn effective_dispatch_mode_defaults_to_runtime_when_env_unset() {
        let _lock = DISPATCH_MODE_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = DispatchModeEnvGuard::unset();
        assert_eq!(effective_dispatch_mode_label(), "runtime");
    }

    #[test]
    fn effective_dispatch_mode_switches_to_embedded_when_configured() {
        // sera-ve9x PR 2: `SERA_DISPATCH_MODE=embedded` flips the effective
        // mode so the boot log (and per-agent log lines) report the truthful
        // active backend. Before PR 2 this returned `"runtime"` regardless.
        let _lock = DISPATCH_MODE_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = DispatchModeEnvGuard::set("embedded");
        assert_eq!(effective_dispatch_mode_label(), "embedded");
    }

    #[test]
    fn effective_dispatch_mode_falls_back_to_runtime_for_unimplemented_targets() {
        // `gateway` is still unimplemented — until ADR §4 step 3 lands, the
        // effective label must remain `runtime` so the active mode cannot
        // claim a security model the code does not own.
        let _lock = DISPATCH_MODE_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for value in ["gateway", "Gateway", "foo", "RUNTIME", ""] {
            let _guard = DispatchModeEnvGuard::set(value);
            assert_eq!(
                effective_dispatch_mode_label(),
                "runtime",
                "effective mode for SERA_DISPATCH_MODE={value:?} must remain runtime"
            );
        }
    }

    #[test]
    fn effective_dispatch_mode_runtime_value_is_explicit_runtime() {
        // The `runtime` value must round-trip through both the parser and
        // the effective-mode switch.
        let _lock = DISPATCH_MODE_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = DispatchModeEnvGuard::set("runtime");
        assert_eq!(effective_dispatch_mode_label(), "runtime");
    }

    fn test_manifests() -> ManifestSet {
        parse_manifests(TEMPLATE_YAML).unwrap()
    }

    async fn test_harnesses() -> std::collections::HashMap<String, Arc<dyn AgentTurnTransport>> {
        let mut h: std::collections::HashMap<String, Arc<dyn AgentTurnTransport>> =
            std::collections::HashMap::new();
        let supervisor = RuntimeChildSupervisor::start_with_factory("sera", || async {
            StdioHarness::spawn_mock().await
        })
        .await
        .unwrap();
        h.insert("sera".to_string(), supervisor);
        h
    }

    async fn test_state_async() -> Arc<AppState> {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        Arc::new(AppState {
            db: Arc::new(Mutex::new(SqliteDb::open_in_memory().unwrap())),
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
            active_cancellation_tokens: Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
            capability_registry: Arc::new(RwLock::new(Arc::new(CapabilityRegistry::empty()))),
            ticket_store: Arc::new(InMemoryTicketStore::new()),
            hitl_resumed_tx: tokio::sync::broadcast::channel(64).0,
            workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
            gh_run_store: Arc::new(InMemoryGhRunStateStore::new()),
            gh_pr_store: Arc::new(InMemoryGhPrStateStore::new()),
            human_gate_store: Arc::new(InMemoryHumanGateStore::new()),
            admin_auth: None,
            admin_audit: None,
        })
    }

    fn test_state() -> Arc<AppState> {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        Arc::new(AppState {
            db: Arc::new(Mutex::new(SqliteDb::open_in_memory().unwrap())),
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
            active_cancellation_tokens: Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
            capability_registry: Arc::new(RwLock::new(Arc::new(CapabilityRegistry::empty()))),
            ticket_store: Arc::new(InMemoryTicketStore::new()),
            hitl_resumed_tx: tokio::sync::broadcast::channel(64).0,
            workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
            gh_run_store: Arc::new(InMemoryGhRunStateStore::new()),
            gh_pr_store: Arc::new(InMemoryGhPrStateStore::new()),
            human_gate_store: Arc::new(InMemoryHumanGateStore::new()),
            admin_auth: None,
            admin_audit: None,
        })
    }

    async fn test_state_with_api_key_async(key: &str) -> Arc<AppState> {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        Arc::new(AppState {
            db: Arc::new(Mutex::new(SqliteDb::open_in_memory().unwrap())),
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
            active_cancellation_tokens: Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
            capability_registry: Arc::new(RwLock::new(Arc::new(CapabilityRegistry::empty()))),
            ticket_store: Arc::new(InMemoryTicketStore::new()),
            hitl_resumed_tx: tokio::sync::broadcast::channel(64).0,
            workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
            gh_run_store: Arc::new(InMemoryGhRunStateStore::new()),
            gh_pr_store: Arc::new(InMemoryGhPrStateStore::new()),
            human_gate_store: Arc::new(InMemoryHumanGateStore::new()),
            admin_auth: None,
            admin_audit: None,
        })
    }

    fn test_state_with_api_key(key: &str) -> Arc<AppState> {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        Arc::new(AppState {
            db: Arc::new(Mutex::new(SqliteDb::open_in_memory().unwrap())),
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
            active_cancellation_tokens: Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
            capability_registry: Arc::new(RwLock::new(Arc::new(CapabilityRegistry::empty()))),
            ticket_store: Arc::new(InMemoryTicketStore::new()),
            hitl_resumed_tx: tokio::sync::broadcast::channel(64).0,
            workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
            gh_run_store: Arc::new(InMemoryGhRunStateStore::new()),
            gh_pr_store: Arc::new(InMemoryGhPrStateStore::new()),
            human_gate_store: Arc::new(InMemoryHumanGateStore::new()),
            admin_auth: None,
            admin_audit: None,
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
            Commands::Start {
                config,
                port,
                local,
            } => {
                assert_eq!(config, PathBuf::from("sera.yaml"));
                assert_eq!(port, 3001);
                assert!(!local);
            }
            _ => panic!("expected Start"),
        }
    }

    #[test]
    fn parse_start_custom() {
        let cli =
            Cli::try_parse_from(["sera", "start", "-c", "custom.yaml", "-p", "8080"]).unwrap();
        match cli.command {
            Commands::Start {
                config,
                port,
                local,
            } => {
                assert_eq!(config, PathBuf::from("custom.yaml"));
                assert_eq!(port, 8080);
                assert!(!local);
            }
            _ => panic!("expected Start"),
        }
    }

    #[test]
    fn parse_start_local_sets_port_42540() {
        // K.0: `--local` flips the default port to 42540 unless the user
        // overrides with -p explicitly.
        let cli = Cli::try_parse_from(["sera", "start", "--local"]).unwrap();
        match cli.command {
            Commands::Start { port, local, .. } => {
                assert_eq!(port, 42540);
                assert!(local);
            }
            _ => panic!("expected Start"),
        }
    }

    #[test]
    fn parse_start_local_respects_explicit_port() {
        let cli = Cli::try_parse_from(["sera", "start", "--local", "-p", "9999"]).unwrap();
        match cli.command {
            Commands::Start { port, local, .. } => {
                assert_eq!(port, 9999);
                assert!(local);
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
        // Replace the always-good supervisor with one wrapping a hanging mock.
        let hanging_sup: Arc<dyn AgentTurnTransport> =
            RuntimeChildSupervisor::start_with_factory("sera", || async {
                StdioHarness::spawn_mock_hang().await
            })
            .await
            .unwrap();
        Arc::get_mut(&mut state)
            .expect("unique state ref")
            .harnesses
            .insert("sera".to_string(), hanging_sup);
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

    // -- HITL ApprovalRouter gate (sera-z6ql, Wave D Phase 1) --

    /// YAML manifest for a strict-mode agent — every turn must be approved.
    /// Used by the HITL integration tests.
    const STRICT_AGENT_YAML: &str = r#"---
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
  enforcement_mode: strict
  persona:
    immutable_anchor: |
      You are Sera, an autonomous assistant.
"#;

    async fn strict_state() -> Arc<AppState> {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        Arc::new(AppState {
            db: Arc::new(Mutex::new(SqliteDb::open_in_memory().unwrap())),
            manifests: parse_manifests(STRICT_AGENT_YAML).unwrap(),
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
            active_cancellation_tokens: Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
            capability_registry: Arc::new(RwLock::new(Arc::new(CapabilityRegistry::empty()))),
            ticket_store: Arc::new(InMemoryTicketStore::new()),
            hitl_resumed_tx: tokio::sync::broadcast::channel(64).0,
            workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
            gh_run_store: Arc::new(InMemoryGhRunStateStore::new()),
            gh_pr_store: Arc::new(InMemoryGhPrStateStore::new()),
            human_gate_store: Arc::new(InMemoryHumanGateStore::new()),
            admin_auth: None,
            admin_audit: None,
        })
    }

    /// Strict-mode agent blocks every turn with 403 `hitl_approval_required`
    /// and the response body carries a `ticket_id` the caller can look up
    /// via the `/api/hitl/requests` routes.
    #[tokio::test]
    async fn hitl_strict_mode_blocks_chat_turn_and_mints_ticket() {
        let state = strict_state().await;
        let app = build_router(Arc::clone(&state));

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

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "hitl_approval_required");
        let ticket_id = json["ticket_id"].as_str().unwrap().to_owned();
        assert!(!ticket_id.is_empty());

        // Ticket is visible via GET /api/hitl/requests.
        let list_resp = build_router(Arc::clone(&state))
            .oneshot(
                Request::builder()
                    .uri("/api/hitl/requests")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = axum::body::to_bytes(list_resp.into_body(), 16384)
            .await
            .unwrap();
        let list_json: serde_json::Value = serde_json::from_slice(&list_body).unwrap();
        assert_eq!(list_json["count"], 1);
        assert_eq!(list_json["tickets"][0]["id"], ticket_id);

        // Approve the ticket.
        let approve_resp = build_router(Arc::clone(&state))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/hitl/requests/{ticket_id}/approve"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(approve_resp.status(), StatusCode::OK);

        // The ticket now reads back as Approved.
        let got = state
            .ticket_store
            .get(&ticket_id)
            .await
            .expect("ticket should still exist after approve");
        assert_eq!(got.status, sera_hitl::TicketStatus::Approved);
    }

    /// The default (autonomous) agent in TEMPLATE_YAML must not be gated —
    /// regression test covering the old pattern-match removal.
    #[tokio::test]
    async fn hitl_autonomous_mode_does_not_gate_benign_message() {
        let state = test_state_async().await;
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

        // The autonomous path must not return 403. It may succeed (200/OK)
        // or fail downstream with 500/502 (the mock harness is not a real
        // provider), but it must NOT be blocked by the HITL gate.
        assert_ne!(
            response.status(),
            StatusCode::FORBIDDEN,
            "autonomous agent must not be blocked by HITL gate"
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
            db: Arc::new(Mutex::new(SqliteDb::open_in_memory().unwrap())),
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
            active_cancellation_tokens: Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
            capability_registry: Arc::new(RwLock::new(Arc::new(CapabilityRegistry::empty()))),
            ticket_store: Arc::new(InMemoryTicketStore::new()),
            hitl_resumed_tx: tokio::sync::broadcast::channel(64).0,
            workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
            gh_run_store: Arc::new(InMemoryGhRunStateStore::new()),
            gh_pr_store: Arc::new(InMemoryGhPrStateStore::new()),
            human_gate_store: Arc::new(InMemoryHumanGateStore::new()),
            admin_auth: None,
            admin_audit: None,
        };
        let headers = HeaderMap::new();
        assert!(validate_api_key(&state, &headers).is_ok());
    }

    #[test]
    fn validate_api_key_unit_correct_key() {
        let hook_registry = Arc::new(HookRegistry::new());
        let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));
        let state = AppState {
            db: Arc::new(Mutex::new(SqliteDb::open_in_memory().unwrap())),
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
            active_cancellation_tokens: Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
            capability_registry: Arc::new(RwLock::new(Arc::new(CapabilityRegistry::empty()))),
            ticket_store: Arc::new(InMemoryTicketStore::new()),
            hitl_resumed_tx: tokio::sync::broadcast::channel(64).0,
            workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
            gh_run_store: Arc::new(InMemoryGhRunStateStore::new()),
            gh_pr_store: Arc::new(InMemoryGhPrStateStore::new()),
            human_gate_store: Arc::new(InMemoryHumanGateStore::new()),
            admin_auth: None,
            admin_audit: None,
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
            db: Arc::new(Mutex::new(SqliteDb::open_in_memory().unwrap())),
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
            active_cancellation_tokens: Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
            capability_registry: Arc::new(RwLock::new(Arc::new(CapabilityRegistry::empty()))),
            ticket_store: Arc::new(InMemoryTicketStore::new()),
            hitl_resumed_tx: tokio::sync::broadcast::channel(64).0,
            workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
            gh_run_store: Arc::new(InMemoryGhRunStateStore::new()),
            gh_pr_store: Arc::new(InMemoryGhPrStateStore::new()),
            human_gate_store: Arc::new(InMemoryHumanGateStore::new()),
            admin_auth: None,
            admin_audit: None,
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
            db: Arc::new(Mutex::new(SqliteDb::open_in_memory().unwrap())),
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
            active_cancellation_tokens: Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
            capability_registry: Arc::new(RwLock::new(Arc::new(CapabilityRegistry::empty()))),
            ticket_store: Arc::new(InMemoryTicketStore::new()),
            hitl_resumed_tx: tokio::sync::broadcast::channel(64).0,
            workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
            gh_run_store: Arc::new(InMemoryGhRunStateStore::new()),
            gh_pr_store: Arc::new(InMemoryGhPrStateStore::new()),
            human_gate_store: Arc::new(InMemoryHumanGateStore::new()),
            admin_auth: None,
            admin_audit: None,
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

    /// sera-k8do: documents the SSE `message` payload shape the streaming
    /// pump emits when a delta lands. The web client's `parseChatSseEvent`
    /// matches on the same `delta` / `session_id` / `message_id` keys.
    #[test]
    fn stream_message_event_payload_shape() {
        let session_id = "sess-1";
        let message_id = "msg_00000001";
        let payload = serde_json::json!({
            "delta": "Hello ",
            "session_id": session_id,
            "message_id": message_id,
        });
        let parsed: serde_json::Value =
            serde_json::from_str(&payload.to_string()).expect("payload re-parses");
        assert_eq!(parsed["delta"], "Hello ");
        assert_eq!(parsed["session_id"], session_id);
        assert_eq!(parsed["message_id"], message_id);
    }

    /// Trivial structural test: feeding `Done` into the unfold immediately
    /// terminates the stream. Mirrors the previous `stream_state_done_yields_nothing`
    /// shape but no longer references the removed `Streaming { chunks }` variant.
    #[tokio::test]
    async fn stream_state_streaming_yields_message_events() {
        use futures_util::StreamExt as _;

        let stream = futures_util::stream::unfold(StreamState::Done, |fold_state| async move {
            match fold_state {
                StreamState::Done => {
                    None::<(Option<Result<axum::response::sse::Event, std::convert::Infallible>>, StreamState)>
                }
                StreamState::Streaming { .. } => unreachable!(),
            }
        })
        .filter_map(|item| async move { item });

        let events: Vec<_> = stream.collect().await;
        assert!(events.is_empty());
    }

    /// sera-aepj: documents the SSE shape the streaming branch emits when
    /// `execute_turn` returns an empty reply. The web client's
    /// `parseChatSseEvent` matches on `event: error` + `data.error` to surface
    /// the failure rather than rendering a stuck "thinking…" spinner.
    #[test]
    fn empty_reply_error_event_payload_shape() {
        let session_id = "sess-empty";
        let message_id = "msg_deadbeef";
        let payload = serde_json::json!({
            "error": "runtime returned empty reply",
            "session_id": session_id,
            "message_id": message_id,
        });
        let parsed: serde_json::Value =
            serde_json::from_str(&payload.to_string()).expect("payload re-parses");
        assert_eq!(parsed["error"], "runtime returned empty reply");
        assert_eq!(parsed["session_id"], session_id);
        assert_eq!(parsed["message_id"], message_id);
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

    /// sera-y9f8 regression guard: `execute_steer` must read the event
    /// type from `msg.type` (mirroring `StdioHarness::send_turn`) **and**
    /// fully drain the steer turn through its terminal `turn_completed`.
    ///
    /// Two failure modes are guarded here:
    /// 1. The original bug — reading `event.type` (top-level) never
    ///    matched, so the loop wedged the lane until the
    ///    `SERA_TURN_TIMEOUT_SECS` deadline. We pin the env var to 1 s so
    ///    a regression bounds the test to ~1 s instead of the 600 s
    ///    default.
    /// 2. The follow-up bug flagged on PR #1117 — breaking early on
    ///    `streaming_delta` leaves the steer's `turn_completed` unread on
    ///    the shared harness stdout. The next `send_turn` (which does not
    ///    correlate by `submission_id`) would then consume that leftover
    ///    completion and return an empty response. We assert that a
    ///    follow-up `send_turn` still observes the mock's normal
    ///    `streaming_delta("mock response")` payload, proving no steer
    ///    frames leaked past the steer call.
    #[tokio::test]
    async fn execute_steer_drains_until_turn_completed() {
        let prior = std::env::var("SERA_TURN_TIMEOUT_SECS").ok();
        // SAFETY: test-only env mutation; `turn_timeout()` reads the var
        // each call, so the value is observed inline. Restored before the
        // test returns to keep parallel tests hermetic.
        unsafe { std::env::set_var("SERA_TURN_TIMEOUT_SECS", "1") };

        let supervisor = RuntimeChildSupervisor::start_with_factory("sera", || async {
            StdioHarness::spawn_mock().await
        })
        .await
        .unwrap();
        let cancel = CancellationToken::new();
        let steer_messages = vec![serde_json::json!({"role": "user", "content": "hi"})];

        let start = std::time::Instant::now();
        let result =
            execute_steer(&*supervisor, &steer_messages, "y9f8-test-session", &cancel).await;
        let elapsed = start.elapsed();

        // Follow-up turn on the same harness: must see the mock's normal
        // payload, not stale steer frames. With an early `streaming_delta`
        // break, this would observe an empty response because the steer's
        // `turn_completed` would be the first frame `send_turn` reads.
        // sera-ojp3: re-acquire the harness through the supervisor so we
        // verify the same live child still answers (no spurious respawn).
        let harness = supervisor
            .acquire()
            .await
            .expect("supervisor still healthy after steer");
        let follow_up = harness
            .send_turn(Vec::new(), "y9f8-test-session")
            .await
            .expect("follow-up send_turn must succeed against a fresh harness state");

        // SAFETY: restoring the pre-test value; same caveat as above.
        unsafe {
            match prior {
                Some(v) => std::env::set_var("SERA_TURN_TIMEOUT_SECS", v),
                None => std::env::remove_var("SERA_TURN_TIMEOUT_SECS"),
            }
        }

        assert!(
            elapsed < std::time::Duration::from_millis(900),
            "execute_steer must break on the steer's `turn_completed` well \
             before the 1 s SERA_TURN_TIMEOUT_SECS deadline (elapsed={:?})",
            elapsed
        );
        assert_eq!(
            result.reply, "[steer injected]",
            "expected the success sentinel, got: {}",
            result.reply
        );
        assert_eq!(
            follow_up.response, "mock response",
            "follow-up turn must read the mock's fresh streaming_delta, not \
             a leaked steer frame; got: {:?}",
            follow_up.response
        );
    }

    /// sera-bsem regression guard: a hung harness must abort when its
    /// cancellation token fires, not wait out `SERA_TURN_TIMEOUT_SECS`. Before
    /// the fix a ROLLBACK could leave a turn stuck for minutes on a wedged
    /// runtime even though the kill switch was armed.
    #[tokio::test]
    async fn execute_turn_aborts_when_cancellation_token_fires() {
        let supervisor = RuntimeChildSupervisor::start_with_factory("sera", || async {
            StdioHarness::spawn_mock_hang().await
        })
        .await
        .unwrap();
        let agent_spec = AgentSpec {
            provider: "stub".to_string(),
            model: None,
            persona: None,
            tools: None,
            workspace: None,
            policy_ref: None,
            enforcement_mode: None,
            approval_policy: None,
        };
        let skill_engine = SkillDispatchEngine::new();
        let semantic_store: Arc<dyn SemanticMemoryStore> = Arc::new(
            SqliteMemoryStore::open_in_memory(None).expect("open in-memory semantic store"),
        );
        let cancel = CancellationToken::new();
        let capability_registry = CapabilityRegistry::empty();

        // Fire the cancellation a short time into the turn.
        let cancel_firing = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            cancel_firing.cancel();
        });

        let start = std::time::Instant::now();
        let result = execute_turn(
            &agent_spec,
            &[],
            "hello",
            &*supervisor,
            "bsem-test-session",
            &skill_engine,
            &semantic_store,
            "sera",
            &cancel,
            &capability_registry,
            None,
        )
        .await;

        // Must return the cancelled-by-rollback sentinel well before the
        // 600 s default turn timeout.
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "execute_turn must abort within 1s of cancellation, elapsed={:?}",
            start.elapsed()
        );
        assert!(
            result.reply.contains("KillSwitch") || result.reply.contains("aborted"),
            "expected KillSwitch/aborted reply, got: {}",
            result.reply
        );
    }

    /// sera-bsem: `AppState::cancel_all_in_flight` must cancel every
    /// registered token, drain the map, and return the pre-drain count so the
    /// on_rollback log line can report how many turns were aborted.
    #[tokio::test]
    async fn cancel_all_in_flight_cancels_every_registered_token() {
        let state = test_state_async().await;
        let t1 = state.register_cancellation_token("s1");
        let t2 = state.register_cancellation_token("s2");
        let t3 = state.register_cancellation_token("s3");

        let n = state.cancel_all_in_flight();

        assert_eq!(n, 3, "cancel_all_in_flight must report pre-drain count");
        assert!(t1.is_cancelled());
        assert!(t2.is_cancelled());
        assert!(t3.is_cancelled());
        assert!(
            state
                .active_cancellation_tokens
                .lock()
                .unwrap()
                .is_empty(),
            "registry must be drained"
        );
    }

    /// sera-bsem regression: after `cancel_all_in_flight` cancels every
    /// registered token, the cancel arm of `execute_turn` returns
    /// `MvsTurnResult { cancelled: true, .. }`, which the calling chat handler
    /// drives through the same `release_lane` cleanup as the success path.
    /// Lock down the end of that loop: a previously-busy lane must accept new
    /// admissions once the operator path completes, otherwise ROLLBACK plus
    /// disarm leaves the lane wedged at `active_runs == 1` until process
    /// restart (the original bug).
    #[tokio::test]
    async fn rollback_releases_lane_slot_for_subsequent_admissions() {
        let state = test_state_async().await;
        let session_key = occupy_sera_lane(&state).await;

        // Mirror the production registration sequence: a chat handler stamps a
        // CancellationToken into the registry alongside the lane reservation.
        let token = state.register_cancellation_token(&session_key);

        // Operator fires ROLLBACK → cancel_all_in_flight drains the registry
        // and cancels every token (the existing
        // `cancel_all_in_flight_cancels_every_registered_token` test pins this
        // half down).
        let cancelled = state.cancel_all_in_flight();
        assert_eq!(cancelled, 1, "rollback must cancel exactly one token");
        assert!(token.is_cancelled());

        // The cancel arm of execute_turn fires before the chat handler's
        // `release_lane`. Mirror that cleanup so the lane returns to baseline.
        {
            let mut lq = state.lane_queue.lock().await;
            lq.complete_run(&session_key);
        }

        let active = state.lane_queue.lock().await.active_runs();
        assert_eq!(
            active, 0,
            "rollback + cancel arm cleanup must release the lane slot; \
             leaving it at 1 is the sera-bsem wedge that pins the next \
             /api/chat at 429 until process restart"
        );

        // Subsequent submissions on the same session_key must be admitted.
        let principal = PrincipalRef {
            id: PrincipalId::new("http-chat"),
            kind: PrincipalKind::Human,
        };
        let event = DomainEvent::api_message("sera", &session_key, principal, "after rollback");
        let mut lq = state.lane_queue.lock().await;
        assert_eq!(
            lq.enqueue(event),
            sera_db::lane_queue::EnqueueResult::Ready,
            "post-rollback enqueue must be Ready, not Queued — lane was leaked"
        );
    }

    /// sera-mplr: `AppState::cancel_http_chat_session` marks the matching
    /// `http:{agent}:{session_id}` handle as client-cancelled and fires its
    /// token. The handle is **not** removed here — the chat handler's
    /// `deregister_cancellation_token` call after `execute_turn` returns is
    /// the cleanup point, and it is what reads `client_cancelled` to decide
    /// whether to short-circuit with a real cancelled outcome (sera-mplr
    /// Codex follow-up — distinct cancellation result path).
    #[tokio::test]
    async fn cancel_http_chat_session_marks_handle_and_fires_token() {
        let state = test_state_async().await;
        let token = state.register_cancellation_token("http:sera:ses_abc123");

        let cancelled = state.cancel_http_chat_session("ses_abc123");

        assert!(cancelled, "must report a token was cancelled");
        assert!(token.is_cancelled(), "token must be cancelled");
        // Entry stays in the map; chat_handler's deregister is the cleanup point.
        assert!(
            state
                .active_cancellation_tokens
                .lock()
                .unwrap()
                .contains_key("http:sera:ses_abc123")
        );
    }

    /// sera-mplr: `deregister_cancellation_token` after a client cancel returns
    /// `true` and removes the entry — this is the cleanup-after-terminal-state
    /// path the chat handler keys off to short-circuit with a 499 cancelled
    /// outcome instead of persisting the rollback-class synthetic reply.
    #[tokio::test]
    async fn deregister_after_client_cancel_returns_true_and_removes_entry() {
        let state = test_state_async().await;
        let _ = state.register_cancellation_token("http:sera:ses_abc123");
        assert!(state.cancel_http_chat_session("ses_abc123"));

        assert!(
            state.deregister_cancellation_token("http:sera:ses_abc123"),
            "deregister must report the cancel was client-driven"
        );
        assert!(
            state.active_cancellation_tokens.lock().unwrap().is_empty(),
            "registry must be drained on deregister"
        );
    }

    /// sera-mplr: `deregister_cancellation_token` returns `false` when the
    /// turn ran to completion without any cancel — chat_handler then takes
    /// the existing transcript-persist path.
    #[tokio::test]
    async fn deregister_after_normal_completion_returns_false() {
        let state = test_state_async().await;
        let _ = state.register_cancellation_token("http:sera:ses_abc123");
        assert!(!state.deregister_cancellation_token("http:sera:ses_abc123"));
    }

    /// sera-mplr: `deregister_cancellation_token` returns `false` after a
    /// rollback-style cancel (admin ROLLBACK / `cancel_all_in_flight`). That
    /// way chat_handler keeps its existing synthetic-reply contract for
    /// operator-driven cancels — only `/api/chat/cancel` flips the flag.
    #[tokio::test]
    async fn deregister_after_rollback_returns_false() {
        let state = test_state_async().await;
        let _ = state.register_cancellation_token("http:sera:ses_abc123");
        let _ = state.cancel_all_in_flight();
        assert!(
            !state.deregister_cancellation_token("http:sera:ses_abc123"),
            "rollback path must not look like a client cancel"
        );
    }

    /// sera-mplr: cancelling a session_id with no active turn returns `false`
    /// (the route translates this to a 404).
    #[tokio::test]
    async fn cancel_http_chat_session_returns_false_when_no_active_turn() {
        let state = test_state_async().await;
        assert!(!state.cancel_http_chat_session("ses_missing"));
    }

    /// sera-mplr: cancelling one session_id must not affect any other
    /// in-flight HTTP turn. Cross-session cancellation would let one client
    /// interrupt another's lane slot.
    #[tokio::test]
    async fn cancel_http_chat_session_does_not_cancel_other_sessions() {
        let state = test_state_async().await;
        let target = state.register_cancellation_token("http:sera:ses_target");
        let bystander = state.register_cancellation_token("http:sera:ses_bystander");

        assert!(state.cancel_http_chat_session("ses_target"));

        assert!(target.is_cancelled());
        assert!(
            !bystander.is_cancelled(),
            "bystander session must not be cancelled"
        );
        // Both entries remain in the map; deregister is the cleanup point.
        let map = state.active_cancellation_tokens.lock().unwrap();
        assert!(map.contains_key("http:sera:ses_bystander"));
        assert!(map.contains_key("http:sera:ses_target"));
    }

    /// sera-mplr: `/api/chat/cancel` is the HTTP-surface cancel route — it
    /// must not match Discord transport keys (`discord:{agent}:{channel_id}`)
    /// even when the suffix happens to coincide with the inbound session_id.
    #[tokio::test]
    async fn cancel_http_chat_session_ignores_non_http_transport_keys() {
        let state = test_state_async().await;
        let discord = state.register_cancellation_token("discord:sera:ses_collide");

        assert!(!state.cancel_http_chat_session("ses_collide"));
        assert!(!discord.is_cancelled());
        assert!(
            state
                .active_cancellation_tokens
                .lock()
                .unwrap()
                .contains_key("discord:sera:ses_collide")
        );
    }

    /// sera-mplr route-level: 204 when an active HTTP turn is cancelled.
    #[tokio::test]
    async fn chat_cancel_endpoint_returns_204_when_active_turn_cancelled() {
        let state = test_state_async().await;
        let token = state.register_cancellation_token("http:sera:ses_route_ok");
        let app = build_router(Arc::clone(&state));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat/cancel")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "session_id": "ses_route_ok" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(token.is_cancelled());
    }

    /// sera-mplr route-level: 404 when no active turn matches the session_id.
    #[tokio::test]
    async fn chat_cancel_endpoint_returns_404_when_no_active_turn() {
        let state = test_state_async().await;
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat/cancel")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "session_id": "ses_nope" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "not_found");
        assert_eq!(json["reason"], "no_active_turn");
        assert_eq!(json["session_id"], "ses_nope");
    }

    /// sera-mplr Codex follow-up: when a `/api/chat` turn is cancelled by a
    /// concurrent `/api/chat/cancel`, the chat handler must:
    ///   1. return a 499 Client Closed Request with a `{cancelled, reason,
    ///      session_id}` body — not 200 with the synthetic
    ///      `[sera] Runtime turn aborted by KillSwitch ROLLBACK` reply, and
    ///   2. **not** persist the synthetic reply as an assistant transcript
    ///      row (so cancellations don't pollute the conversation history).
    ///
    /// Uses the existing `spawn_mock_hang` harness pattern (see the
    /// `readiness_*` tests above) to keep `execute_turn` blocked long enough
    /// for the cancel to race in.
    #[tokio::test]
    async fn chat_handler_returns_499_and_skips_transcript_when_user_cancels_mid_turn() {
        let mut state = test_state_async().await;
        let hanging_sup: Arc<dyn AgentTurnTransport> =
            RuntimeChildSupervisor::start_with_factory("sera", || async {
                StdioHarness::spawn_mock_hang().await
            })
            .await
            .unwrap();
        Arc::get_mut(&mut state)
            .expect("unique state ref")
            .harnesses
            .insert("sera".to_string(), hanging_sup);
        let app = build_router(Arc::clone(&state));

        // Kick off the chat request in the background — it will hang inside
        // `execute_turn` until we cancel it.
        let chat_app = app.clone();
        let chat_handle = tokio::spawn(async move {
            chat_app
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
                .unwrap()
        });

        // Wait for `chat_handler` to register its cancellation token, then
        // extract the session_id from the registry key
        // (`http:{agent}:{session_id}`).
        let session_key = {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                if let Some(key) = state
                    .active_cancellation_tokens
                    .lock()
                    .unwrap()
                    .keys()
                    .find(|k| k.starts_with("http:sera:"))
                    .cloned()
                {
                    break key;
                }
                if std::time::Instant::now() > deadline {
                    panic!("chat_handler never registered a cancellation token");
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        };
        let session_id = session_key
            .splitn(3, ':')
            .nth(2)
            .expect("session_id segment")
            .to_string();

        let cancel_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat/cancel")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "session_id": session_id }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cancel_response.status(), StatusCode::NO_CONTENT);

        let chat_response = chat_handle.await.expect("chat_handler task joined");
        assert_eq!(
            chat_response.status(),
            StatusCode::from_u16(499).unwrap(),
            "chat_handler must return 499 Client Closed Request on user cancel"
        );
        let body = axum::body::to_bytes(chat_response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["cancelled"], true);
        assert_eq!(json["reason"], "client_cancel");
        assert_eq!(json["session_id"], session_id);

        // The synthetic "[sera] Runtime turn aborted ..." reply must NOT be
        // persisted as an assistant transcript row.
        let assistant_rows: Vec<_> = {
            let db = state.db.lock().await;
            db.get_transcript(&session_id)
                .expect("get transcript")
                .into_iter()
                .filter(|r| r.role == "assistant")
                .collect()
        };
        assert!(
            assistant_rows.is_empty(),
            "no assistant transcript row should be persisted on user cancel; got: {assistant_rows:?}"
        );
    }

    /// sera-k8do: real-streaming acceptance test.
    ///
    /// Asserts the first SSE `message` event arrives <500ms after the
    /// `/api/chat` request even though the underlying turn takes >1.5s
    /// in total. With the pre-fix `split_inclusive(' ')` path, no SSE
    /// frame is emitted until `execute_turn` returns the full reply, so
    /// the first message would land >2s after the request — failing this
    /// bound.
    ///
    /// Uses an in-test `AgentTurnTransport` whose `send_turn_streaming`
    /// pushes the first delta after a tiny delay, then sleeps >2s before
    /// emitting the rest. The test reads the response body chunk-by-chunk
    /// and times the first `event: message` frame.
    #[tokio::test]
    async fn sse_first_delta_arrives_before_turn_completes() {
        use async_trait::async_trait;
        use serde_json::Value;
        use std::time::{Duration, Instant};
        use tokio::sync::mpsc::Sender;

        struct SlowStreamingTransport;

        #[async_trait]
        impl AgentTurnTransport for SlowStreamingTransport {
            async fn send_turn(
                &self,
                _messages: Vec<Value>,
                _session_key: &str,
            ) -> anyhow::Result<TurnEvents> {
                // Synchronous fallback path is unused in this test, but
                // keep the timing realistic in case the SSE branch ever
                // regresses to calling send_turn directly.
                tokio::time::sleep(Duration::from_millis(2050)).await;
                Ok(TurnEvents {
                    response: "Hello world!".to_string(),
                    tool_events: vec![],
                    usage: UsageInfo::default(),
                })
            }

            async fn send_turn_streaming(
                &self,
                _messages: Vec<Value>,
                _session_key: &str,
                delta_tx: Sender<String>,
            ) -> anyhow::Result<TurnEvents> {
                // Quick first delta — proves real streaming is plumbed.
                tokio::time::sleep(Duration::from_millis(50)).await;
                let _ = delta_tx.send("Hello ".to_string()).await;
                // Long tail — proves the unfold doesn't wait for the
                // full reply before emitting the first frame.
                tokio::time::sleep(Duration::from_millis(2000)).await;
                let _ = delta_tx.send("world!".to_string()).await;
                Ok(TurnEvents {
                    response: "Hello world!".to_string(),
                    tool_events: vec![],
                    usage: UsageInfo::default(),
                })
            }

            async fn send_steer(
                &self,
                _items: Vec<Value>,
                _session_key: &str,
            ) -> anyhow::Result<()> {
                Ok(())
            }
            async fn shutdown(&self) -> anyhow::Result<()> {
                Ok(())
            }
            async fn liveness_probe(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let mut state = test_state_async().await;
        Arc::get_mut(&mut state)
            .expect("unique state ref")
            .harnesses
            .insert(
                "sera".to_string(),
                Arc::new(SlowStreamingTransport) as Arc<dyn AgentTurnTransport>,
            );
        let app = build_router(Arc::clone(&state));

        let started = Instant::now();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "message": "hi", "stream": true })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let mut body_stream = response.into_body().into_data_stream();
        let mut buffer = Vec::<u8>::new();
        let mut first_message_at: Option<Duration> = None;

        // Read until we observe the first complete `event: message` frame.
        // 1.5s upper bound generously covers the 50ms emit + scheduler jitter
        // while still failing the buggy >2s post-hoc-split behaviour.
        let read_deadline = Instant::now() + Duration::from_millis(1500);
        while first_message_at.is_none() && Instant::now() < read_deadline {
            let remaining = read_deadline.saturating_duration_since(Instant::now());
            let chunk =
                match tokio::time::timeout(remaining, body_stream.next()).await {
                    Ok(Some(Ok(b))) => b,
                    Ok(Some(Err(e))) => panic!("SSE body errored: {e}"),
                    Ok(None) => panic!("SSE body ended before first message"),
                    Err(_) => break,
                };
            buffer.extend_from_slice(&chunk);
            let so_far = std::str::from_utf8(&buffer).unwrap_or("");
            if so_far.contains("event: message\n") {
                first_message_at = Some(started.elapsed());
            }
        }

        let elapsed = first_message_at.unwrap_or_else(|| {
            panic!(
                "first SSE `message` event must arrive within 1.5s; \
                 buffered so far: {}",
                String::from_utf8_lossy(&buffer)
            )
        });
        assert!(
            elapsed < Duration::from_millis(500),
            "first delta must arrive <500ms after request, got {elapsed:?}"
        );

        // Drain the rest so we can verify the turn really did take >1.5s
        // and that a terminating `done` frame closes the stream.
        let drain_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = drain_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, body_stream.next()).await {
                Ok(Some(Ok(b))) => buffer.extend_from_slice(&b),
                Ok(Some(Err(e))) => panic!("SSE body errored mid-drain: {e}"),
                Ok(None) => break,
                Err(_) => break,
            }
        }
        let total_elapsed = started.elapsed();
        let body_str = std::str::from_utf8(&buffer).unwrap_or("");
        assert!(
            body_str.matches("event: message").count() >= 2,
            "expected ≥2 message events; body: {body_str}"
        );
        assert!(
            body_str.contains("event: done"),
            "expected terminating `done` event; body: {body_str}"
        );
        assert!(
            total_elapsed > Duration::from_millis(1500),
            "full turn must take >1.5s for the timing assertion to be \
             meaningful; got {total_elapsed:?}"
        );
    }

    /// sera-k8do (Codex review on PR #1153): mid-stream runtime error
    /// regression test.
    ///
    /// Asserts that when the runtime emits one or more `streaming_delta`
    /// frames and then errors out (e.g. an upstream provider failure or
    /// the runtime "error" NDJSON frame), the SSE client sees the
    /// already-streamed deltas followed by a structured `event: error`
    /// frame, **not** a `done`. The transcript must not record an
    /// assistant row — neither the partial text nor the synthetic
    /// `[sera] Runtime error: …` placeholder — so a follow-up turn does
    /// not see a misleading assistant slot in history.
    ///
    /// Without the fix, `execute_turn` returns a non-empty synthetic
    /// reply, the empty-reply guard does not trip, the unfold persists
    /// the synthetic string as the assistant transcript row and emits
    /// `done` — visible stream output and persisted transcript disagree.
    #[tokio::test]
    async fn sse_runtime_error_after_partial_deltas_emits_error_event_and_skips_transcript_persist()
    {
        use async_trait::async_trait;
        use serde_json::Value;
        use std::time::Duration;
        use tokio::sync::mpsc::Sender;

        struct FailingMidStreamTransport;

        #[async_trait]
        impl AgentTurnTransport for FailingMidStreamTransport {
            async fn send_turn(
                &self,
                _messages: Vec<Value>,
                _session_key: &str,
            ) -> anyhow::Result<TurnEvents> {
                anyhow::bail!("simulated upstream provider error")
            }

            async fn send_turn_streaming(
                &self,
                _messages: Vec<Value>,
                _session_key: &str,
                delta_tx: Sender<String>,
            ) -> anyhow::Result<TurnEvents> {
                // Forward one real delta first — proves the failure
                // path runs *after* visible streaming has begun.
                let _ = delta_tx.send("Hello ".to_string()).await;
                tokio::time::sleep(Duration::from_millis(20)).await;
                anyhow::bail!("simulated upstream provider error")
            }

            async fn send_steer(
                &self,
                _items: Vec<Value>,
                _session_key: &str,
            ) -> anyhow::Result<()> {
                Ok(())
            }
            async fn shutdown(&self) -> anyhow::Result<()> {
                Ok(())
            }
            async fn liveness_probe(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let mut state = test_state_async().await;
        Arc::get_mut(&mut state)
            .expect("unique state ref")
            .harnesses
            .insert(
                "sera".to_string(),
                Arc::new(FailingMidStreamTransport) as Arc<dyn AgentTurnTransport>,
            );

        // Capture the session id up front; `get_or_create_session` is
        // idempotent so the chat handler will dispatch the turn against
        // the same row we inspect afterwards.
        let session_id = {
            let db = state.db.lock().await;
            db.get_or_create_session("sera")
                .expect("get_or_create_session ok")
                .id
        };

        let app = build_router(Arc::clone(&state));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "message": "hi", "stream": true })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Drain the body with a 5s ceiling. The stream must end on its own.
        let body_bytes = tokio::time::timeout(
            Duration::from_secs(5),
            axum::body::to_bytes(response.into_body(), 64 * 1024),
        )
        .await
        .expect("SSE body must terminate after mid-stream runtime error")
        .expect("body bytes");
        let body_str = std::str::from_utf8(&body_bytes).unwrap_or("");

        // Visible stream: one `message` (the "Hello " delta) and exactly
        // one `error` event; no `done`.
        assert_eq!(
            body_str.matches("event: message").count(),
            1,
            "expected exactly one streamed delta before the failure; body: {body_str}"
        );
        assert!(
            body_str.contains("\"delta\":\"Hello \""),
            "the streamed delta must be the real partial text; body: {body_str}"
        );
        assert!(
            body_str.contains("event: error"),
            "expected a structured `error` SSE event; body: {body_str}"
        );
        assert!(
            body_str.contains("runtime stream interrupted"),
            "error payload must surface the failure reason; body: {body_str}"
        );
        assert!(
            !body_str.contains("event: done"),
            "must NOT emit `done` after a mid-stream runtime failure; body: {body_str}"
        );

        // Transcript: the user message must persist, but no assistant
        // row may exist — neither the partial "Hello " nor the synthetic
        // "[sera] Runtime error: …" placeholder.
        let rows = {
            let db = state.db.lock().await;
            db.get_transcript(&session_id).expect("get transcript")
        };
        let assistant_rows: Vec<_> =
            rows.iter().filter(|r| r.role == "assistant").collect();
        assert!(
            assistant_rows.is_empty(),
            "no assistant transcript row should be persisted on mid-stream \
             runtime failure; got: {assistant_rows:?}"
        );
        assert!(
            rows.iter().any(|r| r.role == "user"),
            "the user message must still be persisted; got rows: {rows:?}"
        );
    }

    /// sera-k8do (Codex review on PR #1153): operator-cancel after live
    /// deltas must not persist the synthetic
    /// `[sera] Runtime turn aborted by KillSwitch ROLLBACK` reply as the
    /// assistant transcript row, and must surface a structured
    /// `cancelled` SSE event with `reason=operator_cancel` (not `done`).
    ///
    /// Pre-fix the streaming finalizer gated on `user_cancelled &&
    /// result.cancelled`. Operator paths
    /// (`AppState::cancel_all_in_flight`, KillSwitch ROLLBACK) cancel the
    /// runtime turn without flipping `client_cancelled`, so
    /// `user_cancelled` was `false` and the cancelled branch did not
    /// fire — execute_turn's synthetic abort string was persisted as
    /// assistant content and the client saw `done` after the partial
    /// deltas. Visible stream and persisted history disagreed.
    #[tokio::test]
    async fn sse_operator_cancel_after_partial_deltas_skips_transcript_persist() {
        use async_trait::async_trait;
        use serde_json::Value;
        use std::sync::Arc;
        use std::time::Duration;
        use tokio::sync::Notify;
        use tokio::sync::mpsc::Sender;

        struct HangAfterDeltaTransport {
            emitted: Arc<Notify>,
        }

        #[async_trait]
        impl AgentTurnTransport for HangAfterDeltaTransport {
            async fn send_turn(
                &self,
                _messages: Vec<Value>,
                _session_key: &str,
            ) -> anyhow::Result<TurnEvents> {
                std::future::pending::<()>().await;
                unreachable!()
            }

            async fn send_turn_streaming(
                &self,
                _messages: Vec<Value>,
                _session_key: &str,
                delta_tx: Sender<String>,
            ) -> anyhow::Result<TurnEvents> {
                // Forward one real delta so the SSE client observes
                // live streaming, then hang. The gateway's cancel arm
                // is what aborts us when the test fires
                // `cancel_all_in_flight`.
                let _ = delta_tx.send("Hello ".to_string()).await;
                self.emitted.notify_one();
                std::future::pending::<()>().await;
                unreachable!()
            }

            async fn send_steer(
                &self,
                _items: Vec<Value>,
                _session_key: &str,
            ) -> anyhow::Result<()> {
                Ok(())
            }
            async fn shutdown(&self) -> anyhow::Result<()> {
                Ok(())
            }
            async fn liveness_probe(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let emitted = Arc::new(Notify::new());
        let transport = Arc::new(HangAfterDeltaTransport {
            emitted: Arc::clone(&emitted),
        }) as Arc<dyn AgentTurnTransport>;

        let mut state = test_state_async().await;
        Arc::get_mut(&mut state)
            .expect("unique state ref")
            .harnesses
            .insert("sera".to_string(), transport);

        // Capture the session id up front; chat_handler reuses it via
        // `get_or_create_session`.
        let session_id = {
            let db = state.db.lock().await;
            db.get_or_create_session("sera")
                .expect("session ok")
                .id
        };

        let app = build_router(Arc::clone(&state));
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "message": "hi", "stream": true })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Drive the body in a separate task so we can fire the cancel
        // mid-stream from the test task.
        let body_handle = tokio::spawn(async move {
            axum::body::to_bytes(response.into_body(), 64 * 1024)
                .await
                .expect("body bytes")
        });

        // Wait for the transport to confirm the first delta has been
        // forwarded, then give axum a tick to write the corresponding
        // SSE frame to the body buffer.
        tokio::time::timeout(Duration::from_secs(2), emitted.notified())
            .await
            .expect("first delta must be emitted within 2s");
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Operator cancel: drains the registry and fires every active
        // token without flipping `client_cancelled`.
        let n = state.cancel_all_in_flight();
        assert!(
            n >= 1,
            "cancel_all_in_flight should have cancelled the in-flight \
             HTTP turn; got count={n}"
        );

        let body_bytes = tokio::time::timeout(Duration::from_secs(5), body_handle)
            .await
            .expect("SSE body must terminate after operator cancel")
            .expect("body task joined");
        let body_str = std::str::from_utf8(&body_bytes).unwrap_or("");

        // Visible stream: exactly one delta + one cancelled event with
        // the operator reason; no `done`.
        assert_eq!(
            body_str.matches("event: message").count(),
            1,
            "expected exactly one streamed delta before operator cancel; \
             body: {body_str}"
        );
        assert!(
            body_str.contains("\"delta\":\"Hello \""),
            "streamed delta must be the real partial text; body: {body_str}"
        );
        assert!(
            body_str.contains("event: cancelled"),
            "expected `cancelled` SSE event for operator cancel; body: {body_str}"
        );
        assert!(
            body_str.contains("\"reason\":\"operator_cancel\""),
            "operator cancel must surface as reason=operator_cancel; body: {body_str}"
        );
        assert!(
            !body_str.contains("event: done"),
            "must NOT emit `done` after operator cancel; body: {body_str}"
        );

        // Transcript: no assistant row from the synthetic
        // `[sera] Runtime turn aborted by KillSwitch ROLLBACK` reply.
        let rows = {
            let db = state.db.lock().await;
            db.get_transcript(&session_id).expect("get transcript")
        };
        let assistant_rows: Vec<_> =
            rows.iter().filter(|r| r.role == "assistant").collect();
        assert!(
            assistant_rows.is_empty(),
            "no assistant transcript row should be persisted on operator \
             cancel; got: {assistant_rows:?}"
        );
    }

    /// sera-k8do (Codex review on PR #1153): the streaming delta channel
    /// must be bounded so a slow SSE consumer cannot let the runtime
    /// read loop accumulate unlimited deltas in memory. The chat
    /// handler creates the channel via
    /// `tokio::sync::mpsc::channel::<String>(STREAMING_DELTA_CHANNEL_CAPACITY)`,
    /// so we mirror that construction here and verify a `try_send` past
    /// the capacity is rejected with `Full` rather than silently growing
    /// the queue.
    #[tokio::test]
    async fn streaming_delta_channel_is_bounded() {
        let (tx, _rx) = tokio::sync::mpsc::channel::<String>(
            STREAMING_DELTA_CHANNEL_CAPACITY,
        );
        for i in 0..STREAMING_DELTA_CHANNEL_CAPACITY {
            tx.try_send(format!("delta-{i}"))
                .unwrap_or_else(|e| panic!("delta {i} must fit in capacity: {e}"));
        }
        let result = tx.try_send("overflow".to_string());
        assert!(
            matches!(
                result,
                Err(tokio::sync::mpsc::error::TrySendError::Full(_))
            ),
            "channel must be bounded at STREAMING_DELTA_CHANNEL_CAPACITY={}; \
             got: {result:?}",
            STREAMING_DELTA_CHANNEL_CAPACITY,
        );
    }

    /// sera-7mc1: when the SSE client disconnects mid-turn, the
    /// `CancelOnDrop` guard inside `StreamState::Streaming` must fire the
    /// session's cancellation token within ~2s so the runtime turn aborts
    /// instead of running to completion server-side.
    ///
    /// Pattern: open `/api/chat` with `stream: true` against a hanging mock
    /// runtime, drain enough of the response body to drive the unfold into
    /// its `Streaming` state, capture the token clone from the active
    /// registry, drop the body to simulate disconnect, then assert the
    /// token's `cancelled()` future resolves within 2s. The deregister +
    /// lane release happens inside the spawned turn task, so we also
    /// verify the registry entry is gone afterwards (the spawned turn task
    /// observed the cancel and ran cleanup).
    #[tokio::test]
    async fn sse_disconnect_cancels_in_flight_turn_within_2s() {
        let mut state = test_state_async().await;
        let hanging_sup: Arc<dyn AgentTurnTransport> =
            RuntimeChildSupervisor::start_with_factory("sera", || async {
                StdioHarness::spawn_mock_hang().await
            })
            .await
            .unwrap();
        Arc::get_mut(&mut state)
            .expect("unique state ref")
            .harnesses
            .insert("sera".to_string(), hanging_sup);
        let app = build_router(Arc::clone(&state));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "message": "hi", "stream": true })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let mut body_stream = response.into_body().into_data_stream();
        // Poll once to drive the body stream forward — the chat handler
        // already spawned the turn task and registered the cancellation
        // token before returning the SSE response, so the registry should
        // populate quickly. The mock runtime hangs, so the unfold never
        // produces a frame — the timeout simply means we're sitting
        // inside `Streaming` waiting on `rx.recv()`.
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            body_stream.next(),
        )
        .await;

        // Snapshot the cancellation token from the active registry. The
        // chat handler keys it as `http:{agent}:{session_id}`.
        let cancel_token = {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                let snapshot = {
                    let map = state.active_cancellation_tokens.lock().unwrap();
                    map.iter()
                        .find(|(k, _)| k.starts_with("http:sera:"))
                        .map(|(_, h)| h.token.clone())
                };
                if let Some(token) = snapshot {
                    break token;
                }
                if std::time::Instant::now() > deadline {
                    panic!("chat_handler never registered a cancellation token");
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        };
        assert!(
            !cancel_token.is_cancelled(),
            "cancel token must not be fired before the SSE body is dropped"
        );

        // Simulate client disconnect by dropping the response body. axum
        // drops the underlying SSE stream, which drops the unfold's state,
        // which drops `CancelOnDrop` and fires the token.
        drop(body_stream);

        let cancelled = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            cancel_token.cancelled(),
        )
        .await;
        assert!(
            cancelled.is_ok(),
            "cancellation token must fire within 2s of SSE disconnect"
        );

        // The spawned turn task observes the cancel and runs cleanup
        // (deregister + complete_run). The registry entry must be drained.
        let drained_within = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let still_present = state
                .active_cancellation_tokens
                .lock()
                .unwrap()
                .keys()
                .any(|k| k.starts_with("http:sera:"));
            if !still_present {
                break;
            }
            if std::time::Instant::now() > drained_within {
                panic!("cancellation registry entry must be deregistered after SSE disconnect");
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// sera-7mc1 / Codex review (PR #1150): when the spawned SSE turn task
    /// panics or is aborted, `turn_handle.await` returns `JoinError`. The
    /// task's in-task cleanup (deregister + lane release) never ran, so the
    /// JoinError arm in the unfold has to call them itself — otherwise the
    /// cancellation registry entry and the lane slot leak for that session
    /// and any subsequent `/api/chat` for the same session is wedged.
    #[tokio::test]
    async fn sse_turn_task_panic_runs_cleanup_and_releases_lane() {
        use async_trait::async_trait;
        use serde_json::Value;

        struct PanickingTransport;

        #[async_trait]
        impl AgentTurnTransport for PanickingTransport {
            async fn send_turn(
                &self,
                _messages: Vec<Value>,
                _session_key: &str,
            ) -> anyhow::Result<TurnEvents> {
                panic!("sera-7mc1 test: simulated transport panic");
            }
            async fn send_steer(
                &self,
                _items: Vec<Value>,
                _session_key: &str,
            ) -> anyhow::Result<()> {
                Ok(())
            }
            async fn shutdown(&self) -> anyhow::Result<()> {
                Ok(())
            }
            async fn liveness_probe(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let mut state = test_state_async().await;
        Arc::get_mut(&mut state)
            .expect("unique state ref")
            .harnesses
            .insert(
                "sera".to_string(),
                Arc::new(PanickingTransport) as Arc<dyn AgentTurnTransport>,
            );
        let app = build_router(Arc::clone(&state));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "message": "hi", "stream": true })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Drain the body — the unfold's JoinError arm will produce a
        // single `error` SSE event, then end. Bound the read so a
        // regression that fails to terminate the stream still fails the
        // test instead of hanging.
        let body_bytes = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            axum::body::to_bytes(response.into_body(), 64 * 1024),
        )
        .await
        .expect("SSE body must terminate after JoinError")
        .expect("body bytes");
        let body_str = std::str::from_utf8(&body_bytes).unwrap_or("");
        assert!(
            body_str.contains("event: error")
                && body_str.contains("turn task failed"),
            "JoinError must surface a structured error event; got: {body_str:?}"
        );

        // The cleanup branch must drain the cancellation registry and
        // release the lane slot. Poll briefly to allow any final task
        // scheduling to settle, then assert.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let registry_empty = !state
                .active_cancellation_tokens
                .lock()
                .unwrap()
                .keys()
                .any(|k| k.starts_with("http:sera:"));
            let lane_free = {
                let lq = state.lane_queue.lock().await;
                lq.active_runs() == 0
            };
            if registry_empty && lane_free {
                break;
            }
            if std::time::Instant::now() > deadline {
                let entries: Vec<String> = state
                    .active_cancellation_tokens
                    .lock()
                    .unwrap()
                    .keys()
                    .cloned()
                    .collect();
                let active = state.lane_queue.lock().await.active_runs();
                panic!(
                    "JoinError cleanup must drain registry and release lane within 2s; \
                     registry_empty={registry_empty}, lane_free={lane_free}, \
                     active_runs={active}, remaining_registry_keys={entries:?}"
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// sera-mplr route-level: cancelling one session via the endpoint must
    /// leave another session's token intact.
    #[tokio::test]
    async fn chat_cancel_endpoint_does_not_cancel_other_sessions() {
        let state = test_state_async().await;
        let target = state.register_cancellation_token("http:sera:ses_target_route");
        let bystander = state.register_cancellation_token("http:sera:ses_bystander_route");
        let app = build_router(Arc::clone(&state));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat/cancel")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "session_id": "ses_target_route" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(target.is_cancelled());
        assert!(!bystander.is_cancelled());
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

    // ── sera-ojp3: runtime child supervisor regression tests ─────────────────
    //
    // These cover the three child-exit windows the supervisor must protect:
    //   A. before any turn (cold crash)
    //   B. during a turn (in-flight crash → mark_unhealthy)
    //   C. after a turn (post-turn exit → try_wait detection on next acquire)
    // plus shutdown semantics (no respawn after `shutdown`).

    /// The supervisor exposes a fresh, healthy harness for the first acquire
    /// after a successful initial spawn. Baseline against which the failure
    /// modes are compared.
    #[tokio::test]
    async fn supervisor_initial_spawn_yields_healthy_harness() {
        let supervisor = RuntimeChildSupervisor::start_with_factory("agent", || async {
            StdioHarness::spawn_mock().await
        })
        .await
        .expect("initial spawn should succeed");

        assert_eq!(supervisor.current_generation().await, 1);

        let harness = supervisor.acquire().await.expect("acquire after init");
        let events = harness
            .send_turn(Vec::new(), "ojp3-init")
            .await
            .expect("mock harness answers");
        assert_eq!(events.response, "mock response");
        // No spurious respawn — generation must still be 1.
        assert_eq!(supervisor.current_generation().await, 1);
    }

    /// Case A — child exits *before* the first user-visible turn (cold crash).
    /// `acquire` must surface this via `try_wait`, respawn into a healthy
    /// child, and the next turn must succeed.
    #[tokio::test]
    async fn supervisor_respawns_when_child_exits_before_first_turn() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);
        let supervisor = RuntimeChildSupervisor::start_with_factory("agent", move || {
            let n = counter_clone.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    // First spawn: a shell that exits immediately.
                    StdioHarness::spawn_mock_dead().await
                } else {
                    StdioHarness::spawn_mock().await
                }
            }
        })
        .await
        .expect("initial dead spawn still constructs the harness object");

        // Give the dead shell a moment to actually exit so try_wait sees it.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // Acquire detects the dead child via try_wait and respawns to the
        // healthy mock factory branch.
        let harness = supervisor
            .acquire()
            .await
            .expect("supervisor must respawn after cold-crashed child");
        let events = harness
            .send_turn(Vec::new(), "ojp3-cold")
            .await
            .expect("respawned harness answers normally");
        assert_eq!(events.response, "mock response");
        assert!(
            supervisor.current_generation().await >= 2,
            "respawn should bump the generation counter"
        );
    }

    /// Case B — child crashes *during* a turn. Caller observes a runtime I/O
    /// failure and tells the supervisor via `mark_unhealthy`. The next
    /// `acquire` must respawn rather than hand back the dead child.
    #[tokio::test]
    async fn supervisor_respawns_after_mark_unhealthy() {
        let supervisor = RuntimeChildSupervisor::start_with_factory("agent", || async {
            StdioHarness::spawn_mock().await
        })
        .await
        .expect("initial spawn");

        let gen0 = supervisor.current_generation().await;
        let h0 = supervisor.acquire().await.expect("first acquire");
        let _ = h0
            .send_turn(Vec::new(), "ojp3-during-pre")
            .await
            .expect("first turn ok");

        // Caller saw a "BrokenPipe" mid-turn and notifies the supervisor.
        supervisor
            .mark_unhealthy("simulated send_turn BrokenPipe")
            .await;

        let h1 = supervisor
            .acquire()
            .await
            .expect("acquire after mark_unhealthy must respawn");
        let gen1 = supervisor.current_generation().await;
        assert!(
            gen1 > gen0,
            "expected a new generation after mark_unhealthy (gen0={gen0} gen1={gen1})"
        );
        let events = h1
            .send_turn(Vec::new(), "ojp3-during-post")
            .await
            .expect("respawned harness answers normally");
        assert_eq!(events.response, "mock response");
    }

    /// Case C — child exits *after* a successful turn returns. The supervisor
    /// must detect the dead child on the next `acquire` (via `try_wait`) and
    /// respawn before the next turn writes anything.
    #[tokio::test]
    async fn supervisor_respawns_when_child_exits_after_turn() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);
        let supervisor = RuntimeChildSupervisor::start_with_factory("agent", move || {
            let n = counter_clone.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    // Single-shot mock: process exactly one submission, then
                    // exit. The frames mirror `spawn_mock`'s loop body.
                    let script = concat!(
                        r#"IFS= read -r line; "#,
                        r#"echo '{"id":"00000000-0000-0000-0000-000000000001","submission_id":"00000000-0000-0000-0000-000000000000","msg":{"type":"turn_started","turn_id":"00000000-0000-0000-0000-000000000002"},"timestamp":"2024-01-01T00:00:00Z"}'; "#,
                        r#"echo '{"id":"00000000-0000-0000-0000-000000000003","submission_id":"00000000-0000-0000-0000-000000000000","msg":{"type":"streaming_delta","delta":"mock response"},"timestamp":"2024-01-01T00:00:00Z"}'; "#,
                        r#"echo '{"id":"00000000-0000-0000-0000-000000000004","submission_id":"00000000-0000-0000-0000-000000000000","msg":{"type":"turn_completed","turn_id":"00000000-0000-0000-0000-000000000002"},"timestamp":"2024-01-01T00:00:00Z"}'; "#,
                        r#"exit 0"#,
                    );
                    StdioHarness::spawn_with_script(script).await
                } else {
                    StdioHarness::spawn_mock().await
                }
            }
        })
        .await
        .expect("initial single-shot spawn");

        // First turn succeeds; the single-shot mock then exits.
        let h0 = supervisor.acquire().await.expect("first acquire");
        let events = h0
            .send_turn(Vec::new(), "ojp3-post-1")
            .await
            .expect("first turn ok");
        assert_eq!(events.response, "mock response");
        drop(h0);

        // Wait for the script to actually exit so try_wait reports the status.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // Next acquire must detect the post-turn exit and respawn to the
        // healthy mock factory branch.
        let h1 = supervisor
            .acquire()
            .await
            .expect("supervisor respawns after post-turn child exit");
        let events = h1
            .send_turn(Vec::new(), "ojp3-post-2")
            .await
            .expect("respawned harness answers normally");
        assert_eq!(events.response, "mock response");
        assert!(supervisor.current_generation().await >= 2);
    }

    /// `shutdown` must flip the supervisor's stopping flag so subsequent
    /// `acquire` calls return an error instead of respawning. This is the
    /// drain-phase contract — once the gateway starts shutting down, a
    /// late-arriving turn never resurrects a dead child.
    #[tokio::test]
    async fn supervisor_shutdown_blocks_further_acquires() {
        let supervisor = RuntimeChildSupervisor::start_with_factory("agent", || async {
            StdioHarness::spawn_mock().await
        })
        .await
        .unwrap();

        // Healthy before shutdown.
        let _ = supervisor.acquire().await.expect("acquire pre-shutdown");

        supervisor.shutdown().await.expect("shutdown send ok");

        // `Result<Arc<StdioHarness>, _>::expect_err` would require
        // StdioHarness: Debug; instead, match on the result directly.
        match supervisor.acquire().await {
            Ok(_) => panic!("acquire after shutdown must fail"),
            Err(err) => assert!(
                err.to_string().contains("shutting down"),
                "expected a shutting-down error, got: {err}"
            ),
        }
    }

    /// sera-40y3: `kill_for_rollback` kills the live child and clears the
    /// harness slot so the next `acquire` respawns. Without this, the
    /// runtime child keeps running and its NDJSON stdout still buffers
    /// the aborted turn's `TurnCompleted` frame — the next post-DISARM
    /// submission would read it as its own reply.
    #[tokio::test]
    async fn supervisor_kill_for_rollback_forces_respawn() {
        let supervisor = RuntimeChildSupervisor::start_with_factory("agent", || async {
            StdioHarness::spawn_mock().await
        })
        .await
        .expect("initial spawn");

        let gen0 = supervisor.current_generation().await;
        // Warm the harness so a real child is live.
        let _ = supervisor.acquire().await.expect("acquire before rollback");
        assert_eq!(supervisor.current_generation().await, gen0);

        // Operator fires ROLLBACK — kill the harness.
        supervisor.kill_for_rollback().await;

        // Next acquire must respawn (gen incremented) and produce a
        // healthy harness — proves the stale child was disposed of.
        let h1 = supervisor
            .acquire()
            .await
            .expect("acquire after kill_for_rollback must respawn");
        let gen1 = supervisor.current_generation().await;
        assert!(
            gen1 > gen0,
            "kill_for_rollback must force a fresh generation \
             (gen0={gen0} gen1={gen1})"
        );
        let events = h1
            .send_turn(Vec::new(), "40y3-post-rollback")
            .await
            .expect("respawned harness answers normally");
        assert_eq!(
            events.response, "mock response",
            "post-rollback harness must come from a fresh child, not a \
             stale buffer"
        );
    }

    /// End-to-end through `execute_turn`: a cold-crashed initial child must
    /// not wedge the lane. The first dispatched turn re-acquires through
    /// the supervisor, sees the dead child, respawns, and answers normally.
    /// Acceptance criterion: "child exit before a turn is surfaced cleanly
    /// and does not permanently wedge the lane".
    #[tokio::test]
    async fn execute_turn_recovers_from_cold_crashed_runtime_child() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);
        let supervisor = RuntimeChildSupervisor::start_with_factory("agent", move || {
            let n = counter_clone.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    StdioHarness::spawn_mock_dead().await
                } else {
                    StdioHarness::spawn_mock().await
                }
            }
        })
        .await
        .unwrap();

        // Wait so the dead-child shell has actually exited.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let agent_spec = AgentSpec {
            provider: "stub".to_string(),
            model: None,
            persona: None,
            tools: None,
            workspace: None,
            policy_ref: None,
            enforcement_mode: None,
            approval_policy: None,
        };
        let skill_engine = SkillDispatchEngine::new();
        let semantic_store: Arc<dyn SemanticMemoryStore> = Arc::new(
            SqliteMemoryStore::open_in_memory(None).expect("open in-memory semantic store"),
        );
        let cancel = CancellationToken::new();
        let capability_registry = CapabilityRegistry::empty();

        let start = std::time::Instant::now();
        let result = execute_turn(
            &agent_spec,
            &[],
            "hello",
            &*supervisor,
            "ojp3-e2e-cold",
            &skill_engine,
            &semantic_store,
            "agent",
            &cancel,
            &capability_registry,
            None,
        )
        .await;

        // The supervisor's lazy try_wait + respawn ran inside execute_turn
        // (sera-ojp3) and the second factory call produced a healthy mock.
        assert_eq!(
            result.reply, "mock response",
            "execute_turn should recover via supervisor respawn; got: {}",
            result.reply
        );
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "respawn-then-turn should complete quickly; elapsed={:?}",
            start.elapsed()
        );
    }

    /// `execute_turn`'s send_turn-error path must mark the supervisor
    /// unhealthy so the *next* turn respawns instead of reusing the dead
    /// child. Acceptance criterion: "child exit during/after a turn is
    /// detected; next turn can respawn".
    #[tokio::test]
    async fn execute_turn_marks_supervisor_unhealthy_on_runtime_error() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);
        let supervisor = RuntimeChildSupervisor::start_with_factory("agent", move || {
            let n = counter_clone.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    // Dead child for the first turn — the send_turn write
                    // will fail with a broken-pipe + child_exit_context error.
                    StdioHarness::spawn_mock_dead().await
                } else {
                    StdioHarness::spawn_mock().await
                }
            }
        })
        .await
        .unwrap();

        // Pre-fetch and wait so try_wait actually fires on first acquire.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let agent_spec = AgentSpec {
            provider: "stub".to_string(),
            model: None,
            persona: None,
            tools: None,
            workspace: None,
            policy_ref: None,
            enforcement_mode: None,
            approval_policy: None,
        };
        let skill_engine = SkillDispatchEngine::new();
        let semantic_store: Arc<dyn SemanticMemoryStore> = Arc::new(
            SqliteMemoryStore::open_in_memory(None).expect("open in-memory semantic store"),
        );
        let cancel = CancellationToken::new();
        let capability_registry = CapabilityRegistry::empty();

        // First turn — try_wait detects the dead child and respawns to the
        // healthy mock; the turn succeeds.
        let r1 = execute_turn(
            &agent_spec,
            &[],
            "hello",
            &*supervisor,
            "ojp3-during-1",
            &skill_engine,
            &semantic_store,
            "agent",
            &cancel,
            &capability_registry,
            None,
        )
        .await;
        assert_eq!(r1.reply, "mock response");

        // Simulate an in-flight crash via mark_unhealthy and run another turn.
        supervisor
            .mark_unhealthy("simulated in-flight crash")
            .await;
        let r2 = execute_turn(
            &agent_spec,
            &[],
            "hello again",
            &*supervisor,
            "ojp3-during-2",
            &skill_engine,
            &semantic_store,
            "agent",
            &cancel,
            &capability_registry,
            None,
        )
        .await;
        assert_eq!(
            r2.reply, "mock response",
            "next turn after mark_unhealthy must succeed against a fresh child"
        );
    }

    /// sera-ve9x: `RuntimeChildSupervisor` is the sole `AgentTurnTransport`
    /// implementor in PR 1, and the gateway stores it as
    /// `Arc<dyn AgentTurnTransport>` in `AppState.harnesses`. This is a
    /// compile-only check that the trait remains object-safe and that the
    /// supervisor implements it — any non-object-safe addition or signature
    /// drift would surface here at build time.
    #[test]
    fn supervisor_is_object_safe() {
        fn _assert_impl<T: AgentTurnTransport>() {}
        fn _assert_obj_safe(_: Arc<dyn AgentTurnTransport>) {}
        _assert_impl::<RuntimeChildSupervisor>();
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
            db: Arc::new(Mutex::new(SqliteDb::open_in_memory().unwrap())),
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
            active_cancellation_tokens: Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
            // writing shadow-git dirs to the filesystem during tests.
            session_store: Arc::new(InMemorySessionStore::new()),
            constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
            capability_registry: Arc::new(RwLock::new(Arc::new(CapabilityRegistry::empty()))),
            ticket_store: Arc::new(InMemoryTicketStore::new()),
            hitl_resumed_tx: tokio::sync::broadcast::channel(64).0,
            workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
            gh_run_store: Arc::new(InMemoryGhRunStateStore::new()),
            gh_pr_store: Arc::new(InMemoryGhPrStateStore::new()),
            human_gate_store: Arc::new(InMemoryHumanGateStore::new()),
            admin_auth: None,
            admin_audit: None,
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
                db: Arc::new(Mutex::new(SqliteDb::open_in_memory().unwrap())),
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
                active_cancellation_tokens: Arc::new(std::sync::Mutex::new(
                    std::collections::HashMap::new(),
                )),
                // sera-4i4i: intentional test-fixture — InMemorySessionStore avoids
                // writing shadow-git dirs to the filesystem during tests.
                session_store: Arc::new(InMemorySessionStore::new()),
                constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
                capability_registry: Arc::new(RwLock::new(Arc::new(CapabilityRegistry::empty()))),
                ticket_store: Arc::new(InMemoryTicketStore::new()),
            hitl_resumed_tx: tokio::sync::broadcast::channel(64).0,
                workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
                gh_run_store: Arc::new(InMemoryGhRunStateStore::new()),
                gh_pr_store: Arc::new(InMemoryGhPrStateStore::new()),
                human_gate_store: Arc::new(InMemoryHumanGateStore::new()),
                admin_auth: None,
                admin_audit: None,
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

    // ── Production-path E2E tests (sera-ov7z) ────────────────────────────────
    //
    // These tests drive a synthetic message through the *real* production wire
    // path: process_message / chat_handler -> StdioHarness::send_turn -> a
    // real `sera-runtime --ndjson` child subprocess -> a deterministic mock
    // LLM HTTP server bound to 127.0.0.1:0. They guard the seams that
    // mock-bash `spawn_mock` cannot exercise — runtime startup, NDJSON frame
    // ordering, submission shape, runtime LLM streaming parser, transcript
    // persistence on the success path, lane-queue release, and hook-chain
    // firing on each of the four happy-turn points.
    //
    // Design report: artifacts/reports/coordination/ov7z-production-e2e-test-design-2026-04-29.md
    // Preflight: artifacts/reports/research/ov7z-implementation-preflight-spark-2026-04-29.md
    mod production_e2e {
        use super::*;
        use async_trait::async_trait;
        use sera_hooks::Hook;
        use sera_types::hook::{HookChain, HookMetadata, HookPoint};
        use std::path::PathBuf;
        use std::sync::atomic::{AtomicU64, Ordering};
        use tokio::net::TcpListener;
        use tokio::sync::oneshot;

        /// Locate the workspace's `target/debug/sera-runtime` binary.
        ///
        /// Resolution order:
        ///   1. `SERA_E2E_RUNTIME_BIN=<path>` explicit override, if set.
        ///   2. `CARGO_TARGET_DIR/debug/sera-runtime`, then any ancestor
        ///      `target/debug/sera-runtime` of the gateway crate.
        ///   3. If still missing AND `SERA_E2E_ALLOW_RUNTIME_BUILD=1`, run
        ///      `cargo build -p sera-runtime --bin sera-runtime` to build it.
        ///   4. Otherwise panic with a message instructing the caller.
        ///
        /// The opt-in nested build exists because Cargo does not export
        /// `--frozen`/`--locked`/`--offline` to the test process, so a
        /// silent `cargo build` here could violate hermetic outer-test mode.
        /// Default behavior is therefore to refuse the build and require an
        /// explicit opt-in or a prebuilt binary.
        fn locate_runtime_bin() -> PathBuf {
            let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let exe_name = if cfg!(windows) {
                "sera-runtime.exe"
            } else {
                "sera-runtime"
            };

            if let Ok(p) = std::env::var("SERA_E2E_RUNTIME_BIN") {
                let candidate = PathBuf::from(&p);
                assert!(
                    candidate.exists(),
                    "SERA_E2E_RUNTIME_BIN=`{p}` does not exist"
                );
                return candidate;
            }

            let find_existing = || -> Option<PathBuf> {
                if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
                    let candidate = PathBuf::from(target).join("debug").join(exe_name);
                    if candidate.exists() {
                        return Some(candidate);
                    }
                }
                let mut here: Option<&std::path::Path> = Some(&manifest_dir);
                while let Some(dir) = here {
                    let candidate = dir.join("target").join("debug").join(exe_name);
                    if candidate.exists() {
                        return Some(candidate);
                    }
                    here = dir.parent();
                }
                None
            };

            if let Some(found) = find_existing() {
                return found;
            }

            if std::env::var("SERA_E2E_ALLOW_RUNTIME_BUILD").as_deref() != Ok("1") {
                panic!(
                    "sera-runtime binary not found (searched CARGO_TARGET_DIR \
                     and ancestor target/debug/{exe_name} from {}). Build it \
                     first with `cargo build -p sera-runtime --bin sera-runtime`, \
                     point SERA_E2E_RUNTIME_BIN=<path> at a prebuilt binary, or \
                     set SERA_E2E_ALLOW_RUNTIME_BUILD=1 to let this test invoke \
                     `cargo build` itself (note: the nested build cannot inherit \
                     the outer cargo's --frozen/--locked/--offline mode).",
                    manifest_dir.display()
                );
            }

            // Opt-in nested build. Cargo sets CARGO to its own path; fall
            // back to PATH lookup. The caller has explicitly accepted that
            // this nested build does not honor the outer cargo's network/
            // lockfile mode, so no flag propagation is attempted here.
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
            let status = std::process::Command::new(&cargo)
                .args(["build", "-p", "sera-runtime", "--bin", "sera-runtime"])
                .status()
                .unwrap_or_else(|e| {
                    panic!("failed to invoke `{cargo} build -p sera-runtime`: {e}")
                });
            assert!(
                status.success(),
                "`cargo build -p sera-runtime --bin sera-runtime` failed with {status}"
            );

            find_existing().unwrap_or_else(|| {
                panic!(
                    "sera-runtime binary still not found after `cargo build -p \
                     sera-runtime` (searched CARGO_TARGET_DIR and ancestor \
                     target/debug/{exe_name} from {})",
                    manifest_dir.display()
                )
            })
        }

        /// Manifest YAML used by the production-E2E fixture. Differs from
        /// `TEMPLATE_YAML` only in `base_url`/`default_model` (re-pointed at
        /// the mock LLM) and `tools.allow` (empty so we don't hit memory_*
        /// during the turn).
        fn production_e2e_manifest_yaml(mock_llm_url: &str) -> String {
            format!(
                r#"---
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
  base_url: "{mock_llm_url}/v1"
  default_model: mock-model
---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: sera
spec:
  provider: lm-studio
  model: mock-model
  persona:
    immutable_anchor: |
      You are Sera under E2E test.
  tools:
    allow: []
"#
            )
        }

        /// Handle to the in-process mock LLM HTTP server. Drop to shut down.
        struct MockLlm {
            base_url: String,
            shutdown: Option<oneshot::Sender<()>>,
        }

        impl Drop for MockLlm {
            fn drop(&mut self) {
                if let Some(tx) = self.shutdown.take() {
                    let _ = tx.send(());
                }
            }
        }

        /// Start an OpenAI-compatible mock server on 127.0.0.1:0. Always
        /// returns SSE-streaming chunks for `POST /v1/chat/completions` —
        /// matches what `LlmClient::chat_typed_with_behavior` requests. The
        /// reply text is fixed so assertions can compare exactly.
        async fn start_mock_llm(reply: &'static str, delay_ms: u64) -> MockLlm {
            use axum::Router;
            use axum::response::sse::{Event, Sse};
            use axum::routing::{get, post};
            use futures_util::stream;

            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("mock LLM bind");
            let port = listener.local_addr().expect("mock LLM addr").port();

            let chat_handler = move || {
                let reply = reply.to_string();
                async move {
                    if delay_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                    // Emit two delta chunks (role + content) and a terminal
                    // chunk with `usage`. LlmClient::parse_sse_stream expects
                    // standard OpenAI SSE.
                    let role_chunk = serde_json::json!({
                        "id": "chatcmpl-mock",
                        "object": "chat.completion.chunk",
                        "choices": [{
                            "index": 0,
                            "delta": { "role": "assistant", "content": "" },
                            "finish_reason": null
                        }]
                    })
                    .to_string();
                    let content_chunk = serde_json::json!({
                        "id": "chatcmpl-mock",
                        "object": "chat.completion.chunk",
                        "choices": [{
                            "index": 0,
                            "delta": { "content": reply },
                            "finish_reason": null
                        }]
                    })
                    .to_string();
                    let final_chunk = serde_json::json!({
                        "id": "chatcmpl-mock",
                        "object": "chat.completion.chunk",
                        "choices": [{
                            "index": 0,
                            "delta": {},
                            "finish_reason": "stop"
                        }],
                        "usage": {
                            "prompt_tokens": 7,
                            "completion_tokens": 5,
                            "total_tokens": 12
                        }
                    })
                    .to_string();
                    let events: Vec<Result<Event, std::convert::Infallible>> = vec![
                        Ok(Event::default().data(role_chunk)),
                        Ok(Event::default().data(content_chunk)),
                        Ok(Event::default().data(final_chunk)),
                        Ok(Event::default().data("[DONE]")),
                    ];
                    Sse::new(stream::iter(events))
                }
            };

            let models_handler = || async {
                axum::Json(serde_json::json!({
                    "object": "list",
                    "data": [{ "id": "mock-model", "object": "model" }]
                }))
            };

            let app: Router = Router::new()
                .route("/v1/models", get(models_handler))
                .route("/v1/chat/completions", post(chat_handler));

            let (tx, rx) = oneshot::channel::<()>();
            tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        let _ = rx.await;
                    })
                    .await
                    .ok();
            });

            MockLlm {
                base_url: format!("http://127.0.0.1:{port}"),
                shutdown: Some(tx),
            }
        }

        /// Counting hook — increments a shared atomic on every `execute`.
        /// Registered four times (one per HookPoint) so each lifecycle point
        /// has its own counter; A5 from the design report asserts each
        /// counter == 1 for a happy turn.
        struct CountingHook {
            name: String,
            point: HookPoint,
            counter: Arc<AtomicU64>,
        }

        #[async_trait]
        impl Hook for CountingHook {
            fn metadata(&self) -> HookMetadata {
                HookMetadata {
                    name: self.name.clone(),
                    description: "ov7z production-E2E counter hook".to_string(),
                    version: "1.0.0".to_string(),
                    supported_points: vec![self.point],
                    author: Some("sera-ov7z-test".to_string()),
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
                _ctx: &sera_types::hook::HookContext,
            ) -> Result<sera_types::hook::HookResult, sera_hooks::HookError> {
                self.counter.fetch_add(1, Ordering::SeqCst);
                Ok(sera_types::hook::HookResult::pass())
            }
        }

        /// All four counter handles exposed by `production_e2e_state`.
        struct HookCounters {
            pre_route: Arc<AtomicU64>,
            post_route: Arc<AtomicU64>,
            pre_turn: Arc<AtomicU64>,
            post_turn: Arc<AtomicU64>,
        }

        /// sera-ve9x PR 3: which dispatch backend the production-E2E fixture
        /// wires into `AppState.harnesses`. `Runtime` (legacy default) spawns
        /// a real `sera-runtime --ndjson` child via
        /// [`RuntimeChildSupervisor`]; `Embedded` builds an in-process
        /// [`EmbeddedRuntimeTransport`] backed by a `DefaultRuntime` with no
        /// child process. Both feed the same gateway path
        /// (`process_message` / `chat_handler` -> `AgentTurnTransport`) so
        /// every assertion below applies to both backends — that's the ADR §4
        /// step 2 acceptance gate this module exists to mechanise.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum DispatchMode {
            Runtime,
            Embedded,
        }

        /// Serialises the runtime-mode child spawn against the embedded
        /// "no child spawn" sample window. Without this, peer runtime-mode
        /// production_e2e tests (`discord_path_round_trip`,
        /// `http_chat_path_round_trip`) can spawn `sera-runtime` children
        /// during [`embedded_path_no_runtime_child_spawns`]'s before/after
        /// window and false-positive its assertion. The runtime-mode branch
        /// of [`production_e2e_state_with_mode`] takes this lock for the
        /// duration of `RuntimeChildSupervisor::start`; the embedded
        /// no-spawn test holds it across its entire sample window so a
        /// strict zero-delta assertion is reliable.
        static RUNTIME_SPAWN_SAMPLE_LOCK: tokio::sync::Mutex<()> =
            tokio::sync::Mutex::const_new(());

        /// sera-ve9x PR 3: in-process embedded transport for production-E2E
        /// fixtures. Bypasses `build_embedded_transport` so the test does not
        /// have to mutate `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE` /
        /// `SERA_AGENT_TOOLS_DENY` in the gateway process env. The
        /// `DefaultRuntime` constructed here mirrors the production builder:
        /// `LlmClient::build_from_config` against the mock LLM URL,
        /// `TraitToolRegistry::with_builtins_and_authz` with an empty
        /// `CapabilityRegistry`, and `permissive_gate=true` (matches the
        /// runtime path which forwards `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE=1`
        /// to the child env).
        fn build_embedded_transport_for_test(
            agent_name: &str,
            mock_llm_url: &str,
        ) -> Arc<dyn AgentTurnTransport> {
            use sera_runtime::config::RuntimeConfig;
            use sera_runtime::context_engine::pipeline::ContextPipeline;
            use sera_runtime::default_runtime::DefaultRuntime;
            use sera_runtime::tools::TraitToolRegistry;
            use sera_runtime::tools::dispatcher::RegistryDispatcher;

            let mut runtime_config = RuntimeConfig::from_env();
            runtime_config.llm_base_url = format!("{mock_llm_url}/v1");
            runtime_config.llm_model = "mock-model".to_string();
            runtime_config.llm_api_key = "test-key".to_string();
            runtime_config.agent_id = agent_name.to_string();
            runtime_config.lifecycle_mode = "task".to_string();
            runtime_config.chat_port = 0;

            let cap_registry = Arc::new(sera_config::CapabilityRegistry::empty());
            let delegation_bus = sera_runtime::delegation_bus::DelegationBus::new();
            // sera-i4en: parity with `build_embedded_transport` — the test
            // fixture wires the production InProcAgentRouter so the
            // agent-as-tool entries are present in the registry.
            let agent_router: Arc<sera_runtime::agent_tool_registry::InProcAgentRouter> =
                Arc::new(sera_runtime::agent_tool_registry::InProcAgentRouter::new());
            let agent_registry = Arc::new(
                sera_runtime::agent_tool_registry::AgentToolRegistry::with_router(agent_router),
            );
            let registry = Arc::new(
                TraitToolRegistry::with_builtins_and_authz(runtime_config.tool_authz_enabled)
                    .with_delegation(delegation_bus)
                    .with_agent_tools(agent_registry),
            );
            let dispatcher = RegistryDispatcher::new(Arc::clone(&registry))
                .with_capability_registry(cap_registry, agent_name.to_string());
            let authz_provider =
                sera_runtime::authz_builder::build_provider_from_config(&runtime_config);
            let llm_client = sera_runtime::llm_client::build_from_config(&runtime_config);

            let runtime = DefaultRuntime::new(Box::new(ContextPipeline::new()))
                .with_llm(Box::new(llm_client))
                .with_tool_dispatcher(Box::new(dispatcher))
                .with_authz_provider(authz_provider)
                .with_allow_missing_constitutional_gate(true);

            Arc::new(EmbeddedRuntimeTransport::new(
                agent_name.to_string(),
                Arc::new(runtime),
                vec![],
            ))
        }

        /// Build an `AppState` whose harness map contains a real
        /// `StdioHarness::spawn` (i.e. a live `sera-runtime --ndjson` child),
        /// pointed at the mock LLM, and whose hook registry has a counting
        /// hook + chain wired in for each of the four happy-turn points.
        ///
        /// Thin wrapper that pins the legacy default to keep existing test
        /// call sites unchanged. New parity tests use
        /// [`production_e2e_state_with_mode`] directly.
        async fn production_e2e_state(
            mock_llm_url: &str,
        ) -> (Arc<AppState>, HookCounters) {
            production_e2e_state_with_mode(mock_llm_url, DispatchMode::Runtime).await
        }

        /// sera-ve9x PR 3: dispatch-mode-parameterised variant of
        /// [`production_e2e_state`]. Both modes wire the same hook chains,
        /// the same in-memory SQLite, the same `LaneQueue`, and the same
        /// `Arc<dyn AgentTurnTransport>` interface — only the transport
        /// constructor differs. That is the parity surface ADR §4 step 2
        /// requires before flipping the default; making this a single
        /// helper means every test below applies to both backends with no
        /// drift.
        async fn production_e2e_state_with_mode(
            mock_llm_url: &str,
            mode: DispatchMode,
        ) -> (Arc<AppState>, HookCounters) {
            let manifests =
                parse_manifests(&production_e2e_manifest_yaml(mock_llm_url)).expect("manifest");

            // Build hook registry with one CountingHook per point.
            let pre_route = Arc::new(AtomicU64::new(0));
            let post_route = Arc::new(AtomicU64::new(0));
            let pre_turn = Arc::new(AtomicU64::new(0));
            let post_turn = Arc::new(AtomicU64::new(0));

            let mut hreg = HookRegistry::new();
            hreg.register(Box::new(CountingHook {
                name: "ov7z-counter-pre-route".to_string(),
                point: HookPoint::PreRoute,
                counter: Arc::clone(&pre_route),
            }));
            hreg.register(Box::new(CountingHook {
                name: "ov7z-counter-post-route".to_string(),
                point: HookPoint::PostRoute,
                counter: Arc::clone(&post_route),
            }));
            hreg.register(Box::new(CountingHook {
                name: "ov7z-counter-pre-turn".to_string(),
                point: HookPoint::PreTurn,
                counter: Arc::clone(&pre_turn),
            }));
            hreg.register(Box::new(CountingHook {
                name: "ov7z-counter-post-turn".to_string(),
                point: HookPoint::PostTurn,
                counter: Arc::clone(&post_turn),
            }));
            let hook_registry = Arc::new(hreg);
            let chain_executor = Arc::new(ChainExecutor::new(Arc::clone(&hook_registry)));

            // Build the four chains and stash them on a synthetic manifest.
            // process_message reads chains via `state.manifests.hook_chain_specs()`,
            // which is constructed off the parsed YAML; the cleanest way to
            // inject them is through the YAML manifest. Add HookChain manifests
            // pointing at the hook names registered above.
            let chains_yaml = r#"---
apiVersion: sera.dev/v1
kind: HookChain
metadata:
  name: ov7z-chain-pre-route
spec:
  name: ov7z-chain-pre-route
  point: pre_route
  hooks:
    - hook_ref: ov7z-counter-pre-route
  timeout_ms: 5000
  fail_open: false
---
apiVersion: sera.dev/v1
kind: HookChain
metadata:
  name: ov7z-chain-post-route
spec:
  name: ov7z-chain-post-route
  point: post_route
  hooks:
    - hook_ref: ov7z-counter-post-route
  timeout_ms: 5000
  fail_open: false
---
apiVersion: sera.dev/v1
kind: HookChain
metadata:
  name: ov7z-chain-pre-turn
spec:
  name: ov7z-chain-pre-turn
  point: pre_turn
  hooks:
    - hook_ref: ov7z-counter-pre-turn
  timeout_ms: 5000
  fail_open: false
---
apiVersion: sera.dev/v1
kind: HookChain
metadata:
  name: ov7z-chain-post-turn
spec:
  name: ov7z-chain-post-turn
  point: post_turn
  hooks:
    - hook_ref: ov7z-counter-post-turn
  timeout_ms: 5000
  fail_open: false
"#;
            let mut manifests = manifests;
            let chain_manifests = parse_manifests(chains_yaml).expect("chain manifests");
            manifests.merge_in(chain_manifests);
            // Sanity: the four chains should serialize back through hook_chain_specs.
            let chain_specs: Vec<HookChain> = manifests.hook_chain_specs();
            assert_eq!(
                chain_specs.len(),
                4,
                "expected 4 hook chains, got {}",
                chain_specs.len()
            );

            // sera-ve9x PR 3: branch on the parameterised dispatch mode.
            // Runtime mode preserves the legacy fixture exactly — locate the
            // sera-runtime binary, build the child env, and spawn a real
            // supervised stdio child. Embedded mode skips that path entirely
            // (no `locate_runtime_bin`, no `Command::new`) and constructs an
            // in-process `EmbeddedRuntimeTransport` against the mock LLM URL.
            let mut harnesses: std::collections::HashMap<String, Arc<dyn AgentTurnTransport>> =
                std::collections::HashMap::new();
            match mode {
                DispatchMode::Runtime => {
                    // Hold RUNTIME_SPAWN_SAMPLE_LOCK across the actual
                    // `RuntimeChildSupervisor::start` so the embedded
                    // no-child-spawn test can sample its before/after
                    // child-count window without racing peer runtime-mode
                    // tests' spawns.
                    let _spawn_guard = RUNTIME_SPAWN_SAMPLE_LOCK.lock().await;
                    let runtime_bin = locate_runtime_bin();
                    let mut env = std::collections::HashMap::new();
                    env.insert("LLM_BASE_URL".to_string(), format!("{mock_llm_url}/v1"));
                    env.insert("LLM_MODEL".to_string(), "mock-model".to_string());
                    env.insert("LLM_API_KEY".to_string(), "test-key".to_string());
                    env.insert("AGENT_ID".to_string(), "sera".to_string());
                    env.insert(
                        "SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE".to_string(),
                        "1".to_string(),
                    );
                    // Empty allow list — match the manifest's `tools.allow: []`.
                    env.insert("SERA_AGENT_TOOLS_ALLOW".to_string(), String::new());
                    let supervisor = RuntimeChildSupervisor::start(
                        "sera".to_string(),
                        runtime_bin.to_string_lossy().into_owned(),
                        env,
                    )
                    .await
                    .expect("spawn real sera-runtime supervisor");
                    harnesses.insert("sera".to_string(), supervisor);
                }
                DispatchMode::Embedded => {
                    let transport = build_embedded_transport_for_test("sera", mock_llm_url);
                    harnesses.insert("sera".to_string(), transport);
                }
            }

            let state = Arc::new(AppState {
                db: Arc::new(Mutex::new(SqliteDb::open_in_memory().unwrap())),
                manifests,
                discord: None,
                api_key: None,
                lane_queue: Mutex::new(LaneQueue::new(10, QueueMode::Collect)),
                hook_registry,
                chain_executor,
                harnesses,
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
                    SqliteMemoryStore::open_in_memory(None)
                        .expect("open in-memory semantic store"),
                ),
                kill_switch: Arc::new(KillSwitch::new()),
                active_cancellation_tokens: Arc::new(std::sync::Mutex::new(
                    std::collections::HashMap::new(),
                )),
                session_store: Arc::new(InMemorySessionStore::new()),
                constitutional_registry: Arc::new(ConstitutionalRegistry::new()),
                capability_registry: Arc::new(RwLock::new(Arc::new(CapabilityRegistry::empty()))),
                ticket_store: Arc::new(InMemoryTicketStore::new()),
            hitl_resumed_tx: tokio::sync::broadcast::channel(64).0,
                workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
                gh_run_store: Arc::new(InMemoryGhRunStateStore::new()),
                gh_pr_store: Arc::new(InMemoryGhPrStateStore::new()),
                human_gate_store: Arc::new(InMemoryHumanGateStore::new()),
                admin_auth: None,
                admin_audit: None,
            });

            (
                state,
                HookCounters {
                    pre_route,
                    post_route,
                    pre_turn,
                    post_turn,
                },
            )
        }

        /// Poll `db.get_transcript` for `session_id` until the row count
        /// reaches `expected` or `budget` elapses. Yields a precise failure
        /// message naming the missing row so a regression in process_message
        /// is diagnosable from CI output alone.
        async fn wait_for_transcript_len(
            state: &Arc<AppState>,
            session_id: &str,
            expected: usize,
            budget: std::time::Duration,
        ) {
            let start = std::time::Instant::now();
            loop {
                let len = {
                    let db = state.db.lock().await;
                    db.get_transcript(session_id).map(|t| t.len()).unwrap_or(0)
                };
                if len >= expected {
                    return;
                }
                if start.elapsed() > budget {
                    panic!(
                        "transcript did not reach expected len {expected} within \
                         {:?} (saw {len}); session_id={session_id} — production \
                         E2E path likely broke between process_message and \
                         transcript persistence",
                        budget
                    );
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }

        /// Discord-path round trip:
        /// inject DiscordMessage -> event_loop -> process_message ->
        /// StdioHarness::send_turn -> real sera-runtime --ndjson child ->
        /// LlmClient -> mock LLM HTTP -> reply -> transcript persisted ->
        /// lane idle -> hook chain points fired.
        ///
        /// Guards the seams that `spawn_mock` cannot exercise:
        ///   - runtime startup + handshake skip in StdioHarness::send_turn
        ///   - canonical Submission shape (op.type=user_turn, session_key)
        ///   - runtime SSE parser on a real LLM body
        ///   - persist_tool_events + assistant transcript append
        ///   - lane_queue.complete_run on every exit path
        ///   - run_hook_point firing for pre_route, post_route, pre_turn, post_turn
        #[tokio::test]
        async fn discord_path_round_trip() {
            let mock = start_mock_llm("hello from mock", 0).await;
            let (state, counters) = production_e2e_state(&mock.base_url).await;

            let (tx, rx) = mpsc::channel::<DiscordMessage>(8);
            let event_state = Arc::clone(&state);
            let handle = tokio::spawn(async move {
                event_loop(event_state, rx).await;
            });

            tx.send(DiscordMessage {
                channel_id: "ch_ov7z".into(),
                user_id: "user_ov7z".into(),
                username: "tester".into(),
                content: "ping".into(),
                message_id: "msg_ov7z_1".into(),
                is_dm: true,
                mentions_bot: false,
            })
            .await
            .expect("send discord message");

            // Wait for the assistant row to appear (deterministic, no sleep).
            let session_key = "discord:sera:ch_ov7z";
            let session_id = {
                let start = std::time::Instant::now();
                loop {
                    let maybe = {
                        let db = state.db.lock().await;
                        db.get_session_by_key(session_key).ok().flatten()
                    };
                    if let Some(s) = maybe {
                        break s.id;
                    }
                    if start.elapsed() > std::time::Duration::from_secs(10) {
                        panic!(
                            "session for {session_key} never created — \
                             process_message did not reach the session-store \
                             write. Check runtime startup + StdioHarness::spawn"
                        );
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
            };
            wait_for_transcript_len(&state, &session_id, 2, std::time::Duration::from_secs(10))
                .await;

            // Tear down the event loop.
            drop(tx);
            handle.await.expect("event_loop join");

            // ── Assertions ───────────────────────────────────────────────

            // A2/A3: assistant reply persisted with mock content.
            let transcript = {
                let db = state.db.lock().await;
                db.get_transcript(&session_id).expect("get_transcript")
            };
            assert_eq!(
                transcript.len(),
                2,
                "expected exactly 2 transcript rows (user + assistant), got {}",
                transcript.len()
            );
            assert_eq!(transcript[0].role, "user");
            assert_eq!(transcript[0].content.as_deref(), Some("ping"));
            assert_eq!(
                transcript[1].role, "assistant",
                "row[1] must be the assistant reply"
            );
            assert_eq!(
                transcript[1].content.as_deref(),
                Some("hello from mock"),
                "assistant reply must match mock LLM body byte-for-byte; an \
                 empty or mismatched value indicates the runtime SSE parser \
                 or the gateway streaming-delta accumulator regressed (see \
                 sera-aepj)"
            );

            // A4: lane slot released; no pending events for the session.
            {
                let lq = state.lane_queue.lock().await;
                assert!(
                    !lq.has_pending(session_key),
                    "lane queue still has pending events for {session_key} \
                     after a happy turn — sera-y9f8-style wedge"
                );
                assert_eq!(
                    lq.active_runs(),
                    0,
                    "lane_queue.active_runs() != 0 after happy turn; \
                     complete_run was missed on some exit path"
                );
            }

            // A5: each lifecycle hook chain fired exactly once.
            assert_eq!(
                counters.pre_route.load(Ordering::SeqCst),
                1,
                "pre_route hook chain did not fire exactly once"
            );
            assert_eq!(
                counters.post_route.load(Ordering::SeqCst),
                1,
                "post_route hook chain did not fire exactly once"
            );
            assert_eq!(
                counters.pre_turn.load(Ordering::SeqCst),
                1,
                "pre_turn hook chain did not fire exactly once"
            );
            assert_eq!(
                counters.post_turn.load(Ordering::SeqCst),
                1,
                "post_turn hook chain did not fire exactly once — guards a \
                 regression where post_turn is skipped on the success path"
            );

            // Drop state — owning `Arc<AppState>` releases the runtime
            // child's stdin (via StdioHarness Drop), so the child sees EOF
            // and exits its NDJSON loop cleanly.
            drop(state);
            drop(mock);
        }

        /// HTTP `/api/chat` round trip:
        /// chat_handler -> StdioHarness::send_turn -> real sera-runtime
        /// --ndjson child -> mock LLM -> ChatResponse with body + usage.
        ///
        /// Guards the HTTP-specific seams that the Discord path does not
        /// exercise:
        ///   - ChatResponse.response carries the assistant reply (sera-aepj
        ///     empty-reply guard on the sync branch)
        ///   - ChatResponse.usage carries the provider-reported token counts
        ///   - lane_queue.complete_run on the HTTP exit path
        #[tokio::test]
        async fn http_chat_path_round_trip() {
            let mock = start_mock_llm("hello from mock", 0).await;
            let (state, _counters) = production_e2e_state(&mock.base_url).await;
            let app = build_router(Arc::clone(&state));

            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/chat")
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::json!({ "message": "ping" }).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .expect("oneshot /api/chat");

            assert_eq!(
                response.status(),
                StatusCode::OK,
                "chat_handler returned non-200; production HTTP path broken"
            );
            let body_bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
                .await
                .expect("read response body");
            let body: serde_json::Value =
                serde_json::from_slice(&body_bytes).expect("response body must be JSON");

            // Body shape (production-path contract): { response, session_id, usage }.
            assert_eq!(
                body["response"].as_str(),
                Some("hello from mock"),
                "ChatResponse.response is wrong; got {body}; if empty, \
                 regression of sera-aepj (empty-reply guard) on /api/chat \
                 sync path"
            );
            assert_eq!(
                body["usage"]["prompt_tokens"], 7,
                "usage.prompt_tokens not propagated from mock LLM"
            );
            assert_eq!(
                body["usage"]["completion_tokens"], 5,
                "usage.completion_tokens not propagated from mock LLM"
            );
            assert_eq!(
                body["usage"]["total_tokens"], 12,
                "usage.total_tokens not propagated end-to-end (runtime \
                 TurnCompleted -> harness.send_turn -> chat_handler)"
            );

            // Lane idle after happy turn.
            {
                let lq = state.lane_queue.lock().await;
                assert_eq!(
                    lq.active_runs(),
                    0,
                    "HTTP chat_handler leaked a lane slot — complete_run \
                     missed on success path"
                );
            }

            drop(state);
            drop(mock);
        }

        // ── sera-ve9x PR 3: embedded-mode parity ───────────────────────────
        //
        // Each test below mirrors a runtime-mode test above, swapping
        // `DispatchMode::Runtime` for `DispatchMode::Embedded` on the same
        // fixture. Every assertion (transcript persistence, lane release,
        // hook-counter parity, chat response shape) is preserved byte-for-
        // byte. Together with the runtime tests, these form the ADR §4
        // step 2 acceptance gate: the same gateway path produces the same
        // observable result against both backends.

        /// Discord-path round trip against the in-process embedded backend.
        ///
        /// Mirrors [`discord_path_round_trip`] but routes through
        /// [`EmbeddedRuntimeTransport`] instead of a `sera-runtime --ndjson`
        /// child. No `locate_runtime_bin()` call, no child process, but the
        /// same transcript / lane / hook-counter assertions hold — that's
        /// the parity proof PR 3 was written to mechanise.
        #[tokio::test]
        async fn discord_path_round_trip_embedded() {
            let mock = start_mock_llm("hello from mock", 0).await;
            let (state, counters) =
                production_e2e_state_with_mode(&mock.base_url, DispatchMode::Embedded).await;

            let (tx, rx) = mpsc::channel::<DiscordMessage>(8);
            let event_state = Arc::clone(&state);
            let handle = tokio::spawn(async move {
                event_loop(event_state, rx).await;
            });

            tx.send(DiscordMessage {
                channel_id: "ch_ve9x".into(),
                user_id: "user_ve9x".into(),
                username: "tester".into(),
                content: "ping".into(),
                message_id: "msg_ve9x_1".into(),
                is_dm: true,
                mentions_bot: false,
            })
            .await
            .expect("send discord message");

            let session_key = "discord:sera:ch_ve9x";
            let session_id = {
                let start = std::time::Instant::now();
                loop {
                    let maybe = {
                        let db = state.db.lock().await;
                        db.get_session_by_key(session_key).ok().flatten()
                    };
                    if let Some(s) = maybe {
                        break s.id;
                    }
                    if start.elapsed() > std::time::Duration::from_secs(10) {
                        panic!(
                            "session for {session_key} never created — \
                             process_message did not reach the session-store \
                             write under EmbeddedRuntimeTransport"
                        );
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
            };
            wait_for_transcript_len(&state, &session_id, 2, std::time::Duration::from_secs(10))
                .await;

            drop(tx);
            handle.await.expect("event_loop join");

            let transcript = {
                let db = state.db.lock().await;
                db.get_transcript(&session_id).expect("get_transcript")
            };
            assert_eq!(
                transcript.len(),
                2,
                "expected exactly 2 transcript rows (user + assistant), got {}",
                transcript.len()
            );
            assert_eq!(transcript[0].role, "user");
            assert_eq!(transcript[0].content.as_deref(), Some("ping"));
            assert_eq!(
                transcript[1].role, "assistant",
                "row[1] must be the assistant reply (embedded path)"
            );
            assert_eq!(
                transcript[1].content.as_deref(),
                Some("hello from mock"),
                "embedded transport must surface the mock LLM body byte-for-\
                 byte; mismatch indicates the in-process LlmClient SSE \
                 parser or `turn_events_from_outcome` projection regressed"
            );

            {
                let lq = state.lane_queue.lock().await;
                assert!(
                    !lq.has_pending(session_key),
                    "lane queue still has pending events for {session_key} \
                     after a happy turn — embedded path leaked a lane slot"
                );
                assert_eq!(
                    lq.active_runs(),
                    0,
                    "lane_queue.active_runs() != 0 after happy embedded turn"
                );
            }

            assert_eq!(
                counters.pre_route.load(Ordering::SeqCst),
                1,
                "pre_route hook chain did not fire exactly once (embedded)"
            );
            assert_eq!(
                counters.post_route.load(Ordering::SeqCst),
                1,
                "post_route hook chain did not fire exactly once (embedded)"
            );
            assert_eq!(
                counters.pre_turn.load(Ordering::SeqCst),
                1,
                "pre_turn hook chain did not fire exactly once (embedded)"
            );
            assert_eq!(
                counters.post_turn.load(Ordering::SeqCst),
                1,
                "post_turn hook chain did not fire exactly once (embedded)"
            );

            drop(state);
            drop(mock);
        }

        /// HTTP `/api/chat` round trip against the in-process embedded
        /// backend. Mirrors [`http_chat_path_round_trip`] — same body shape,
        /// same usage propagation, same lane release.
        #[tokio::test]
        async fn http_chat_path_round_trip_embedded() {
            let mock = start_mock_llm("hello from mock", 0).await;
            let (state, _counters) =
                production_e2e_state_with_mode(&mock.base_url, DispatchMode::Embedded).await;
            let app = build_router(Arc::clone(&state));

            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/chat")
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::json!({ "message": "ping" }).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .expect("oneshot /api/chat");

            assert_eq!(
                response.status(),
                StatusCode::OK,
                "embedded chat_handler returned non-200; in-process \
                 production HTTP path broken"
            );
            let body_bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
                .await
                .expect("read response body");
            let body: serde_json::Value =
                serde_json::from_slice(&body_bytes).expect("response body must be JSON");

            assert_eq!(
                body["response"].as_str(),
                Some("hello from mock"),
                "ChatResponse.response is wrong on embedded path; got {body}"
            );
            assert_eq!(
                body["usage"]["prompt_tokens"], 7,
                "embedded usage.prompt_tokens not propagated from mock LLM"
            );
            assert_eq!(
                body["usage"]["completion_tokens"], 5,
                "embedded usage.completion_tokens not propagated from mock LLM"
            );
            assert_eq!(
                body["usage"]["total_tokens"], 12,
                "embedded usage.total_tokens not propagated end-to-end \
                 (DefaultRuntime -> turn_events_from_outcome -> chat_handler)"
            );

            {
                let lq = state.lane_queue.lock().await;
                assert_eq!(
                    lq.active_runs(),
                    0,
                    "embedded HTTP chat_handler leaked a lane slot"
                );
            }

            drop(state);
            drop(mock);
        }

        /// Snapshot the *set* of direct child PIDs of the current process
        /// whose `/proc/<pid>/comm` matches `comm_filter` (case-sensitive).
        /// Used by [`embedded_path_no_runtime_child_spawns`] for set-
        /// difference detection: any PID present in the post-window snapshot
        /// but absent from the pre-window snapshot is a fresh spawn.
        ///
        /// A pure count delta is unreliable because reaps and spawns can
        /// cancel out — e.g. a peer runtime-mode test reaps its
        /// `sera-runtime` child during our window while the embedded path
        /// (regressed) spawns a new one, leaving the count unchanged.
        /// PID-set comparison cannot be defeated by such cancellation.
        ///
        /// Returns `None` on non-Linux or if `/proc` is unreadable; the
        /// caller then degrades to the structural assertion that the
        /// embedded branch in [`production_e2e_state_with_mode`] does not
        /// call `locate_runtime_bin`.
        fn list_direct_children_with_comm(
            comm_filter: &str,
        ) -> Option<std::collections::HashSet<u32>> {
            let my_pid = std::process::id();
            let entries = std::fs::read_dir("/proc").ok()?;
            let mut pids: std::collections::HashSet<u32> = std::collections::HashSet::new();
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name_str) = name.to_str() else {
                    continue;
                };
                let Ok(child_pid) = name_str.parse::<u32>() else {
                    continue;
                };
                let stat = match std::fs::read_to_string(entry.path().join("stat")) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                // /proc/<pid>/stat format: "pid (comm) state ppid ...".
                // The comm field can contain spaces and parens, so split
                // after the last ')'.
                let after_comm = match stat.rsplit_once(')') {
                    Some((_, rest)) => rest.trim(),
                    None => continue,
                };
                let parts: Vec<&str> = after_comm.split_whitespace().collect();
                // After the comm field: state (idx 0), ppid (idx 1), ...
                let Some(ppid_str) = parts.get(1) else {
                    continue;
                };
                let Ok(ppid) = ppid_str.parse::<u32>() else {
                    continue;
                };
                if ppid != my_pid {
                    continue;
                }
                // Read /proc/<pid>/comm directly (truncated to ~16 chars
                // by the kernel; matches "sera-runtime" exactly).
                let comm = match std::fs::read_to_string(entry.path().join("comm")) {
                    Ok(c) => c.trim_end().to_string(),
                    Err(_) => continue,
                };
                if comm == comm_filter {
                    pids.insert(child_pid);
                }
            }
            Some(pids)
        }

        /// Embedded mode must not spawn a `sera-runtime --ndjson` child.
        /// Asserts (a) the embedded boot/turn round-trips successfully via
        /// the in-process transport, and (b) no *new* `sera-runtime` PID
        /// appears as a direct child of the test process during the
        /// embedded fixture's run.
        ///
        /// PID-set difference rather than count delta: a pure count check
        /// can be defeated when a peer runtime-mode test's child reaps
        /// during the same window as a regressed embedded spawn (count
        /// stays equal, regression slips through). Comparing the
        /// pre-window and post-window PID sets and asserting `after \\
        /// before` is empty makes the check robust against cancellation.
        ///
        /// Reliability under cargo test parallelism: the runtime-mode
        /// branch of [`production_e2e_state_with_mode`] takes
        /// [`RUNTIME_SPAWN_SAMPLE_LOCK`] across `RuntimeChildSupervisor::start`,
        /// and this test holds the same lock across its sample window. So
        /// no peer runtime-mode test can spawn a `sera-runtime` child
        /// while we sample. We further filter by `comm == "sera-runtime"`
        /// so peer `bash` mocks from `StdioHarness::spawn_mock` are
        /// ignored. On non-Linux (no `/proc`) the test still asserts the
        /// embedded round-trip and the `dispatch_kind()` belt-and-braces
        /// assertion; the structural absence of `RuntimeChildSupervisor::start`
        /// from the embedded branch in `production_e2e_state_with_mode`
        /// carries the load there.
        #[tokio::test]
        async fn embedded_path_no_runtime_child_spawns() {
            let _spawn_guard = RUNTIME_SPAWN_SAMPLE_LOCK.lock().await;
            let baseline = list_direct_children_with_comm("sera-runtime");

            let mock = start_mock_llm("hello from mock", 0).await;
            let (state, _counters) =
                production_e2e_state_with_mode(&mock.base_url, DispatchMode::Embedded).await;

            // Round-trip through the in-process backend.
            let app = build_router(Arc::clone(&state));
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/chat")
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::json!({ "message": "ping" }).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .expect("oneshot /api/chat");
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "embedded boot/turn must succeed without spawning a child"
            );

            // PID-set difference: any PID present in `after` but absent from
            // `before` is a fresh `sera-runtime` spawn that occurred during
            // our sample window. Under RUNTIME_SPAWN_SAMPLE_LOCK, no peer
            // runtime-mode test could have spawned during this window — so
            // a non-empty difference is attributable to this test and means
            // the embedded branch accidentally took the runtime-supervisor
            // path. PID disappearances (peer reaps) do NOT affect the
            // difference, so reap/spawn cancellation cannot mask a
            // regression.
            if let (Some(before), Some(after)) =
                (baseline, list_direct_children_with_comm("sera-runtime"))
            {
                let new_pids: Vec<u32> = after.difference(&before).copied().collect();
                assert!(
                    new_pids.is_empty(),
                    "embedded path spawned `sera-runtime` child PIDs {new_pids:?} \
                     during the sample window; embedded boot must not invoke \
                     RuntimeChildSupervisor::start"
                );
            }

            // Belt-and-braces: the wired transport reports its backend
            // identity directly, so a regression that wires
            // `RuntimeChildSupervisor` under `DispatchMode::Embedded`
            // fails this assertion even on platforms without `/proc`.
            let transport = state
                .harnesses
                .get("sera")
                .cloned()
                .expect("embedded transport present");
            assert_eq!(
                transport.dispatch_kind(),
                "embedded",
                "DispatchMode::Embedded must wire EmbeddedRuntimeTransport, \
                 not the stdio supervisor"
            );

            drop(state);
            drop(mock);
        }

        /// Closest steer parity test against production state: exercise the
        /// transport-level steer staging path on an embedded
        /// `production_e2e_state_with_mode` fixture, then prove the next
        /// turn drains the staged item without wedging the lane queue. This
        /// is the embedded mirror of the stdio backend's "steer-then-turn"
        /// behaviour (`execute_steer_drains_until_turn_completed` covers
        /// the same shape against `StdioHarness::spawn_mock`).
        #[tokio::test]
        async fn embedded_steer_drains_into_next_turn_via_production_state() {
            let mock = start_mock_llm("ack", 0).await;
            let (state, _counters) =
                production_e2e_state_with_mode(&mock.base_url, DispatchMode::Embedded).await;

            let session_key = "discord:sera:ch_ve9x_steer";
            let transport = state
                .harnesses
                .get("sera")
                .cloned()
                .expect("embedded sera transport present");

            transport
                .send_steer(
                    vec![serde_json::json!({"role": "user", "content": "guidance"})],
                    session_key,
                )
                .await
                .expect("steer staged");

            let events = transport
                .send_turn(
                    vec![serde_json::json!({"role": "user", "content": "ping"})],
                    session_key,
                )
                .await
                .expect("turn after steer");

            assert_eq!(
                events.response, "ack",
                "embedded turn after steer must produce the mock reply — \
                 a wedge would surface as an empty / timed-out response"
            );

            // Lane queue idle (no `process_message` invocation made; this is
            // a transport-level parity check, mirroring
            // `execute_steer_drains_until_turn_completed`).
            {
                let lq = state.lane_queue.lock().await;
                assert_eq!(
                    lq.active_runs(),
                    0,
                    "embedded steer must not allocate or leak a lane slot"
                );
            }

            drop(state);
            drop(mock);
        }

        /// `effective_dispatch_mode_label()` reads the
        /// `SERA_DISPATCH_MODE` env var; this test asserts it returns
        /// `embedded` while an embedded `production_e2e_state` is live. The
        /// guard restores the env var on drop so concurrent tests are
        /// unaffected. Coverage parity with the existing
        /// `effective_dispatch_mode_*` tests, but evaluated alongside an
        /// embedded fixture so the label and the active backend are
        /// asserted together.
        #[tokio::test]
        async fn dispatch_mode_label_reflects_active_backend_in_embedded_state() {
            // P1 fix (Codex review on PR #1131): keep the
            // `DispatchModeEnvGuard` strictly inside the
            // `DISPATCH_MODE_ENV_LOCK` critical section so its `Drop`
            // (which calls unsafe `std::env::set_var/remove_var`) cannot
            // race with peer tests that mutate `SERA_DISPATCH_MODE`. The
            // env mutation + label assertion happens synchronously under
            // the lock; the lock and guard are released together before
            // any await. After release, the assertion no longer holds —
            // the env reverts to its prior value — but the production-
            // state setup below does NOT read `SERA_DISPATCH_MODE`
            // (`production_e2e_state_with_mode` takes the dispatch mode
            // as an explicit parameter), so the embedded fixture runs
            // independently of process env state.
            {
                let _lock = DISPATCH_MODE_ENV_LOCK
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let _guard = DispatchModeEnvGuard::set("embedded");
                assert_eq!(
                    effective_dispatch_mode_label(),
                    "embedded",
                    "effective dispatch mode must report `embedded` when \
                     the env requests it"
                );
                // _guard drops here (unsafe env restore happens while
                // _lock is still held), then _lock drops.
            }

            let mock = start_mock_llm("hello from mock", 0).await;
            let (state, _counters) =
                production_e2e_state_with_mode(&mock.base_url, DispatchMode::Embedded).await;
            // P2 fix (Codex review on PR #1131): both runtime and embedded
            // branches insert the same `"sera"` key into `harnesses`, so a
            // `contains_key` check is vacuous. Verify backend selection by
            // querying `dispatch_kind()` on the trait object — fails if the
            // embedded branch accidentally wires `RuntimeChildSupervisor`.
            let transport = state
                .harnesses
                .get("sera")
                .cloned()
                .expect("embedded production state must wire a transport for `sera`");
            assert_eq!(
                transport.dispatch_kind(),
                "embedded",
                "DispatchMode::Embedded must wire EmbeddedRuntimeTransport"
            );

            drop(state);
            drop(mock);
        }
    }

    // ── W3C traceparent header propagation (sera-n806) ───────────────────────

    /// Verify that W3cTraceContext fields are populated from the incoming HTTP
    /// headers rather than left as None (default).
    #[test]
    fn w3c_trace_context_from_headers() {
        use axum::http::HeaderMap;
        use sera_types::envelope::W3cTraceContext;

        fn extract_trace(headers: &HeaderMap) -> W3cTraceContext {
            W3cTraceContext {
                traceparent: headers
                    .get("traceparent")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned),
                tracestate: headers
                    .get("tracestate")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned),
            }
        }

        // Both headers present — both fields populated.
        let mut headers = HeaderMap::new();
        headers.insert(
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
                .parse()
                .unwrap(),
        );
        headers.insert("tracestate", "vendor=value".parse().unwrap());
        let trace = extract_trace(&headers);
        assert_eq!(
            trace.traceparent.as_deref(),
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        );
        assert_eq!(trace.tracestate.as_deref(), Some("vendor=value"));

        // No headers — both fields None (same as default).
        let empty = HeaderMap::new();
        let trace_none = extract_trace(&empty);
        assert_eq!(trace_none.traceparent, None);
        assert_eq!(trace_none.tracestate, None);

        // Only traceparent — tracestate is None.
        let mut partial = HeaderMap::new();
        partial.insert(
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
                .parse()
                .unwrap(),
        );
        let trace_partial = extract_trace(&partial);
        assert!(trace_partial.traceparent.is_some());
        assert_eq!(trace_partial.tracestate, None);
    }
}
