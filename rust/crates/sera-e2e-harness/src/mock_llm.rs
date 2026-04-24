//! A scripted OpenAI-compatible mock LLM for integration tests.
//!
//! Wraps `wiremock::MockServer` with the **streaming** response shape
//! `sera-runtime`'s `LlmClient` expects.  The runtime hardcodes
//! `"stream": true` on every LLM request and parses the body as Server-Sent
//! Events (SSE), so this mock emits OpenAI-compat chunk frames rather than
//! a single JSON completion body.  Binds to an ephemeral loopback port so
//! multiple tests in the same process can each spin their own mock without
//! collision.
//!
//! The returned URL has no trailing slash and is ready to drop into
//! `LLM_BASE_URL` or into a Provider manifest's `base_url`.

use anyhow::Result;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

pub use wiremock::MockServer;

/// Default reply text the mock returns when a test doesn't supply its own.
pub const DEFAULT_REPLY: &str = "hello from mock LLM";

/// Start a minimal mock LLM returning [`DEFAULT_REPLY`].  Convenience wrapper
/// around [`start_mock_llm_with_reply`] — most tests only need deterministic
/// output, not specific content.
///
/// Returns `(url, server)` — the caller must hold `server` on its stack for
/// the duration of the test; dropping it tears down the listener.
pub async fn start_mock_llm() -> Result<(String, MockServer)> {
    start_mock_llm_with_reply(DEFAULT_REPLY).await
}

/// Start a minimal OpenAI-compatible streaming mock that always replies
/// with `reply`.
///
/// Registers both `/v1/chat/completions` (the canonical OpenAI path) and
/// the bare `/chat/completions` form that some compat backends use; the
/// runtime's `LlmClient` can hit either.  The response body is a single
/// SSE stream carrying one `delta` chunk with the full reply plus the
/// terminator — the runtime's accumulator handles both single-chunk and
/// multi-chunk streams identically.
///
/// Returns `(url, server)` — the caller must hold `server` on its stack for
/// the duration of the test; dropping it tears down the listener.
pub async fn start_mock_llm_with_reply(reply: &str) -> Result<(String, MockServer)> {
    let server = MockServer::start().await;
    let sse_body = build_sse_stream(reply);

    for p in ["/v1/chat/completions", "/chat/completions"] {
        Mock::given(method("POST"))
            .and(path(p))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .insert_header("cache-control", "no-cache")
                    .set_body_string(sse_body.clone()),
            )
            .mount(&server)
            .await;
    }

    let url = server.uri();
    Ok((url, server))
}

/// Render a minimal well-formed OpenAI-compat streaming completion that
/// carries `reply` in a single delta.  Format:
///
/// ```text
/// data: {"id":"...","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"<REPLY>"},"finish_reason":null}]}
///
/// data: {"id":"...","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}
///
/// data: [DONE]
///
/// ```
///
/// The runtime's `parse_sse_stream` accumulates `delta.content` across all
/// chunks and expects a terminal `finish_reason` + `[DONE]` marker.
fn build_sse_stream(reply: &str) -> String {
    let first = json!({
        "id": "chatcmpl-sera-e2e",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": "e2e-mock",
        "choices": [{
            "index": 0,
            "delta": { "role": "assistant", "content": reply },
            "finish_reason": null
        }]
    });
    let finish = json!({
        "id": "chatcmpl-sera-e2e",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": "e2e-mock",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 4,
            "completion_tokens": 4,
            "total_tokens": 8
        }
    });

    format!(
        "data: {first}\n\ndata: {finish}\n\ndata: [DONE]\n\n",
        first = first,
        finish = finish
    )
}
