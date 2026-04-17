//! Integration tests for the `ping` subcommand using a wiremock server.

use sera_cli::commands::ping::do_ping;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Happy path: mock server returns `{"status":"ok"}`.
#[tokio::test]
async fn ping_ok_returns_status_and_latency() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": "ok"})))
        .mount(&server)
        .await;

    let (status, latency_ms) = do_ping(&server.uri()).await.unwrap();
    assert_eq!(status, "ok");
    // Latency should be a non-negative number (always true for u128, but
    // assert it's plausibly small for a loopback call).
    assert!(latency_ms < 5_000, "latency unexpectedly large: {latency_ms}ms");
}

/// Error path: mock server returns 503.
#[tokio::test]
async fn ping_fails_on_non_success_status() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(503).set_body_json(serde_json::json!({"status": "degraded"})))
        .mount(&server)
        .await;

    let result = do_ping(&server.uri()).await;
    assert!(result.is_err(), "expected error for 503 response");
}

/// Verify the registry wires up PingCommand correctly.
#[test]
fn registry_contains_ping() {
    let registry = sera_cli::build_registry();
    assert!(registry.get("ping").is_some(), "ping not found in registry");
    // Registry also contains auth:login, auth:whoami, auth:logout + agent:list, agent:show, agent:run
    assert_eq!(registry.len(), 7);
}
