//! A scripted OpenAI-compatible mock LLM for integration tests.
//!
//! Wraps `wiremock::MockServer` with the response shape `sera-runtime`'s
//! `LlmClient` expects.  Binds to an ephemeral loopback port so multiple
//! tests in the same process can each spin their own mock without
//! collision.
//!
//! The returned URL has no trailing slash and is ready to drop into
//! `LLM_BASE_URL` or into a Provider manifest's `base_url`.

use anyhow::Result;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Default reply text the mock returns when a test doesn't supply its own.
pub const DEFAULT_REPLY: &str = "hello from mock LLM";

/// Start a minimal mock LLM returning [`DEFAULT_REPLY`].  Convenience wrapper
/// around [`start_mock_llm_with_reply`] — most tests only need deterministic
/// output, not specific content.
pub async fn start_mock_llm() -> Result<String> {
    start_mock_llm_with_reply(DEFAULT_REPLY).await
}

/// Start a minimal OpenAI-compatible mock that always replies with `reply`.
///
/// The mock registers both `/v1/chat/completions` (the canonical OpenAI
/// path) and bare `/chat/completions` (what some compatible backends use)
/// so the runtime's LLM client can hit either shape.
///
/// The `MockServer` is leaked into a static so it outlives this function's
/// stack frame — otherwise its Drop would tear down the listener the moment
/// this returns and the gateway's first turn would get an `ECONNREFUSED`.
pub async fn start_mock_llm_with_reply(reply: &str) -> Result<String> {
    let server = MockServer::start().await;

    let body = json!({
        "id": "chatcmpl-sera-e2e",
        "object": "chat.completion",
        "created": 0,
        "model": "e2e-mock",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": reply
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 4,
            "completion_tokens": 4,
            "total_tokens": 8
        }
    });

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let url = server.uri();
    let _leaked: &'static MockServer = Box::leak(Box::new(server));
    Ok(url)
}
