//! SERA Core — the central API server and orchestration engine.
//!
//! This binary replaces the TypeScript sera-core Express server.
//! Built on axum + tokio + sqlx.

mod error;
mod middleware;
mod routes;
mod state;

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

use sera_auth::JwtService;
use sera_config::core_config::CoreConfig;
use sera_config::providers::ProvidersConfig;
use sera_db::DbPool;
use sera_docker::ContainerManager;

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

    // Initialize Docker client
    let docker = Arc::new(ContainerManager::new().map_err(|e| {
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

    let app_state = AppState {
        db,
        config: config.clone(),
        jwt: jwt_service.clone(),
        providers,
        docker,
        providers_path,
        centrifugo,
        mcp_registry: Arc::new(RwLock::new(routes::mcp::McpRegistry::new())),
    };

    // Build router
    let app = build_router(app_state, jwt_service, api_key);

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
        .route("/api/health/detail", get(routes::health::health_detail));

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
        .route("/api/skills/{name}", delete(routes::skills::delete_skill))
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
        .route("/api/circles/{id}", delete(routes::circles::delete_circle))
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
        // Agent instance write endpoints
        .route("/api/agents/instances", post(routes::agents::create_instance))
        .route(
            "/api/agents/instances/{id}",
            patch(routes::agents::update_instance).delete(routes::agents::delete_instance),
        )
        .route("/api/agents/instances/{id}/start", post(routes::agents::start_instance))
        .route("/api/agents/instances/{id}/stop", post(routes::agents::stop_instance))
        // Providers write endpoints
        .route("/api/providers", post(routes::providers::add_provider))
        .route(
            "/api/providers/{model_name}",
            patch(routes::providers::update_provider).delete(routes::providers::delete_provider),
        )
        // Budget write endpoints
        .route(
            "/api/budget/agents/{agent_id}/budget",
            patch(routes::metering::update_agent_budget),
        )
        .route(
            "/api/budget/agents/{agent_id}/budget/reset",
            post(routes::metering::reset_agent_budget),
        )
        // Metering record endpoint
        .route("/api/metering/usage", post(routes::metering::record_usage))
        // Audit — GET log + POST append
        .route("/api/audit/log", get(routes::audit::get_audit_log))
        .route("/api/audit", post(routes::audit::append_audit))
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
            "/api/memory/entries/{id}",
            get(routes::memory::get_block)
                .put(routes::memory::update_block)
                .delete(routes::memory::delete_block),
        )
        // Task queue
        .route("/api/agents/{id}/tasks", post(routes::tasks::enqueue_task))
        .route("/api/agents/{id}/tasks/next", get(routes::tasks::poll_next_task))
        .route("/api/agents/{id}/tasks/{task_id}/result", post(routes::tasks::submit_task_result))
        .route("/api/agents/{id}/tasks/history", get(routes::tasks::get_task_history))
        // Operator requests
        .route("/api/operator-requests/pending/count", get(routes::operator_requests::pending_count))
        .route("/api/operator-requests", get(routes::operator_requests::list_requests))
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
        .route("/api/mcp-servers/{name}", get(routes::mcp::get_mcp_server))
        .route("/api/mcp-servers/{name}/health", get(routes::mcp::mcp_server_health))
        .route("/api/mcp-servers/{name}/reload", post(routes::mcp::reload_mcp_server))
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
        // Pipelines — workflow engine
        .route("/api/pipelines", post(routes::pipelines::create_pipeline))
        .route("/api/pipelines/{id}", get(routes::pipelines::get_pipeline))
        // Chat — container routing with SSE streaming
        .route("/api/chat", post(routes::chat::chat))
        // OpenAI-compatible endpoint
        .route("/v1/chat/completions", post(routes::openai_compat::chat_completions))
        // Embedding — Ollama integration
        .route("/api/embedding/config", get(routes::embedding::get_config).put(routes::embedding::update_config))
        .route("/api/embedding/status", get(routes::embedding::get_status))
        .route("/api/embedding/models", get(routes::embedding::list_models))
        .route("/api/embedding/test", post(routes::embedding::test_embedding))
        // Knowledge — git history + merge requests
        .route("/api/knowledge/circles/{id}/history", get(routes::knowledge::get_history))
        .route("/api/knowledge/circles/{id}/merge-requests", get(routes::knowledge::list_merge_requests).post(routes::knowledge::create_merge_request))
        .route("/api/knowledge/circles/{id}/merge-requests/{mr_id}/approve", post(routes::knowledge::approve_merge_request))
        .route("/api/knowledge/circles/{id}/merge-requests/{mr_id}/reject", post(routes::knowledge::reject_merge_request))
        // Permission requests
        .route("/api/permission-requests", get(routes::permission_requests::list_requests).post(routes::permission_requests::create_request))
        .route("/api/permission-requests/{id}/approve", post(routes::permission_requests::approve_request))
        .route("/api/permission-requests/{id}/deny", post(routes::permission_requests::deny_request))
        // Service identities
        .route("/api/agents/{agent_id}/service-identities", get(routes::service_identities::list_identities).post(routes::service_identities::create_identity))
        .route("/api/agents/{agent_id}/service-identities/{identity_id}", delete(routes::service_identities::delete_identity))
        .route("/api/agents/{agent_id}/service-identities/{identity_id}/rotate", post(routes::service_identities::rotate_key))
        // OIDC auth flow
        .route("/api/auth/oidc-config", get(routes::oidc::get_oidc_config))
        .route("/api/auth/login", get(routes::oidc::login))
        .route("/api/auth/oidc/callback", post(routes::oidc::callback))
        .route("/api/auth/logout", post(routes::oidc::logout))
        // Agent sub-route stubs
        .route("/api/agents/{id}/logs", get(routes::stubs::agent_logs))
        .route("/api/agents/{id}/subagents", get(routes::stubs::agent_subagents))
        .route("/api/agents/pending-updates", get(routes::stubs::pending_updates))
        .route("/api/tools", get(routes::stubs::list_tools))
        .route("/api/templates", get(routes::stubs::list_templates))
        // Schedule detail + runs
        .route("/api/schedules/{id}", get(routes::stubs::get_schedule).patch(routes::schedules::update_schedule).delete(routes::schedules::delete_schedule))
        .route("/api/schedules/runs", get(routes::stubs::schedule_runs))
        // Memory advanced
        .route("/api/memory/overview", get(routes::stubs::memory_overview))
        .route("/api/memory/{agent_id}/core", get(routes::stubs::agent_core_memory))
        .route("/api/memory/{agent_id}/core/{name}", axum::routing::put(routes::stubs::update_core_memory))
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
