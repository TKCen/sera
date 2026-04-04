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
    routing::{delete, get, patch, post},
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

    let app_state = AppState {
        db,
        config: config.clone(),
        jwt: jwt_service.clone(),
        providers,
        docker,
        providers_path,
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
        .route(
            "/api/schedules/{id}",
            patch(routes::schedules::update_schedule).delete(routes::schedules::delete_schedule),
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
