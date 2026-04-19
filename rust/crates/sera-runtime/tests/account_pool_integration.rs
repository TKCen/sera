//! Integration tests for sera-jvi account pool failover behaviour in
//! [`sera_runtime::llm_client::LlmClient`].
//!
//! Each test spins up two wiremock servers (simulating two "accounts") and
//! attaches per-account base URLs. The pool round-robins across them; a 429
//! marks an account cooling-down so the next request transparently hits the
//! second account.

use std::sync::Arc;
use std::time::Duration;

use sera_models::{AccountPool, CooldownConfig, ProviderAccount};
use sera_runtime::llm_client::LlmClient;
use sera_runtime::types::ChatMessage;
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

fn chat_response_body() -> serde_json::Value {
    serde_json::json!({
        "id": "cmpl-1",
        "object": "chat.completion",
        "created": 0,
        "model": "m",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "hi"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    })
}

async fn setup_healthy_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_response_body()))
        .mount(&server)
        .await;
    server
}

async fn setup_rate_limited_server() -> MockServer {
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

/// Test config that disables the new sticky-fallback window so the legacy
/// integration tests continue to round-robin / fail-over predictably.
fn integ_cfg() -> CooldownConfig {
    CooldownConfig {
        sticky_fallback_window: Duration::ZERO,
        failure_threshold: 3,
        ..CooldownConfig::default()
    }
}

#[tokio::test]
async fn pool_fails_over_from_rate_limited_to_healthy() {
    let bad = setup_rate_limited_server().await;
    let good = setup_healthy_server().await;

    // Two accounts — account 0 points at the 429 server, account 1 at the
    // healthy one.
    let pool = Arc::new(AccountPool::new(
        "openai",
        vec![
            ProviderAccount::new("bad", "sk-bad", Some(bad.uri())),
            ProviderAccount::new("good", "sk-good", Some(good.uri())),
        ],
        integ_cfg(),
    ));

    // LlmClient's fallback base_url / api_key should never be used because
    // each account carries its own override.
    let client = LlmClient::with_params("http://unused.invalid", "m", None, 5_000)
        .with_account_pool(Arc::clone(&pool));

    // First non-streaming call: pool picks account 0 → 429 → marks it
    // cooling-down → caller sees RateLimited.
    let first = client.chat_non_streaming(&[user_msg("hi")], &[]).await;
    match first {
        Err(sera_runtime::llm_client::LlmError::RateLimited { .. }) => {}
        Err(other) => panic!("expected RateLimited, got {other:?}"),
        Ok(_) => panic!("first call should hit the 429 account"),
    }

    // Second non-streaming call: pool should skip the cooling-down account
    // and reach the healthy one.
    let ok = client
        .chat_non_streaming(&[user_msg("hi")], &[])
        .await
        .expect("second call should succeed via failover");
    assert_eq!(ok.message.role, "assistant");
    assert_eq!(ok.message.content.as_deref(), Some("hi"));
}

#[tokio::test]
async fn pool_exhausted_returns_provider_unavailable() {
    let bad1 = setup_rate_limited_server().await;
    let bad2 = setup_rate_limited_server().await;

    let pool = Arc::new(AccountPool::new(
        "openai",
        vec![
            ProviderAccount::new("b1", "sk-b1", Some(bad1.uri())),
            ProviderAccount::new("b2", "sk-b2", Some(bad2.uri())),
        ],
        // Long cooldown so we can't "expire" between the two hits.
        CooldownConfig {
            rate_limit_duration: Duration::from_secs(60),
            ..integ_cfg()
        },
    ));

    let client = LlmClient::with_params("http://unused.invalid", "m", None, 5_000)
        .with_account_pool(Arc::clone(&pool));

    // First hit → 429 on account 0
    let _ = client.chat_non_streaming(&[user_msg("a")], &[]).await;
    // Second hit → 429 on account 1
    let _ = client.chat_non_streaming(&[user_msg("b")], &[]).await;

    // Third hit → pool is exhausted → LlmError::RateLimited (sera-hjem) with
    // the soonest cooldown expiry attached so callers can sleep instead of
    // spinning on retries.
    let result = client.chat_non_streaming(&[user_msg("c")], &[]).await;
    match result {
        Err(sera_runtime::llm_client::LlmError::RateLimited { message, retry_after }) => {
            assert!(
                message.contains("rate-limited") || message.contains("unavailable"),
                "unexpected message: {message}"
            );
            assert!(
                retry_after.is_some(),
                "exhausted pool should report soonest expiry"
            );
        }
        Err(other) => panic!("expected RateLimited, got {other:?}"),
        Ok(_) => panic!("pool should be exhausted"),
    }
}

#[tokio::test]
async fn pool_success_does_not_affect_other_account_state() {
    let good1 = setup_healthy_server().await;
    let good2 = setup_healthy_server().await;

    let pool = Arc::new(AccountPool::new(
        "openai",
        vec![
            ProviderAccount::new("a", "sk-a", Some(good1.uri())),
            ProviderAccount::new("b", "sk-b", Some(good2.uri())),
        ],
        CooldownConfig::default(),
    ));

    let client = LlmClient::with_params("http://unused.invalid", "m", None, 5_000)
        .with_account_pool(Arc::clone(&pool));

    for _ in 0..4 {
        let ok = client
            .chat_non_streaming(&[user_msg("hi")], &[])
            .await
            .expect("success");
        assert_eq!(ok.message.role, "assistant");
    }

    for (_id, state) in pool.state_snapshot() {
        assert!(
            matches!(state, sera_models::AccountState::Available),
            "state should remain available after success"
        );
    }
}

#[tokio::test]
async fn single_account_fallback_when_no_pool() {
    // sera-jvi backwards-compat: no pool attached → uses
    // `with_params`-supplied base_url + api_key directly.
    let good = setup_healthy_server().await;

    let client = LlmClient::with_params(&good.uri(), "m", Some("sk-fallback"), 5_000);
    assert!(!client.has_account_pool());

    let ok = client
        .chat_non_streaming(&[user_msg("hi")], &[])
        .await
        .expect("fallback single-account path must still work");
    assert_eq!(ok.message.role, "assistant");
}
