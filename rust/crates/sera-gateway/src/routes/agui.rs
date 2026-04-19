//! AG-UI streaming protocol endpoints.
//!
//! Routes:
//!   GET  /api/agui/stream — SSE stream of AG-UI events (subscribe to EventSink)
//!   POST /api/agui/emit   — inject an event into the shared sink (test/internal helper)
#![allow(dead_code)]

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_stream::wrappers::UnboundedReceiverStream;
use futures_util::StreamExt;

use sera_agui::AgUiEvent;

// ---------------------------------------------------------------------------
// Shared broadcast hub
// ---------------------------------------------------------------------------

/// Simple broadcast hub: keeps a list of active sender channels.
/// When an event is emitted, it is forwarded to all connected subscribers.
#[derive(Default)]
pub struct AguiHub {
    senders: Vec<tokio::sync::mpsc::UnboundedSender<AgUiEvent>>,
}

impl AguiHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe a new client. Returns a `ChannelSink` for emitting and a
    /// receiver stream for the SSE handler to drain.
    pub fn subscribe(
        &mut self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<AgUiEvent> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.senders.push(tx);
        rx
    }

    /// Broadcast one event to all live subscribers; silently drops closed channels.
    pub fn broadcast(&mut self, event: AgUiEvent) {
        self.senders.retain(|tx| tx.send(event.clone()).is_ok());
    }

    /// Number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.senders.len()
    }
}

// ---------------------------------------------------------------------------
// Request/response shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct EmitRequest {
    #[serde(flatten)]
    pub event: AgUiEvent,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmitResponse {
    pub ok: bool,
    pub subscribers: usize,
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
// Handlers
// ---------------------------------------------------------------------------

/// Trait abstracting AppState fields needed by agui handlers.
pub trait AguiAppState: Send + Sync + 'static {
    fn api_key(&self) -> &Option<String>;
    fn agui_hub(&self) -> Arc<RwLock<AguiHub>>;
}

/// GET /api/agui/stream — subscribe to the AG-UI SSE event stream.
pub async fn stream_events<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, StatusCode>
where
    S: AguiAppState,
{
    check_auth(state.api_key(), &headers)?;

    let rx = {
        let hub_arc = state.agui_hub();
        let mut hub = hub_arc.write().await;
        hub.subscribe()
    };

    let event_stream = UnboundedReceiverStream::new(rx).map(|agui_event| {
        let data = agui_event.to_sse_data().unwrap_or_else(|_| "{}".into());
        Ok::<Event, Infallible>(
            Event::default()
                .event(agui_event.event_type())
                .data(data),
        )
    });

    Ok(Sse::new(event_stream).keep_alive(KeepAlive::default()))
}

/// POST /api/agui/emit — inject an event (internal/test helper, gated by auth).
pub async fn emit_event<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Json(body): Json<EmitRequest>,
) -> Result<Json<EmitResponse>, StatusCode>
where
    S: AguiAppState,
{
    check_auth(state.api_key(), &headers)?;

    let hub_arc = state.agui_hub();
    let mut hub = hub_arc.write().await;
    hub.broadcast(body.event);
    let after = hub.subscriber_count();

    Ok(Json(EmitResponse {
        ok: true,
        subscribers: after,
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use tower::ServiceExt;

    struct TestState {
        api_key: Option<String>,
        hub: Arc<RwLock<AguiHub>>,
    }

    impl TestState {
        fn new(key: Option<&str>) -> Arc<Self> {
            Arc::new(Self {
                api_key: key.map(|k| k.to_owned()),
                hub: Arc::new(RwLock::new(AguiHub::new())),
            })
        }
    }

    impl AguiAppState for TestState {
        fn api_key(&self) -> &Option<String> {
            &self.api_key
        }
        fn agui_hub(&self) -> Arc<RwLock<AguiHub>> {
            Arc::clone(&self.hub)
        }
    }

    fn test_router(state: Arc<TestState>) -> Router {
        Router::new()
            .route("/api/agui/stream", get(stream_events::<TestState>))
            .route("/api/agui/emit", post(emit_event::<TestState>))
            .with_state(state)
    }

    // --- happy path: emit returns ok ---

    #[tokio::test]
    async fn emit_happy_path() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "type": "RUN_STARTED",
            "thread_id": "t1",
            "run_id": "r1"
        });
        let resp = app
            .oneshot(
                Request::post("/api/agui/emit")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: EmitResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(result.ok);
    }

    // --- happy path: stream returns 200 text/event-stream ---

    #[tokio::test]
    async fn stream_happy_path() {
        let app = test_router(TestState::new(None));
        let resp = app
            .oneshot(
                Request::get("/api/agui/stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.contains("text/event-stream"), "content-type was: {ct}");
    }

    // --- auth denied: emit requires key ---

    #[tokio::test]
    async fn emit_auth_denied() {
        let app = test_router(TestState::new(Some("secret")));
        let body = serde_json::json!({
            "type": "RUN_STARTED",
            "thread_id": "t",
            "run_id": "r"
        });
        let resp = app
            .oneshot(
                Request::post("/api/agui/emit")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // --- auth denied: stream requires key ---

    #[tokio::test]
    async fn stream_auth_denied() {
        let app = test_router(TestState::new(Some("secret")));
        let resp = app
            .oneshot(
                Request::get("/api/agui/stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
