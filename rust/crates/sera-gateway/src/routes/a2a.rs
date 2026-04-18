//! A2A (Agent-to-Agent) protocol endpoints.
//!
//! Routes:
//!   POST /api/a2a/send    — forward a task to a peer via the outbound client
//!   GET  /api/a2a/peers   — list known A2A peers (from the inbound router registry)
//!   POST /api/a2a/accept  — accept an inbound A2A JSON-RPC request
#![allow(dead_code)]

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

use sera_a2a::{
    A2aClient, A2aRequest, A2aResponse, Capabilities, Task,
};
#[cfg(test)]
use sera_a2a::InProcRouter;

// ---------------------------------------------------------------------------
// Peer registry (in-process store, injected into AppState)
// ---------------------------------------------------------------------------

/// An entry in the gateway's A2A peer table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aPeer {
    pub name: String,
    pub url: String,
    pub capabilities: Capabilities,
}

/// In-memory registry of known A2A peers.
#[derive(Debug, Default, Clone)]
pub struct A2aPeerRegistry {
    pub peers: Vec<A2aPeer>,
}

impl A2aPeerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, peer: A2aPeer) {
        // Upsert by URL.
        if let Some(existing) = self.peers.iter_mut().find(|p| p.url == peer.url) {
            *existing = peer;
        } else {
            self.peers.push(peer);
        }
    }

    pub fn list(&self) -> &[A2aPeer] {
        &self.peers
    }
}

// ---------------------------------------------------------------------------
// Request / response shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    /// URL of the target A2A peer.
    pub peer_url: String,
    /// The task to send.
    pub task: Task,
}

#[derive(Debug, Serialize)]
pub struct SendMessageResponse {
    pub task: Task,
}

#[derive(Debug, Deserialize)]
pub struct AcceptRequest {
    /// Raw A2A JSON-RPC envelope forwarded from an inbound peer.
    #[serde(flatten)]
    pub request: A2aRequest,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Validate the `Authorization: Bearer <key>` header.
/// Returns `Ok(())` when auth is disabled or the key matches.
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
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/a2a/send — send a task to a remote A2A peer.
pub async fn send_message<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Json(body): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, StatusCode>
where
    S: A2aAppState,
{
    check_auth(state.api_key(), &headers)?;

    // Build a loopback client that can reach external peers.
    // In production the transport would use reqwest; for now we stub
    // with the inbound router in loopback mode and return NOT_IMPLEMENTED
    // for external URLs (the external HTTP transport is a follow-up).
    let client = state.a2a_client();
    let task = client
        .send_task(&body.peer_url, &body.task)
        .await
        .map_err(|_| StatusCode::NOT_IMPLEMENTED)?;

    Ok(Json(SendMessageResponse { task }))
}

/// GET /api/a2a/peers — list known A2A peers.
pub async fn list_peers<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
) -> Result<Json<Vec<A2aPeer>>, StatusCode>
where
    S: A2aAppState,
{
    check_auth(state.api_key(), &headers)?;
    let peers_arc = state.a2a_peers();
    let registry = peers_arc.read().await;
    Ok(Json(registry.list().to_vec()))
}

/// POST /api/a2a/accept — receive an inbound A2A JSON-RPC request.
pub async fn accept<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Json(body): Json<AcceptRequest>,
) -> Result<Json<A2aResponse>, StatusCode>
where
    S: A2aAppState,
{
    check_auth(state.api_key(), &headers)?;
    let router = state.a2a_router();
    let response = router.handle(body.request).await;
    Ok(Json(response))
}

// ---------------------------------------------------------------------------
// Trait that the local AppState must implement to supply A2A deps
// ---------------------------------------------------------------------------

/// Abstraction over AppState for A2A handlers.
/// This keeps the route file decoupled from the concrete AppState.
pub trait A2aAppState: Send + Sync + 'static {
    fn api_key(&self) -> &Option<String>;
    fn a2a_peers(&self) -> Arc<RwLock<A2aPeerRegistry>>;
    fn a2a_router(&self) -> Arc<dyn sera_a2a::A2aRouter>;
    fn a2a_client(&self) -> A2aClient;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use sera_a2a::{A2aRequest, A2aResponse, A2aRouter, A2aTransport, A2aError, LoopbackTransport};
    use tower::ServiceExt;

    // --- minimal AppState for tests ---

    struct TestState {
        api_key: Option<String>,
        peers: Arc<RwLock<A2aPeerRegistry>>,
        router: Arc<InProcRouter>,
    }

    impl TestState {
        fn new(key: Option<&str>) -> Arc<Self> {
            let router = Arc::new(InProcRouter::new(|_req: A2aRequest| async move {
                Ok(serde_json::json!({"ok": true}))
            }));
            Arc::new(Self {
                api_key: key.map(|k| k.to_owned()),
                peers: Arc::new(RwLock::new(A2aPeerRegistry::new())),
                router,
            })
        }
    }

    impl A2aAppState for TestState {
        fn api_key(&self) -> &Option<String> {
            &self.api_key
        }
        fn a2a_peers(&self) -> Arc<RwLock<A2aPeerRegistry>> {
            Arc::clone(&self.peers)
        }
        fn a2a_router(&self) -> Arc<dyn A2aRouter> {
            Arc::clone(&self.router) as Arc<dyn A2aRouter>
        }
        fn a2a_client(&self) -> A2aClient {
            // loopback: sends to our InProcRouter
            let transport = LoopbackTransport::from_arc(Arc::clone(&self.router));
            A2aClient::new(transport)
        }
    }

    fn test_router(state: Arc<TestState>) -> Router {
        Router::new()
            .route("/api/a2a/send", post(send_message::<TestState>))
            .route("/api/a2a/peers", get(list_peers::<TestState>))
            .route("/api/a2a/accept", post(accept::<TestState>))
            .with_state(state)
    }

    // --- happy-path: list peers returns empty vec ---

    #[tokio::test]
    async fn list_peers_happy_path() {
        let app = test_router(TestState::new(None));
        let resp = app
            .oneshot(Request::get("/api/a2a/peers").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let peers: Vec<A2aPeer> = serde_json::from_slice(&body).unwrap();
        assert!(peers.is_empty());
    }

    // --- happy-path: accept routes inbound request ---

    #[tokio::test]
    async fn accept_happy_path() {
        let app = test_router(TestState::new(None));
        let req_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "test-1",
            "method": "tasks/send",
            "params": {}
        });
        let resp = app
            .oneshot(
                Request::post("/api/a2a/accept")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp_json: A2aResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp_json.id, "test-1");
        assert!(resp_json.error.is_none());
    }

    // --- auth-denied: list peers rejects bad key ---

    #[tokio::test]
    async fn list_peers_auth_denied() {
        let app = test_router(TestState::new(Some("secret")));
        let resp = app
            .oneshot(Request::get("/api/a2a/peers").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // --- auth-denied: accept rejects bad key ---

    #[tokio::test]
    async fn accept_auth_denied() {
        let app = test_router(TestState::new(Some("secret")));
        let req_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "x",
            "method": "tasks/send",
            "params": {}
        });
        let resp = app
            .oneshot(
                Request::post("/api/a2a/accept")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // --- auth-denied: send rejects bad key ---

    #[tokio::test]
    async fn send_message_auth_denied() {
        let app = test_router(TestState::new(Some("secret")));
        let req_body = serde_json::json!({
            "peer_url": "http://peer",
            "task": {
                "id": "t1",
                "status": "submitted",
                "artifacts": [],
                "history": [],
                "metadata": {}
            }
        });
        let resp = app
            .oneshot(
                Request::post("/api/a2a/send")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
