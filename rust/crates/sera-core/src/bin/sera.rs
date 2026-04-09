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

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};
use tracing_subscriber::EnvFilter;

use sera_config::manifest_loader::{
    load_manifest_file, parse_manifests, resolve_connector_token, resolve_provider_api_key,
    ManifestSet,
};
use sera_db::sqlite::SqliteDb;
use sera_domain::config_manifest::{AgentSpec, ConnectorSpec, ProviderSpec};

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
}

#[derive(Subcommand)]
enum AgentCommands {
    /// Create a new agent
    Create { name: String },
    /// List agents
    List,
}

// ── Shared state ────────────────────────────────────────────────────────────

struct AppState {
    db: Mutex<SqliteDb>,
    manifests: ManifestSet,
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
}

#[derive(Serialize)]
struct ChatResponse {
    reply: String,
    agent: String,
    session_id: String,
}

// ── HTTP handlers ───────────────────────────────────────────────────────────

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    // Determine which agent to use.
    let agent_name = req
        .agent
        .as_deref()
        .or_else(|| state.manifests.agent_names().into_iter().next())
        .unwrap_or("sera")
        .to_owned();

    let agent_spec: AgentSpec = match state
        .manifests
        .agent_spec(&agent_name)
    {
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

    // Get recent transcript for context.
    let transcript = db.get_transcript_recent(&session.id, 20).unwrap_or_default();
    let session_id = session.id.clone();
    drop(db); // Release lock before making HTTP call.

    // Execute a simple turn: call the LLM via the configured provider.
    let reply = execute_turn(&state.manifests, &agent_spec, &transcript, &req.message).await;

    // Save assistant response.
    let db = state.db.lock().await;
    db.append_transcript(&session_id, "assistant", Some(&reply), None, None)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ChatResponse {
        reply,
        agent: agent_name,
        session_id,
    }))
}

// ── Turn execution (inlined from sera-runtime reasoning loop) ───────────────

async fn execute_turn(
    manifests: &ManifestSet,
    agent_spec: &AgentSpec,
    transcript: &[sera_db::sqlite::TranscriptRow],
    user_message: &str,
) -> String {
    let mut messages: Vec<serde_json::Value> = Vec::new();

    // Add system message from persona if configured.
    if let Some(persona) = &agent_spec.persona {
        if let Some(anchor) = &persona.immutable_anchor {
            messages.push(serde_json::json!({
                "role": "system",
                "content": anchor,
            }));
        }
    }

    // Add transcript history.
    for row in transcript {
        if let Some(content) = &row.content {
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
    let provider_spec: Option<ProviderSpec> = manifests
        .provider_spec(&agent_spec.provider)
        .ok()
        .flatten();

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
            return format!(
                "[sera] Provider '{}' not found in config.",
                agent_spec.provider
            );
        }
    };

    // Call the LLM.
    let client = reqwest::Client::new();
    let mut request_body = serde_json::json!({
        "model": model,
        "messages": messages,
    });

    // Add API key header only if non-empty.
    let mut req_builder = client
        .post(format!("{}/chat/completions", base_url))
        .header("Content-Type", "application/json");
    if !api_key.is_empty() {
        req_builder = req_builder.header("Authorization", format!("Bearer {api_key}"));
    }

    // Limit max_tokens for safety.
    request_body["max_tokens"] = serde_json::json!(4096);

    let response = match req_builder.json(&request_body).send().await {
        Ok(resp) => resp,
        Err(e) => {
            return format!("[sera] LLM request failed: {e}");
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return format!("[sera] LLM error {status}: {body}");
    }

    let body: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            return format!("[sera] Failed to parse LLM response: {e}");
        }
    };

    // Extract the assistant's reply.
    body.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("[sera] No response from LLM")
        .to_owned()
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

        // Use channel_id as the session key for Discord conversations.
        let session_key = format!("discord:{}", msg.channel_id);
        let (session, transcript) = {
            let db = state.db.lock().await;
            let session = match db.get_session_by_key(&session_key) {
                Ok(Some(s)) => s,
                Ok(None) => {
                    let id = format!("ses_discord_{}", msg.channel_id);
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

        let reply = execute_turn(&state.manifests, &agent_spec, &transcript, &msg.content).await;

        {
            let db = state.db.lock().await;
            let _ = db.append_transcript(&session.id, "assistant", Some(&reply), None, None);
        }

        // Send the reply back to Discord.
        let connector_spec = state
            .manifests
            .connectors
            .first()
            .and_then(|c| serde_json::from_value::<ConnectorSpec>(c.spec.clone()).ok());

        if let Some(spec) = connector_spec {
            if let Some(token) = resolve_connector_token(&spec) {
                let dc = DiscordConnector::new(&token, &agent_name, state_noop_sender());
                if let Err(e) = dc.send_message(&msg.channel_id, &reply).await {
                    tracing::error!("Failed to send Discord reply: {e}");
                }
            }
        }
    }
}

/// Create a no-op sender for DiscordConnector::send_message (we only need
/// the REST client, not the gateway receiver).
fn state_noop_sender() -> mpsc::Sender<DiscordMessage> {
    let (tx, _rx) = mpsc::channel(1);
    tx
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

fn run_agent_list(config: &PathBuf) -> anyhow::Result<()> {
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

        Commands::Start { config, port } => run_start(config, port).await,
    }
}

async fn run_start(config: PathBuf, port: u16) -> anyhow::Result<()> {
    // 1. Load config.
    tracing::info!(config = %config.display(), "Loading SERA configuration");
    let manifests = load_manifest_file(&config)?;

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

    let state = Arc::new(AppState {
        db: Mutex::new(db),
        manifests,
    });

    // 3. Start Discord connector if configured.
    let (discord_tx, discord_rx) = mpsc::channel::<DiscordMessage>(256);

    for cm in &state.manifests.connectors {
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

        let token = match resolve_connector_token(&spec) {
            Some(t) => t,
            None => {
                tracing::warn!(
                    name = %cm.metadata.name,
                    "Discord token not resolved (set SERA_SECRET_* env var). Skipping connector."
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

        let connector = DiscordConnector::new(&token, &agent_name, discord_tx.clone());
        tokio::spawn(async move {
            if let Err(e) = connector.run().await {
                tracing::error!("Discord connector exited with error: {e}");
            }
        });
    }

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
        Arc::new(AppState {
            db: Mutex::new(SqliteDb::open_in_memory().unwrap()),
            manifests: test_manifests(),
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
        assert_eq!(json["agent"], "sera");
        assert!(json["session_id"].as_str().is_some());
        // Reply will contain an error message since LLM is not reachable,
        // but the structure is correct.
        assert!(json["reply"].as_str().is_some());
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
}
