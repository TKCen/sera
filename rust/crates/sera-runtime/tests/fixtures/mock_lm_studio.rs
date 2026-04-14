//! Minimal OpenAI-compatible HTTP server for integration tests.
//!
//! Provides:
//!   POST /v1/chat/completions  — streaming (SSE) or non-streaming
//!   GET  /v1/models            — model list
//!
//! Error injection: query param `?force_error=rate_limit`
//! or header `X-Mock-Error: rate_limit` → 429.

use axum::extract::Query;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream;
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

// ---------------------------------------------------------------------------
// Public handle
// ---------------------------------------------------------------------------

/// Handle returned by `start_mock_lm_studio`. Drop or call `shutdown()` to stop.
pub struct MockServerHandle {
    pub port: u16,
    #[allow(dead_code)]
    pub shutdown: oneshot::Sender<()>,
}

impl MockServerHandle {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

// ---------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------

/// Start the mock server on a random port. Returns immediately; server runs
/// in the background until the `shutdown` sender is dropped or fired.
pub async fn start_mock_lm_studio() -> MockServerHandle {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind to random port");
    let port = listener.local_addr().expect("local addr").port();

    let (tx, rx) = oneshot::channel::<()>();

    let app = Router::new()
        .route("/v1/models", get(models_handler))
        .route("/v1/chat/completions", post(chat_handler));

    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .ok();
    });

    MockServerHandle { port, shutdown: tx }
}

// ---------------------------------------------------------------------------
// Query / header for error injection
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct ErrorQuery {
    force_error: Option<String>,
}

fn rate_limit_error_body() -> serde_json::Value {
    json!({
        "error": {
            "message": "rate limit exceeded",
            "type": "rate_limit_error"
        }
    })
}

fn should_force_error(query: &ErrorQuery, headers: &HeaderMap) -> bool {
    if query
        .force_error
        .as_deref()
        .map(|v| v == "rate_limit")
        .unwrap_or(false)
    {
        return true;
    }
    headers
        .get("x-mock-error")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "rate_limit")
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// GET /v1/models
// ---------------------------------------------------------------------------

async fn models_handler() -> Json<serde_json::Value> {
    Json(json!({
        "object": "list",
        "data": [{
            "id": "mock-model",
            "object": "model",
            "created": 1_700_000_000u64,
            "owned_by": "mock"
        }]
    }))
}

// ---------------------------------------------------------------------------
// POST /v1/chat/completions
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct ChatRequest {
    #[serde(default)]
    stream: Option<bool>,
}

async fn chat_handler(
    Query(query): Query<ErrorQuery>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    if should_force_error(&query, &headers) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(rate_limit_error_body()),
        )
            .into_response();
    }

    let streaming = req.stream.unwrap_or(false);

    if streaming {
        streaming_response().into_response()
    } else {
        non_streaming_response().into_response()
    }
}

// ---------------------------------------------------------------------------
// Streaming (SSE)
// ---------------------------------------------------------------------------

fn streaming_response() -> impl IntoResponse {
    // Three canned delta chunks + [DONE]
    let chunks: Vec<&'static str> = vec![
        r#"{"id":"chatcmpl-mock","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":null}]}"#,
        r#"{"id":"chatcmpl-mock","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Hello from mock"},"finish_reason":null}]}"#,
        r#"{"id":"chatcmpl-mock","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"!"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":4}}"#,
    ];

    let events: Vec<Result<Event, std::convert::Infallible>> = chunks
        .into_iter()
        .map(|c| Ok(Event::default().data(c)))
        .chain(std::iter::once(Ok(Event::default().data("[DONE]"))))
        .collect();

    Sse::new(stream::iter(events))
}

// ---------------------------------------------------------------------------
// Non-streaming
// ---------------------------------------------------------------------------

fn non_streaming_response() -> Json<serde_json::Value> {
    Json(json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello from mock (non-streaming)!"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 6,
            "total_tokens": 16
        }
    }))
}

