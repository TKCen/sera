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

/// Start a two-phase mock LLM: the first request returns a `tool_calls`
/// chunk for `tool_name` with the supplied `args_json`; every subsequent
/// request returns `final_reply` as a content delta.
///
/// This is the fixture S4.1 (CapabilityPolicy deny) needs: the runtime
/// hardcodes `stream: true`, so the mock has to speak SSE; the gateway
/// denies the tool dispatch and surfaces the denial back into the turn as
/// a synthetic tool result; the runtime then continues the conversation by
/// calling the LLM again, at which point we want a terminating content
/// reply rather than another tool_call (which would loop).
///
/// wiremock's `up_to_n_times(1)` on the first matcher plus a fallback
/// matcher with no limit gives us the desired sequence without a custom
/// stateful responder.
pub async fn start_mock_llm_tool_call_then_content(
    tool_name: &str,
    args_json: &str,
    final_reply: &str,
) -> Result<(String, MockServer)> {
    let server = MockServer::start().await;
    let tool_call_body = build_tool_call_stream(tool_name, args_json);
    let content_body = build_sse_stream(final_reply);

    for p in ["/v1/chat/completions", "/chat/completions"] {
        // First call: tool_call.
        Mock::given(method("POST"))
            .and(path(p))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .insert_header("cache-control", "no-cache")
                    .set_body_string(tool_call_body.clone()),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        // Subsequent calls: content reply.
        Mock::given(method("POST"))
            .and(path(p))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .insert_header("cache-control", "no-cache")
                    .set_body_string(content_body.clone()),
            )
            .mount(&server)
            .await;
    }

    let url = server.uri();
    Ok((url, server))
}

/// Render an OpenAI-compat streaming completion that carries a single
/// `tool_calls` delta followed by a `finish_reason: tool_calls` terminator.
/// The runtime's SSE accumulator collects the tool_call across chunks, so
/// emitting id+name+arguments in one chunk is the shortest valid form.
fn build_tool_call_stream(tool_name: &str, args_json: &str) -> String {
    let first = json!({
        "id": "chatcmpl-sera-e2e-tc",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": "e2e-mock",
        "choices": [{
            "index": 0,
            "delta": {
                "role": "assistant",
                "tool_calls": [{
                    "index": 0,
                    "id": "call_sera_e2e_1",
                    "type": "function",
                    "function": {
                        "name": tool_name,
                        "arguments": args_json,
                    }
                }]
            },
            "finish_reason": null
        }]
    });
    let finish = json!({
        "id": "chatcmpl-sera-e2e-tc",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": "e2e-mock",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "tool_calls"
        }]
    });
    format!("data: {first}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
}

/// Render a minimal well-formed OpenAI-compat streaming completion that
/// carries `reply` in a single delta.  The runtime's `parse_sse_stream`
/// accumulates `delta.content` across all chunks and expects a terminal
/// `finish_reason` + `[DONE]` marker.
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
