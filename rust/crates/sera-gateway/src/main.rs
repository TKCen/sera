//! SERA Core — the central API server and orchestration engine.
//!
//! This binary replaces the TypeScript sera-core Express server.
//! Built on axum + tokio + sqlx.

pub mod constitutional_config;
pub mod discord;
mod error;
mod middleware;
mod routes;
mod state;
mod services;

use std::sync::Arc;
use tokio::sync::RwLock;

use axum::{
    middleware::from_fn,
    routing::{delete, get, patch, post, put},
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use std::time::Duration;

use sera_auth::JwtService;
use sera_config::core_config::CoreConfig;
use sera_config::providers::ProvidersConfig;
use sera_db::DbPool;
use sera_tools::sandbox::docker::DockerSandboxProvider;

use crate::services::schedule_service::ScheduleService;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Load configuration
    let mut config = CoreConfig::from_env().map_err(|e| anyhow::anyhow!("{e}"))?;

    // Optionally load providers.json
    let providers_path = std::env::var("SERA_PROVIDERS_JSON").ok();
    if let Some(path) = &providers_path
        && let Err(e) = config.load_providers(path)
    {
        tracing::warn!("Failed to load providers.json: {e}");
    }

    let port = config.port;

    // Connect to database
    let db = DbPool::connect(&config.database_url).await?;
    tracing::info!("Connected to database");

    // Initialize sandbox provider (Docker)
    let sandbox: Arc<dyn sera_tools::sandbox::SandboxProvider> =
        Arc::new(DockerSandboxProvider::new().map_err(|e| {
            tracing::warn!("Docker not available: {e}");
            anyhow::anyhow!("Docker connection failed: {e}")
        })?);
    tracing::info!("Connected to Docker daemon");

    // Build shared state
    let providers = Arc::new(RwLock::new(
        config.providers.clone().unwrap_or(ProvidersConfig { providers: vec![] }),
    ));
    let jwt_service = Arc::new(JwtService::new(config.centrifugo.token_secret.clone()));
    let api_key = Arc::new(config.api_key.clone());
    let config = Arc::new(config);

    // Initialize Centrifugo client
    let centrifugo = {
        let c = &config.centrifugo;
        Some(Arc::new(sera_events::CentrifugoClient::new(
            c.api_url.clone(),
            c.api_key.clone(),
            c.token_secret.clone(),
        )))
    };

    let schedule_svc = Arc::new(ScheduleService::new(db.clone()));

    // Initialize transcript persistence
    let session_persist = Arc::new(sera_gateway::session_persist::SqlxSessionPersist::from_db_pool(&db));
    let transcript_persistence = Arc::new(sera_gateway::transcript_persist::TranscriptPersistence::new(session_persist));

    // Self-evolution pipeline + hook chain executor — feed one another through
    // route handlers so evolve transitions fire `OnChangeArtifactProposed`
    // hooks with `HookContext.change_artifact` populated.
    let hook_registry = Arc::new(sera_hooks::HookRegistry::new());
    let chain_executor = Arc::new(sera_hooks::ChainExecutor::new(Arc::clone(&hook_registry)));
    let evolution_pipeline = Arc::new(
        sera_meta::artifact_pipeline::ArtifactPipeline::with_defaults(),
    );
    // Constitutional-rule registry consulted at `PreApproval` inside the
    // evolve `/evaluate` route. Seeded from the rules file at startup; if the
    // file is absent the registry stays empty (backward-compat). A parse error
    // is fatal — a misconfigured rule file is a security concern.
    let constitutional_registry = Arc::new(
        sera_meta::constitutional::ConstitutionalRegistry::new(),
    );
    if let Err(e) = constitutional_config::seed_registry_from_env(&constitutional_registry).await {
        tracing::error!("Failed to load constitutional rules: {e}");
        std::process::exit(1);
    }

    // Signer for /api/evolve/* CapabilityTokens. Prefer the dedicated
    // SERA_EVOLVE_TOKEN_SECRET so operators can rotate it independently of
    // the JWT/API secrets; fall back to SERA_JWT_SECRET and finally to the
    // centrifugo token secret (already present in CoreConfig). An empty
    // secret causes verify() to fail with EmptySecret — surfaced as 401 by
    // the route layer — which is the intended behaviour when no secret is
    // configured.
    let evolve_token_secret = std::env::var("SERA_EVOLVE_TOKEN_SECRET")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("SERA_JWT_SECRET").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| config.centrifugo.token_secret.clone());
    let evolve_token_signer = Arc::new(
        sera_gateway::evolve_token::EvolveTokenSigner::new(evolve_token_secret.into_bytes()),
    );
    let proposal_usage = sera_gateway::evolve_token::ProposalUsageTracker::new_arc();

    let app_state = AppState {
        db,
        config: config.clone(),
        jwt: jwt_service.clone(),
        providers,
        sandbox,
        providers_path,
        centrifugo,
        mcp_registry: Arc::new(RwLock::new(routes::mcp::McpRegistry::new())),
        schedule_svc: schedule_svc.clone(),
        harness_registry: sera_gateway::harness_dispatch::new_harness_registry(),
        plugin_registry: sera_gateway::harness_dispatch::new_plugin_registry(),
        queue_backend: Arc::new(sera_queue::LocalQueueBackend::new()),
        generation_marker: sera_gateway::generation::current_generation(),
        kill_switch: Arc::new(sera_gateway::kill_switch::KillSwitch::new()),
        transcript_persistence: transcript_persistence.clone(),
        lane_queue: std::sync::Arc::new(tokio::sync::Mutex::new(
            sera_db::lane_queue::LaneQueue::new(10, sera_db::lane_queue::QueueMode::Collect),
        )),
        evolution_pipeline,
        constitutional_registry,
        hook_registry,
        chain_executor,
        evolve_token_signer,
        proposal_usage,
    };

    // Extract queue backend before app_state is moved into the router.
    let queue_backend = app_state.queue_backend.clone();

    // Build router
    let app = build_router(app_state, jwt_service, api_key);

    // Spawn schedule tick loop — fires every 60s to process due cron schedules
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            match schedule_svc.process_due_schedules().await {
                Ok(n) if n > 0 => tracing::info!("Schedule tick: triggered {} schedule(s)", n),
                Ok(_) => {}
                Err(e) => tracing::error!("Schedule tick error: {}", e),
            }
        }
    });

    // Spawn Discord connector if DISCORD_TOKEN is set.
    // Two tasks: one runs the WebSocket connector, one forwards DiscordMessages
    // into the queue backend as per-channel lanes ("discord:{channel_id}").
    if let Ok(discord_token) = std::env::var("DISCORD_TOKEN") {
        let agent_name = std::env::var("DISCORD_AGENT_NAME")
            .unwrap_or_else(|_| "sera".to_string());
        let (discord_tx, mut discord_rx) =
            tokio::sync::mpsc::channel::<discord::DiscordMessage>(128);
        let queue = queue_backend;

        tracing::info!("Discord connector spawned (agent_name={agent_name})");

        tokio::spawn(async move {
            let shutting_down = std::sync::Arc::new(
                std::sync::atomic::AtomicBool::new(false),
            );
            let connector =
                discord::DiscordConnector::new(&discord_token, &agent_name, discord_tx, shutting_down);
            if let Err(e) = connector.run().await {
                tracing::error!("Discord connector exited with error: {e}");
            }
        });

        tokio::spawn(async move {
            while let Some(msg) = discord_rx.recv().await {
                let lane = format!("discord:{}", msg.channel_id);
                let payload = serde_json::json!({
                    "channel_id": msg.channel_id,
                    "user_id": msg.user_id,
                    "username": msg.username,
                    "content": msg.content,
                    "message_id": msg.message_id,
                });
                match queue.push(&lane, payload).await {
                    Ok(id) => tracing::debug!("Discord message queued: job={id} lane={lane}"),
                    Err(e) => tracing::error!("Failed to enqueue Discord message: {e}"),
                }
            }
        });
    }

    // Start server
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("sera-core-rs listening on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("sera-core-rs shut down gracefully");
    Ok(())
}

/// Build the complete application router.
fn build_router(
    state: AppState,
    jwt_service: Arc<JwtService>,
    api_key: Arc<String>,
) -> Router {
    // Public routes (no auth)
    let public = Router::new()
        .route("/api/health", get(routes::health::health))
        .route("/api/health/detail", get(routes::health::health_detail))
        // OIDC auth flow — must be public (user not yet authenticated)
        .route("/api/auth/oidc-config", get(routes::oidc::get_oidc_config))
        .route("/api/auth/login", get(routes::oidc::login))
        .route("/api/auth/oidc/callback", post(routes::oidc::callback))
        .route("/api/auth/logout", post(routes::oidc::logout));

    // Protected routes (require auth)
    let protected = Router::new()
        .route("/api/agents/templates", get(routes::agents::list_templates))
        .route("/api/agents", get(routes::agents::list_instances))
        .route("/api/agents/{id}", get(routes::agents::get_instance))
        .route("/api/providers/list", get(routes::providers::list_providers))
        .route(
            "/api/budget/agents/{agent_id}",
            get(routes::metering::get_agent_budget),
        )
        // Skills — GET list + POST create
        .route(
            "/api/skills",
            get(routes::skills::list_skills).post(routes::skills::create_skill),
        )
        .route("/api/skills/{name}", get(routes::skills::get_skill).delete(routes::skills::delete_skill))
        // Schedules — GET list + POST create
        .route(
            "/api/schedules",
            get(routes::schedules::list_schedules).post(routes::schedules::create_schedule),
        )
        // Circles — GET list + POST create
        .route(
            "/api/circles",
            get(routes::circles::list_circles).post(routes::circles::create_circle),
        )
        .route("/api/circles/{id}", get(routes::circles::get_circle).patch(routes::circles::update_circle).delete(routes::circles::delete_circle))
        // Sessions — GET list + POST create
        .route(
            "/api/sessions",
            get(routes::sessions::list_sessions).post(routes::sessions::create_session),
        )
        .route(
            "/api/sessions/{id}",
            get(routes::sessions::get_session)
                .put(routes::sessions::update_session)
                .delete(routes::sessions::delete_session),
        )
        // Agent instance CRUD
        .route("/api/agents/instances", post(routes::agents::create_instance))
        .route(
            "/api/agents/instances/{id}",
            get(routes::agents::get_instance)
                .patch(routes::agents::update_instance)
                .delete(routes::agents::delete_instance),
        )
        .route("/api/agents/instances/{id}/start", post(routes::agents::start_instance))
        .route("/api/agents/instances/{id}/stop", post(routes::agents::stop_instance))
        .route("/api/agents/instances/{id}/restart", post(routes::agents::restart_instance))
        .route("/api/agents/instances/{id}/status", get(routes::agents::get_agent_status))
        .route("/api/agents/instances/{id}/metrics", get(routes::agents::get_agent_metrics))
        .route("/api/agents/instances/{id}/skills", post(routes::agents::add_agent_skill))
        .route("/api/agents/instances/{id}/skills/{skill_name}", delete(routes::agents::remove_agent_skill))
        .route("/api/agents/instances/{id}/tools", get(routes::stubs::agent_tools))
        // Providers write endpoints
        .route("/api/providers", post(routes::providers::add_provider))
        .route(
            "/api/providers/{model_name}",
            patch(routes::providers::update_provider).delete(routes::providers::delete_provider),
        )
        // Budget write endpoints
        .route(
            "/api/budget/agents/{agent_id}/budget",
            get(routes::metering::get_agent_budget).patch(routes::metering::update_agent_budget),
        )
        .route(
            "/api/budget/agents/{agent_id}/budget/reset",
            post(routes::metering::reset_agent_budget),
        )
        // Metering record + read endpoints
        .route("/api/metering/usage", get(routes::metering::get_usage).post(routes::metering::record_usage))
        // Audit — GET log + POST append (frontend uses /api/audit for both)
        .route("/api/audit", get(routes::audit::get_audit_log).post(routes::audit::append_audit))
        .route("/api/audit/log", get(routes::audit::get_audit_log))
        .route("/api/audit/{sequence}", get(routes::audit::get_audit_by_sequence))
        .route("/api/audit/verify", get(routes::stubs::audit_verify))
        // Secrets CRUD
        .route(
            "/api/secrets",
            get(routes::secrets::list_secrets).post(routes::secrets::create_secret),
        )
        .route(
            "/api/secrets/{key}",
            get(routes::secrets::get_secret).delete(routes::secrets::delete_secret),
        )
        // Budget read endpoints
        .route("/api/budget", get(routes::metering::get_global_budget))
        .route("/api/budget/agents", get(routes::metering::get_agent_rankings))
        // Metering read endpoints
        .route("/api/metering/summary", get(routes::metering::get_metering_summary))
        // LLM Proxy
        .route("/v1/llm/models", get(routes::llm_proxy::list_models))
        .route("/v1/llm/chat/completions", post(routes::llm_proxy::chat_completions))
        // Memory blocks
        .route(
            "/api/memory/blocks",
            get(routes::memory::list_blocks).post(routes::memory::create_block),
        )
        .route(
            "/api/memory/blocks/{id}",
            get(routes::memory::get_block)
                .put(routes::memory::update_block)
                .delete(routes::memory::delete_block),
        )
        .route("/api/memory/search", post(routes::memory::search_memory))
        .route("/api/memory/versions/{agent_id}", get(routes::memory::get_memory_versions))
        .route("/api/memory/versions/{agent_id}/snapshot", post(routes::memory::create_memory_snapshot))
        // Task queue
        .route("/api/agents/{id}/tasks", get(routes::tasks::list_tasks).post(routes::tasks::enqueue_task))
        .route("/api/agents/{id}/tasks/next", get(routes::tasks::poll_next_task))
        .route("/api/agents/{id}/tasks/{task_id}/result", post(routes::tasks::submit_task_result))
        .route("/api/agents/{id}/tasks/{task_id}", get(routes::tasks::get_task).delete(routes::tasks::cancel_task))
        .route("/api/agents/{id}/tasks/history", get(routes::tasks::get_task_history))
        // Operator requests
        .route("/api/operator-requests/pending/count", get(routes::operator_requests::pending_count))
        .route(
            "/api/operator-requests",
            get(routes::operator_requests::list_requests)
                .post(routes::operator_requests::create_request),
        )
        .route("/api/operator-requests/{id}/respond", post(routes::operator_requests::respond_to_request))
        // Heartbeat + lifecycle
        .route("/api/agents/{id}/heartbeat", post(routes::heartbeat::heartbeat))
        .route("/api/agents/{id}/lifecycle", get(routes::heartbeat::get_lifecycle))
        .route("/api/agents/{id}/lifecycle/start", post(routes::agents::start_instance))
        .route("/api/agents/{id}/lifecycle/stop", post(routes::agents::stop_instance))
        // Notification channels
        .route(
            "/api/channels",
            get(routes::channels::list_channels).post(routes::channels::create_channel),
        )
        .route("/api/channels/{id}", delete(routes::channels::delete_channel))
        // MCP servers
        .route("/api/mcp-servers", get(routes::mcp::list_mcp_servers).post(routes::mcp::register_mcp_server))
        .route("/api/mcp-servers/{name}", get(routes::mcp::get_mcp_server).delete(routes::mcp::delete_mcp_server))
        .route("/api/mcp-servers/{name}/health", get(routes::mcp::mcp_server_health))
        .route("/api/mcp-servers/{name}/reload", post(routes::mcp::reload_mcp_server))
        // Tool management
        .route("/api/tools", get(routes::mcp::list_tools))
        .route("/api/tools/execute", post(routes::mcp::execute_tool))
        .route("/api/tools/validate", post(routes::mcp::validate_tool))
        // LSP proxy
        .route("/api/lsp/definition", post(routes::lsp::definition))
        .route("/api/lsp/references", post(routes::lsp::references))
        .route("/api/lsp/symbols", post(routes::lsp::symbols))
        // Webhooks
        .route(
            "/api/webhooks",
            get(routes::webhooks::list_webhooks).post(routes::webhooks::create_webhook),
        )
        // Config + system stubs
        .route("/api/config/llm", get(routes::config::get_llm_config))
        .route("/api/config/providers", get(routes::config::list_providers))
        .route("/api/config/reload", post(routes::config::reload_config))
        .route("/api/federation/peers", get(routes::config::list_federation_peers))
        .route("/api/system/circuit-breakers", get(routes::config::get_circuit_breakers))
        .route("/api/rt/token", get(routes::config::get_rt_token))
        // Auth — API key management
        .route("/api/auth/me", get(routes::auth::get_me))
        .route(
            "/api/auth/api-keys",
            get(routes::auth::list_api_keys).post(routes::auth::create_api_key),
        )
        .route("/api/auth/api-keys/{id}", delete(routes::auth::delete_api_key))
        // Delegation
        .route("/api/delegation", get(routes::delegation::list_delegations))
        .route("/api/delegation/issue", post(routes::delegation::issue_delegation))
        .route(
            "/api/delegation/{id}",
            delete(routes::delegation::revoke_delegation),
        )
        .route("/api/delegation/{id}/children", get(routes::delegation::get_delegation_children))
        .route("/api/agents/{agent_id}/delegations", get(routes::delegation::get_agent_delegations))
        // Registry — advanced template/instance CRUD
        .route(
            "/api/registry/templates",
            get(routes::registry::list_templates).post(routes::registry::upsert_template),
        )
        .route(
            "/api/registry/templates/{name}",
            get(routes::registry::get_template)
                .put(routes::registry::update_template)
                .delete(routes::registry::delete_template),
        )
        .route("/api/registry/instances", get(routes::agents::list_instances).post(routes::agents::create_instance))
        // Sandbox — real bollard implementation
        .route("/api/sandbox/spawn", post(routes::sandbox::spawn))
        .route("/api/sandbox/exec", post(routes::sandbox::exec))
        // Intercom — Centrifugo HTTP client
        .route("/api/intercom/publish", post(routes::intercom::publish))
        .route("/api/intercom/dm", post(routes::intercom::dm))
        .route("/api/intercom/centrifugo/token", get(routes::intercom::get_connection_token))
        // Pipelines — workflow engine
        .route("/api/pipelines", post(routes::pipelines::create_pipeline))
        .route("/api/pipelines/{id}", get(routes::pipelines::get_pipeline))
        // Chat — container routing with SSE streaming
        .route("/api/chat", post(routes::chat::chat))
        .route("/api/chat/stream", post(routes::chat::stream_chat))
        .route("/api/chat/completions", post(routes::chat::completions))
        // Chat session messages
        .route(
            "/api/chat/sessions/{id}/messages",
            post(routes::chat::add_message).get(routes::chat::list_messages),
        )
        // OpenAI-compatible endpoint
        .route("/v1/chat/completions", post(routes::openai_compat::chat_completions))
        // Embedding — Ollama integration
        .route("/api/embedding/config", get(routes::embedding::get_config).put(routes::embedding::update_config))
        .route("/api/embedding/status", get(routes::embedding::get_status))
        .route("/api/embedding/models", get(routes::embedding::list_models))
        .route("/api/embedding/test", post(routes::embedding::test_embedding))
        .route("/api/embedding/embed", post(routes::embedding::embed_text))
        .route("/api/embedding/batch", post(routes::embedding::embed_batch))
        // Knowledge — agent knowledge + git history + merge requests
        .route("/api/knowledge/{agent_id}", get(routes::embedding::get_knowledge).post(routes::embedding::update_knowledge))
        .route("/api/knowledge/{agent_id}/history", get(routes::embedding::get_knowledge_history))
        .route("/api/knowledge/{agent_id}/diff", get(routes::embedding::get_knowledge_diff))
        .route("/api/knowledge/circles/{id}/history", get(routes::knowledge::get_history))
        .route("/api/knowledge/circles/{id}/merge-requests", get(routes::knowledge::list_merge_requests).post(routes::knowledge::create_merge_request))
        .route("/api/knowledge/circles/{id}/merge-requests/{mr_id}/approve", post(routes::knowledge::approve_merge_request))
        .route("/api/knowledge/circles/{id}/merge-requests/{mr_id}/reject", post(routes::knowledge::reject_merge_request))
        // Permission requests
        .route("/api/permission-requests", get(routes::permission_requests::list_requests).post(routes::permission_requests::create_request))
        .route("/api/permission-requests/{id}/approve", post(routes::permission_requests::approve_request))
        .route("/api/permission-requests/{id}/deny", post(routes::permission_requests::deny_request))
        // Self-evolution pipeline (SPEC-self-evolution §16): propose →
        // evaluate → approve → apply. Every transition fires the
        // `on_change_artifact_proposed` hook chain with `HookContext.change_artifact` populated.
        .route("/api/evolve/propose", post(routes::evolve::propose))
        .route("/api/evolve/evaluate/{id}", post(routes::evolve::evaluate))
        .route("/api/evolve/approve/{id}", post(routes::evolve::approve))
        .route("/api/evolve/apply/{id}", post(routes::evolve::apply))
        .route("/api/evolve/operator-key/{id}", post(routes::evolve::supply_operator_key))
        .route("/api/evolve/{id}", get(routes::evolve::get))
        // Service identities
        .route("/api/agents/{agent_id}/service-identities", get(routes::service_identities::list_identities).post(routes::service_identities::create_identity))
        .route("/api/agents/{agent_id}/service-identities/{identity_id}", delete(routes::service_identities::delete_identity))
        .route("/api/agents/{agent_id}/service-identities/{identity_id}/rotate", post(routes::service_identities::rotate_key))
        // Agent sub-route stubs
        .route("/api/agents/{id}/logs", get(routes::stubs::agent_logs))
        .route("/api/agents/{id}/subagents", get(routes::stubs::agent_subagents))
        .route("/api/agents/{id}/template-diff", get(routes::stubs::agent_template_diff))
        .route("/api/agents/{id}/grants", get(routes::stubs::agent_grants))
        .route("/api/agents/{id}/context-debug", get(routes::stubs::agent_context_debug))
        .route("/api/agents/{id}/system-prompt", get(routes::stubs::agent_system_prompt))
        .route("/api/agents/{id}/health-check", get(routes::stubs::agent_health_check))
        .route("/api/agents/{id}/sessions/{sid}/commands", get(routes::stubs::session_commands))
        .route("/api/agents/pending-updates", get(routes::stubs::pending_updates))
        .route("/v1/tools/catalog", get(routes::stubs::tools_catalog))
        .route("/api/templates", get(routes::stubs::list_templates))
        // Schedule detail + runs
        .route("/api/schedules/{id}", get(routes::stubs::get_schedule).patch(routes::schedules::update_schedule).delete(routes::schedules::delete_schedule))
        .route("/api/schedules/runs", get(routes::stubs::schedule_runs))
        // Provider dynamic/template stubs
        .route("/api/providers/dynamic", get(routes::stubs::providers_dynamic))
        .route("/api/providers/dynamic/statuses", get(routes::stubs::providers_dynamic_statuses))
        .route("/api/providers/templates", get(routes::stubs::providers_templates))
        .route("/api/providers/default-model", get(routes::stubs::providers_default_model).put(routes::stubs::set_default_model))
        // Memory advanced
        .route("/api/memory/recent", get(routes::stubs::memory_recent))
        .route("/api/memory/explorer-graph", get(routes::stubs::memory_explorer_graph))
        .route("/api/memory/overview", get(routes::stubs::memory_overview))
        .route("/api/memory/{agent_id}/core", get(routes::stubs::agent_core_memory))
        .route("/api/memory/{agent_id}/core/{name}", put(routes::stubs::update_core_memory))
        .route("/api/memory/{agent_id}/blocks", get(routes::stubs::agent_scoped_blocks))
        .route("/api/memory/{agent_id}/blocks/{block_id}", delete(routes::stubs::delete_agent_block))
        .layer(from_fn(move |req, next| {
            let jwt = jwt_service.clone();
            let key = api_key.clone();
            middleware::require_auth(req, next, jwt, key)
        }));

    // Combine and add shared middleware
    Router::new()
        .merge(public)
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Wait for SIGTERM or SIGINT for graceful shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("Received SIGINT"),
        () = terminate => tracing::info!("Received SIGTERM"),
    }
}
