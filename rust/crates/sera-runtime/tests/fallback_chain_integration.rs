//! Integration tests for [`sera_runtime::fallback_chain::FallbackChain`].
//!
//! Covers the sera-m1k8 acceptance criteria: MiniMax-style primary +
//! local-style fallback behaviour using wiremock servers as stand-ins for any
//! OpenAI-compatible endpoint. Tests assert both fallback (on retryable
//! upstream errors) and non-fallback (on request-bug / context-overflow
//! classes that should not be silently masked).

use sera_runtime::fallback_chain::FallbackChain;
use sera_runtime::llm_client::{LlmClient, LlmError};
use sera_runtime::types::{ChatMessage, FunctionDefinition, ToolDefinition};
use sera_types::tool::ToolUseBehavior;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn user_msg(content: &str) -> ChatMessage {
    ChatMessage {
        role: "user".to_string(),
        content: Some(content.to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }
}

fn make_tool(name: &str) -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".to_string(),
        function: FunctionDefinition {
            name: name.to_string(),
            description: format!("Tool {name}"),
            parameters: serde_json::json!({"type":"object","properties":{}}),
        },
    }
}

fn chat_response_body(content: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "cmpl-1",
        "object": "chat.completion",
        "created": 0,
        "model": "m",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7}
    })
}

fn tool_call_response_body() -> serde_json::Value {
    serde_json::json!({
        "id": "cmpl-tc",
        "object": "chat.completion",
        "created": 0,
        "model": "m",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_xyz",
                    "type": "function",
                    "function": {"name": "shell_exec", "arguments": "{\"command\":\"ls\"}"}
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 8, "completion_tokens": 3, "total_tokens": 11}
    })
}

async fn healthy_server(content: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_response_body(content)))
        .mount(&server)
        .await;
    server
}

async fn rate_limited_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_string(r#"{"error":{"message":"rate limit"}}"#),
        )
        .mount(&server)
        .await;
    server
}

async fn unavailable_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(503)
                .set_body_string(r#"{"error":{"message":"upstream down"}}"#),
        )
        .mount(&server)
        .await;
    server
}

async fn bad_request_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_string(r#"{"error":{"message":"invalid tool schema"}}"#),
        )
        .mount(&server)
        .await;
    server
}

async fn context_overflow_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_string(
            r#"{"error":{"code":"context_length_exceeded","message":"maximum context length is 8192 tokens"}}"#,
        ))
        .mount(&server)
        .await;
    server
}

async fn tool_call_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(tool_call_response_body()))
        .mount(&server)
        .await;
    server
}

// ---------------------------------------------------------------------------
// Primary-success path: no fallback consumed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn primary_success_does_not_touch_fallback() {
    // Primary returns 200 immediately. Fallback points at an invalid host so
    // any accidental fallback call would surface as a network error.
    let primary = healthy_server("from-minimax").await;

    let primary_client = LlmClient::with_params(&primary.uri(), "minimax-m2.1", Some("sk-mm"), 5_000);
    let fallback_client =
        LlmClient::with_params("http://unreachable.invalid:1", "qwen-local", Some("local"), 5_000);

    let chain = FallbackChain::new(vec![primary_client, fallback_client]);
    let result = chain
        .chat_non_streaming_with_behavior(&[user_msg("hi")], &[], &ToolUseBehavior::Auto)
        .await
        .expect("primary should succeed");

    assert_eq!(result.message.role, "assistant");
    assert_eq!(result.message.content.as_deref(), Some("from-minimax"));
    assert_eq!(result.prompt_tokens, 5);
    assert_eq!(result.completion_tokens, 2);
}

// ---------------------------------------------------------------------------
// Retryable-error fallback paths
// ---------------------------------------------------------------------------

#[tokio::test]
async fn primary_rate_limited_falls_back_to_local() {
    let primary = rate_limited_server().await;
    let fallback = healthy_server("from-local").await;

    let primary_client = LlmClient::with_params(&primary.uri(), "minimax-m2.1", Some("sk-mm"), 5_000);
    let fallback_client = LlmClient::with_params(&fallback.uri(), "qwen-local", Some("local"), 5_000);

    let chain = FallbackChain::new(vec![primary_client, fallback_client]);
    let result = chain
        .chat_non_streaming_with_behavior(&[user_msg("hi")], &[], &ToolUseBehavior::Auto)
        .await
        .expect("fallback should succeed after primary 429");

    assert_eq!(result.message.content.as_deref(), Some("from-local"));
}

#[tokio::test]
async fn primary_unavailable_falls_back_to_local() {
    let primary = unavailable_server().await;
    let fallback = healthy_server("from-local").await;

    let primary_client = LlmClient::with_params(&primary.uri(), "minimax-m2.1", Some("sk-mm"), 5_000);
    let fallback_client = LlmClient::with_params(&fallback.uri(), "qwen-local", Some("local"), 5_000);

    let chain = FallbackChain::new(vec![primary_client, fallback_client]);
    let result = chain
        .chat_non_streaming_with_behavior(&[user_msg("hi")], &[], &ToolUseBehavior::Auto)
        .await
        .expect("fallback should succeed after primary 503");

    assert_eq!(result.message.content.as_deref(), Some("from-local"));
}

#[tokio::test]
async fn primary_timeout_falls_back_to_local() {
    // Force a per-request timeout by setting timeout_ms very low and making
    // the primary delay longer than that.
    let primary = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(chat_response_body("late"))
                .set_delay(std::time::Duration::from_secs(2)),
        )
        .mount(&primary)
        .await;
    let fallback = healthy_server("from-local").await;

    // 100ms timeout → primary will always trip outer timeout before responding.
    let primary_client = LlmClient::with_params(&primary.uri(), "minimax-m2.1", Some("sk-mm"), 100);
    let fallback_client = LlmClient::with_params(&fallback.uri(), "qwen-local", Some("local"), 5_000);

    let chain = FallbackChain::new(vec![primary_client, fallback_client]);
    let result = chain
        .chat_non_streaming_with_behavior(&[user_msg("hi")], &[], &ToolUseBehavior::Auto)
        .await
        .expect("fallback should succeed after primary timeout");

    assert_eq!(result.message.content.as_deref(), Some("from-local"));
}

// ---------------------------------------------------------------------------
// Non-retryable: must NOT fall back (would mask real SERA bugs)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn primary_request_error_does_not_fall_back() {
    let primary = bad_request_server().await;
    // Fallback is healthy; if the chain incorrectly fell back, we'd see the
    // local response instead of the schema-bug error. This is exactly the
    // class of failure the operator wants surfaced loudly.
    let fallback = healthy_server("from-local").await;

    let primary_client = LlmClient::with_params(&primary.uri(), "minimax-m2.1", Some("sk-mm"), 5_000);
    let fallback_client = LlmClient::with_params(&fallback.uri(), "qwen-local", Some("local"), 5_000);

    let chain = FallbackChain::new(vec![primary_client, fallback_client]);
    match chain
        .chat_non_streaming_with_behavior(&[user_msg("hi")], &[], &ToolUseBehavior::Auto)
        .await
    {
        Err(LlmError::RequestError(msg)) => {
            assert!(
                msg.contains("HTTP 400") || msg.contains("invalid tool schema"),
                "expected request error from primary, got: {msg}"
            );
        }
        Err(other) => panic!("expected RequestError, got {other:?}"),
        Ok(_) => panic!("request-bug class must surface, not fall back"),
    }
}

#[tokio::test]
async fn primary_context_overflow_does_not_fall_back() {
    let primary = context_overflow_server().await;
    let fallback = healthy_server("from-local").await;

    let primary_client = LlmClient::with_params(&primary.uri(), "minimax-m2.1", Some("sk-mm"), 5_000);
    let fallback_client = LlmClient::with_params(&fallback.uri(), "qwen-local", Some("local"), 5_000);

    let chain = FallbackChain::new(vec![primary_client, fallback_client]);
    match chain
        .chat_non_streaming_with_behavior(&[user_msg("hi")], &[], &ToolUseBehavior::Auto)
        .await
    {
        Err(LlmError::ContextOverflow(_)) => {}
        Err(other) => panic!("expected ContextOverflow, got {other:?}"),
        Ok(_) => panic!("context overflow must surface, not fall back"),
    }
}

// ---------------------------------------------------------------------------
// Chain exhaustion: every provider failed → surface the last error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chain_exhausted_returns_final_error() {
    let primary = rate_limited_server().await;
    let fallback = unavailable_server().await;

    let primary_client = LlmClient::with_params(&primary.uri(), "minimax-m2.1", Some("sk-mm"), 5_000);
    let fallback_client = LlmClient::with_params(&fallback.uri(), "qwen-local", Some("local"), 5_000);

    let chain = FallbackChain::new(vec![primary_client, fallback_client]);
    match chain
        .chat_non_streaming_with_behavior(&[user_msg("hi")], &[], &ToolUseBehavior::Auto)
        .await
    {
        // The last provider returned 503 → ProviderUnavailable.
        Err(LlmError::ProviderUnavailable(_)) => {}
        Err(other) => panic!("expected ProviderUnavailable, got {other:?}"),
        Ok(_) => panic!("both providers failed; chain must surface an error"),
    }
}

// ---------------------------------------------------------------------------
// Tool-call semantics preserved through the chain
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tool_calls_preserved_through_fallback() {
    // Primary unavailable; fallback returns a tool_calls response. The chain
    // must propagate the tool_calls intact (empty content + valid tool_calls).
    let primary = unavailable_server().await;
    let fallback = tool_call_server().await;

    let primary_client = LlmClient::with_params(&primary.uri(), "minimax-m2.1", Some("sk-mm"), 5_000);
    let fallback_client = LlmClient::with_params(&fallback.uri(), "qwen-local", Some("local"), 5_000);

    let chain = FallbackChain::new(vec![primary_client, fallback_client]);
    let result = chain
        .chat_non_streaming_with_behavior(
            &[user_msg("run ls")],
            &[make_tool("shell_exec")],
            &ToolUseBehavior::Auto,
        )
        .await
        .expect("fallback should return tool_calls response");

    assert!(result.message.content.is_none(), "content should be null when tool_calls present");
    let tool_calls = result
        .message
        .tool_calls
        .expect("tool_calls must be preserved");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].id, "call_xyz");
    assert_eq!(tool_calls[0].function.name, "shell_exec");
}

// ---------------------------------------------------------------------------
// Single-provider chain behaves identically to a direct LlmClient call
// ---------------------------------------------------------------------------

#[tokio::test]
async fn single_provider_chain_propagates_errors_verbatim() {
    let server = bad_request_server().await;
    let client = LlmClient::with_params(&server.uri(), "minimax-m2.1", Some("sk-mm"), 5_000);

    let chain = FallbackChain::new(vec![client]);
    match chain
        .chat_non_streaming_with_behavior(&[user_msg("hi")], &[], &ToolUseBehavior::Auto)
        .await
    {
        Err(LlmError::RequestError(_)) => {}
        Err(other) => panic!("expected RequestError, got {other:?}"),
        Ok(_) => panic!("single-provider chain must propagate errors verbatim"),
    }
}
