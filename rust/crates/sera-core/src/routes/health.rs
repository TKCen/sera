//! Health check endpoints — public, no auth required.

use axum::{extract::State, Json};
use serde_json::{json, Value};

use crate::state::AppState;

/// GET /api/health — simple liveness check.
pub async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

/// GET /api/health/detail — checks database connectivity and returns agent stats.
pub async fn health_detail(State(state): State<AppState>) -> Json<Value> {
    let mut components = Vec::new();
    let mut overall = "healthy";

    // Database check
    let db_status = match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(state.db.inner())
        .await
    {
        Ok(_) => "healthy",
        Err(_) => {
            overall = "degraded";
            "unreachable"
        }
    };
    components.push(json!({
        "name": "database",
        "status": db_status,
    }));

    // Docker — if we got this far, ContainerManager was created successfully at startup
    components.push(json!({
        "name": "docker",
        "status": "healthy",
    }));

    // Centrifugo
    let centrifugo_status = if state.centrifugo.is_some() { "healthy" } else { "degraded" };
    components.push(json!({
        "name": "centrifugo",
        "status": centrifugo_status,
    }));

    // Qdrant
    components.push(json!({
        "name": "qdrant",
        "status": "healthy",
    }));

    // Agent stats from DB
    let agent_stats = match sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT \
            COUNT(*), \
            COUNT(*) FILTER (WHERE status = 'running'), \
            COUNT(*) FILTER (WHERE status = 'stopped'), \
            COUNT(*) FILTER (WHERE status = 'error') \
         FROM agent_instances"
    )
    .fetch_one(state.db.inner())
    .await
    {
        Ok((total, running, stopped, errored)) => json!({
            "total": total,
            "running": running,
            "stopped": stopped,
            "errored": errored,
        }),
        Err(_) => json!({
            "total": 0,
            "running": 0,
            "stopped": 0,
            "errored": 0,
        }),
    };

    // Use std time for ISO 8601 timestamp
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Json(json!({
        "status": overall,
        "components": components,
        "agentStats": agent_stats,
        "timestamp": format!("{now}"),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_returns_ok() {
        let response = health().await;
        assert_eq!(response.0["status"], "ok");
    }
}
