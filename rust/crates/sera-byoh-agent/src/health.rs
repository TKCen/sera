use axum::{routing::get, Json, Router};
use sera_types::HealthResponse;
use tracing::info;

pub async fn serve(port: u16) {
    let app = Router::new().route("/health", get(health_handler));

    let addr = format!("0.0.0.0:{port}");
    info!("Health server listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        ready: true,
        busy: false,
    })
}
