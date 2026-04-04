//! SERA Core — the central API server and orchestration engine.
//!
//! This binary replaces the TypeScript sera-core Express server.
//! Built on axum + tokio + sqlx.

mod error;
mod middleware;
mod routes;
mod state;

use std::sync::Arc;

use axum::{
    middleware::from_fn,
    routing::get,
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use sera_auth::JwtService;
use sera_config::core_config::CoreConfig;
use sera_db::DbPool;

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
    if let Ok(path) = std::env::var("SERA_PROVIDERS_JSON")
        && let Err(e) = config.load_providers(&path)
    {
        tracing::warn!("Failed to load providers.json: {e}");
    }

    let port = config.port;

    // Connect to database
    let db = DbPool::connect(&config.database_url).await?;
    tracing::info!("Connected to database");

    // Build shared state
    let jwt_service = Arc::new(JwtService::new(config.centrifugo.token_secret.clone()));
    let api_key = Arc::new(config.api_key.clone());
    let config = Arc::new(config);

    let app_state = AppState {
        db,
        config: config.clone(),
        jwt: jwt_service.clone(),
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
        .route("/api/audit/log", get(routes::audit::get_audit_log))
        .route(
            "/api/budget/agents/{agent_id}",
            get(routes::metering::get_agent_budget),
        )
        .route("/api/skills", get(routes::skills::list_skills))
        .route("/api/schedules", get(routes::schedules::list_schedules))
        .route("/api/circles", get(routes::circles::list_circles))
        .route("/api/sessions", get(routes::sessions::list_sessions))
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
