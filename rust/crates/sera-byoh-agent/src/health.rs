use axum::{routing::get, Json, Router};
use sera_types::HealthResponse;
use tracing::info;

pub async fn serve(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = Router::new().route("/health", get(health_handler));

    let addr = format!("0.0.0.0:{port}");
    info!("Health server listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        ready: true,
        busy: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_response_ready() {
        let resp = HealthResponse {
            ready: true,
            busy: false,
        };
        assert!(resp.ready);
        assert!(!resp.busy);
    }

    #[test]
    fn health_response_serializes_to_json() {
        let resp = HealthResponse {
            ready: true,
            busy: false,
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains("\"ready\":true"));
        assert!(json.contains("\"busy\":false"));
    }

    #[test]
    fn health_response_deserializes_from_json() {
        let json = r#"{"ready":true,"busy":false}"#;
        let resp: HealthResponse = serde_json::from_str(json).expect("deserialize");
        assert!(resp.ready);
        assert!(!resp.busy);
    }

    #[test]
    fn health_response_busy_state() {
        let resp = HealthResponse {
            ready: true,
            busy: true,
        };
        assert!(resp.ready);
        assert!(resp.busy);
    }

    #[test]
    fn health_response_not_ready() {
        let resp = HealthResponse {
            ready: false,
            busy: false,
        };
        assert!(!resp.ready);
        assert!(!resp.busy);
    }

    #[test]
    fn health_response_roundtrip() {
        let original = HealthResponse {
            ready: true,
            busy: true,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: HealthResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.ready, original.ready);
        assert_eq!(parsed.busy, original.busy);
    }
}
