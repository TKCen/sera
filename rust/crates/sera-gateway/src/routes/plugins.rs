//! Plugin registry endpoints.
//!
//! Routes:
//!   GET  /api/plugins               — list registered plugins
//!   POST /api/plugins/{id}/call     — call a plugin (stub; full gRPC wiring is a follow-up)
//!   POST /api/plugins/hot-reload    — trigger hot-reload of plugin manifests
#![allow(dead_code)]

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use sera_plugins::{InMemoryPluginRegistry, PluginInfo, PluginRegistry};

// ---------------------------------------------------------------------------
// Request/response shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct ListPluginsResponse {
    pub plugins: Vec<PluginInfo>,
    pub count: usize,
}

#[derive(Debug, Deserialize)]
pub struct CallPluginRequest {
    /// Arbitrary JSON payload forwarded to the plugin.
    pub input: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct CallPluginResponse {
    pub plugin_id: String,
    pub output: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HotReloadResponse {
    pub ok: bool,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Auth helper
// ---------------------------------------------------------------------------

fn check_auth(api_key: &Option<String>, headers: &HeaderMap) -> Result<(), StatusCode> {
    let expected = match api_key {
        None => return Ok(()),
        Some(k) => k,
    };
    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match provided {
        Some(k) if k == expected => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

// ---------------------------------------------------------------------------
// AppState abstraction
// ---------------------------------------------------------------------------

/// Abstraction over AppState for plugin handlers.
pub trait PluginsAppState: Send + Sync + 'static {
    fn api_key(&self) -> &Option<String>;
    fn plugin_registry(&self) -> Arc<InMemoryPluginRegistry>;
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/plugins — list all registered plugins.
pub async fn list_plugins<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
) -> Result<Json<ListPluginsResponse>, StatusCode>
where
    S: PluginsAppState,
{
    check_auth(state.api_key(), &headers)?;

    let registry = state.plugin_registry();
    let plugins = registry.list().await;
    let count = plugins.len();
    Ok(Json(ListPluginsResponse { plugins, count }))
}

/// POST /api/plugins/{id}/call — call a plugin by name.
///
/// Full gRPC dispatch is deferred (follow-up: sera-ne64-grpc).
/// Returns 501 Not Implemented when the plugin exists but gRPC isn't wired yet.
pub async fn call_plugin<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CallPluginRequest>,
) -> Result<Json<CallPluginResponse>, StatusCode>
where
    S: PluginsAppState,
{
    check_auth(state.api_key(), &headers)?;

    let registry = state.plugin_registry();
    // Verify the plugin exists; return 404 if not.
    let _info = registry.get(&id).await.map_err(|_| StatusCode::NOT_FOUND)?;

    // gRPC dispatch not yet implemented — return 501 with a stub response.
    // Filed as follow-up: full gRPC wiring via tonic.
    let _ = body.input;
    Err(StatusCode::NOT_IMPLEMENTED)
}

/// POST /api/plugins/hot-reload — reload plugin manifests from disk.
///
/// Manifest scanning and re-registration are deferred (follow-up: sera-ne64-reload).
pub async fn hot_reload<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
) -> Result<Json<HotReloadResponse>, StatusCode>
where
    S: PluginsAppState,
{
    check_auth(state.api_key(), &headers)?;

    // Manifest re-scan not yet implemented — stub that acknowledges the request.
    Ok(Json(HotReloadResponse {
        ok: true,
        message: "hot-reload acknowledged (manifest re-scan not yet implemented)".into(),
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
    };
    use sera_plugins::{
        GrpcTransportConfig, PluginCapability, PluginRegistration, PluginTransport, PluginVersion,
    };
    use std::time::Duration;
    use tower::ServiceExt;

    struct TestState {
        api_key: Option<String>,
        registry: Arc<InMemoryPluginRegistry>,
    }

    impl TestState {
        fn new(key: Option<&str>) -> Arc<Self> {
            Arc::new(Self {
                api_key: key.map(|k| k.to_owned()),
                registry: Arc::new(InMemoryPluginRegistry::new()),
            })
        }

        async fn with_plugin(key: Option<&str>, name: &str) -> Arc<Self> {
            let s = Self::new(key);
            let reg = PluginRegistration {
                name: name.to_owned(),
                version: PluginVersion::new(1, 0, 0),
                capabilities: vec![PluginCapability::ToolExecutor],
                transport: PluginTransport::Grpc {
                    grpc: GrpcTransportConfig {
                        endpoint: "localhost:9090".to_owned(),
                        tls: None,
                    },
                },
                health_check_interval: Duration::from_secs(30),
            };
            s.registry.register(reg).await.unwrap();
            s
        }
    }

    impl PluginsAppState for TestState {
        fn api_key(&self) -> &Option<String> {
            &self.api_key
        }
        fn plugin_registry(&self) -> Arc<InMemoryPluginRegistry> {
            Arc::clone(&self.registry)
        }
    }

    fn test_router(state: Arc<TestState>) -> Router {
        Router::new()
            .route("/api/plugins", get(list_plugins::<TestState>))
            .route("/api/plugins/{id}/call", post(call_plugin::<TestState>))
            .route("/api/plugins/hot-reload", post(hot_reload::<TestState>))
            .with_state(state)
    }

    // --- happy path: list returns empty registry ---

    #[tokio::test]
    async fn list_plugins_empty() {
        let app = test_router(TestState::new(None));
        let resp = app
            .oneshot(Request::get("/api/plugins").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: ListPluginsResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(result.count, 0);
        assert!(result.plugins.is_empty());
    }

    // --- happy path: list returns registered plugin ---

    #[tokio::test]
    async fn list_plugins_with_entry() {
        let state = TestState::with_plugin(None, "my-plugin").await;
        let app = test_router(state);
        let resp = app
            .oneshot(Request::get("/api/plugins").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: ListPluginsResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(result.count, 1);
        assert_eq!(result.plugins[0].registration.name, "my-plugin");
    }

    // --- auth denied: list requires key ---

    #[tokio::test]
    async fn list_plugins_auth_denied() {
        let app = test_router(TestState::new(Some("secret")));
        let resp = app
            .oneshot(Request::get("/api/plugins").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // --- not found: call for unknown plugin ---

    #[tokio::test]
    async fn call_plugin_not_found() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({"input": {}});
        let resp = app
            .oneshot(
                Request::post("/api/plugins/ghost/call")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // --- stub: call for known plugin returns 501 ---

    #[tokio::test]
    async fn call_plugin_returns_not_implemented() {
        let state = TestState::with_plugin(None, "my-plugin").await;
        let app = test_router(state);
        let body = serde_json::json!({"input": {"foo": "bar"}});
        let resp = app
            .oneshot(
                Request::post("/api/plugins/my-plugin/call")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    }

    // --- happy path: hot-reload returns ok ---

    #[tokio::test]
    async fn hot_reload_happy_path() {
        let app = test_router(TestState::new(None));
        let resp = app
            .oneshot(
                Request::post("/api/plugins/hot-reload")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: HotReloadResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(result.ok);
    }

    // --- auth denied: hot-reload requires key ---

    #[tokio::test]
    async fn hot_reload_auth_denied() {
        let app = test_router(TestState::new(Some("secret")));
        let resp = app
            .oneshot(
                Request::post("/api/plugins/hot-reload")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
