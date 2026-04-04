//! Health check endpoints — public, no auth required.

use axum::{extract::State, Json};
use serde_json::{json, Value};

use crate::state::AppState;

/// GET /api/health — simple liveness check.
pub async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

/// GET /api/health/detail — checks database connectivity.
pub async fn health_detail(State(state): State<AppState>) -> Json<Value> {
    let db_status = match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(state.db.inner())
        .await
    {
        Ok(_) => "ok",
        Err(_) => "degraded",
    };

    Json(json!({
        "status": if db_status == "ok" { "ok" } else { "degraded" },
        "database": db_status,
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
