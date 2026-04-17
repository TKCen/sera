//! Integration tests for `sera auth` commands.
//!
//! Uses a [`MockTokenStore`] to avoid touching the OS keyring or filesystem,
//! and a [`wiremock`] server to stub `GET /api/auth/me`.

use std::sync::Arc;

use sera_cli::commands::auth::{LoginCommand, LogoutCommand, WhoamiCommand};
use sera_cli::token_store::{MockTokenStore, TokenStore};
use sera_commands::{Command, CommandArgs, CommandContext};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// Helper: run a command against a mock store injected via CommandArgs
// ---------------------------------------------------------------------------

/// Build a `CommandArgs` map from key-value pairs.
fn args(pairs: &[(&str, &str)]) -> CommandArgs {
    let mut a = CommandArgs::new();
    for (k, v) in pairs {
        a.insert(*k, *v);
    }
    a
}

// ---------------------------------------------------------------------------
// LoginCommand tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn login_stores_token_on_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/auth/me"))
        .and(header("authorization", "Bearer test-token-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sub": "test-user",
            "roles": ["operator"],
            "authenticated": true
        })))
        .mount(&server)
        .await;

    let store = Arc::new(MockTokenStore::new());
    let cmd = LoginCommand::with_store(store.clone());
    let a = args(&[
        ("endpoint", &server.uri()),
        ("token", "test-token-123"),
    ]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_ok(), "login should succeed: {:?}", result);
    assert_eq!(store.peek().as_deref(), Some("test-token-123"));
}

#[tokio::test]
async fn login_fails_on_401() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/auth/me"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let store = Arc::new(MockTokenStore::new());
    let cmd = LoginCommand::with_store(store.clone());
    let a = args(&[("endpoint", &server.uri()), ("token", "bad-token")]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("invalid API key") || err.contains("authentication failed"), "got: {err}");
    assert!(store.peek().is_none(), "token must not be stored on 401");
}

#[tokio::test]
async fn login_rejects_empty_token() {
    let store = Arc::new(MockTokenStore::new());
    let cmd = LoginCommand::with_store(store.clone());
    let a = args(&[("token", "  ")]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("empty"), "got: {err}");
}

#[tokio::test]
async fn login_fails_when_gateway_unreachable() {
    let store = Arc::new(MockTokenStore::new());
    let cmd = LoginCommand::with_store(store.clone());
    let a = args(&[
        ("endpoint", "http://127.0.0.1:19999"),
        ("token", "some-token"),
    ]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// WhoamiCommand tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn whoami_prints_principal_on_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/auth/me"))
        .and(header("authorization", "Bearer stored-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sub": "alice",
            "roles": ["admin", "operator"],
            "authenticated": true
        })))
        .mount(&server)
        .await;

    let store = Arc::new(MockTokenStore::new());
    store.save("stored-token").unwrap();

    let cmd = WhoamiCommand::with_store(store.clone());
    let a = args(&[("endpoint", &server.uri())]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_ok(), "whoami should succeed: {:?}", result);
    let data = result.unwrap().data;
    assert_eq!(data["sub"], "alice");
}

#[tokio::test]
async fn whoami_errors_when_not_logged_in() {
    let store = Arc::new(MockTokenStore::new());
    // store is empty
    let cmd = WhoamiCommand::with_store(store.clone());
    let a = args(&[("endpoint", "http://localhost:8080")]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("not logged in") || err.contains("login"), "got: {err}");
}

#[tokio::test]
async fn whoami_errors_on_401() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/auth/me"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let store = Arc::new(MockTokenStore::new());
    store.save("expired-token").unwrap();

    let cmd = WhoamiCommand::with_store(store.clone());
    let a = args(&[("endpoint", &server.uri())]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("login") || err.contains("rejected"), "got: {err}");
}

// ---------------------------------------------------------------------------
// LogoutCommand tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn logout_clears_stored_token() {
    let store = Arc::new(MockTokenStore::new());
    store.save("some-token").unwrap();
    assert!(store.peek().is_some());

    let cmd = LogoutCommand::with_store(store.clone());
    let result = cmd.execute(CommandArgs::new(), &CommandContext::new()).await;
    assert!(result.is_ok(), "logout should succeed: {:?}", result);
    assert!(store.peek().is_none(), "token should be cleared after logout");
}

#[tokio::test]
async fn logout_is_idempotent_when_not_logged_in() {
    let store = Arc::new(MockTokenStore::new());
    // store is already empty
    let cmd = LogoutCommand::with_store(store.clone());
    let result = cmd.execute(CommandArgs::new(), &CommandContext::new()).await;
    assert!(result.is_ok(), "logout on empty store should not error: {:?}", result);
}
