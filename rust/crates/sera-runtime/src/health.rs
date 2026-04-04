//! Health server — listens on AGENT_CHAT_PORT for health checks and chat requests.

use axum::{routing::get, routing::post, Json, Router};

/// Start the health/chat server on the given port.
pub async fn serve(port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/chat", post(handle_chat));

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Health server listening on {addr}");

    axum::serve(listener, app).await?;
    Ok(())
}

async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "runtime": "sera-runtime-rs",
    }))
}

/// Handle direct chat messages from sera-core.
async fn handle_chat(
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    // In persistent mode, this receives chat messages routed by sera-core.
    // For now, echo back — full implementation would run a reasoning loop per request.
    let message = body["message"].as_str().unwrap_or("");
    Json(serde_json::json!({
        "message": {
            "role": "assistant",
            "content": format!("Received: {message}"),
        },
        "status": "ok",
    }))
}
