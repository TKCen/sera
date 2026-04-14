//! Integration tests for the mock LM Studio fixture.

mod fixtures;

use fixtures::mock_lm_studio::start_mock_lm_studio;

#[tokio::test]
async fn mock_serves_models_and_chat() {
    let h = start_mock_lm_studio().await;
    let base = h.base_url();

    // GET /v1/models
    let resp = reqwest::get(format!("{base}/v1/models"))
        .await
        .expect("GET /v1/models");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("parse JSON");
    assert_eq!(body["object"], "list");
    assert_eq!(body["data"][0]["id"], "mock-model");

    // POST /v1/chat/completions — non-streaming
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/v1/chat/completions"))
        .json(&serde_json::json!({
            "model": "mock-model",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .expect("POST chat non-streaming");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("parse JSON");
    assert_eq!(body["choices"][0]["message"]["role"], "assistant");

    // POST /v1/chat/completions — 429 via query param
    let resp = client
        .post(format!("{base}/v1/chat/completions?force_error=rate_limit"))
        .json(&serde_json::json!({
            "model": "mock-model",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .expect("POST rate limit");
    assert_eq!(resp.status(), 429);
    let body: serde_json::Value = resp.json().await.expect("parse JSON");
    assert_eq!(body["error"]["type"], "rate_limit_error");

    // POST /v1/chat/completions — 429 via header
    let resp = client
        .post(format!("{base}/v1/chat/completions"))
        .header("X-Mock-Error", "rate_limit")
        .json(&serde_json::json!({
            "model": "mock-model",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .expect("POST rate limit via header");
    assert_eq!(resp.status(), 429);
}

#[tokio::test]
async fn mock_serves_streaming_chat() {
    let h = start_mock_lm_studio().await;
    let base = h.base_url();

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/v1/chat/completions"))
        .json(&serde_json::json!({
            "model": "mock-model",
            "stream": true,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .expect("POST chat streaming");
    assert_eq!(resp.status(), 200);

    let text = resp.text().await.expect("read body");
    assert!(text.contains("Hello from mock"), "body: {text}");
    assert!(text.contains("[DONE]"), "body: {text}");
}
