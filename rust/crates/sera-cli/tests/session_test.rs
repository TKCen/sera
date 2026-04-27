//! Integration tests for `sera session` verbs.

use sera_cli::commands::session::{
    SessionBrowseCommand, SessionKillCommand, SessionListCommand, SessionShowCommand,
};
use sera_commands::{Command, CommandArgs, CommandContext};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn args(pairs: &[(&str, &str)]) -> CommandArgs {
    let mut a = CommandArgs::new();
    for (k, v) in pairs {
        a.insert(*k, *v);
    }
    a
}

#[tokio::test]
async fn session_list_returns_array() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/admin/sessions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": "s-1", "agent_id": "a-1", "session_key": "k-1", "state": "running"},
            {"id": "s-2", "agent_id": "a-2", "session_key": "k-2", "state": "idle"}
        ])))
        .mount(&server)
        .await;

    let result = SessionListCommand
        .execute(
            args(&[("endpoint", &server.uri()), ("admin-token", "t")]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    let golden = serde_json::json!([
        {"id": "s-1", "agent_id": "a-1", "session_key": "k-1", "state": "running"},
        {"id": "s-2", "agent_id": "a-2", "session_key": "k-2", "state": "idle"}
    ]);
    assert_eq!(result.data, golden);
}

#[tokio::test]
async fn session_show_returns_record() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/admin/sessions/s-9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "s-9",
            "agent_id": "agent-x",
            "session_key": "k-9",
            "state": "running"
        })))
        .mount(&server)
        .await;

    let result = SessionShowCommand
        .execute(
            args(&[
                ("endpoint", &server.uri()),
                ("admin-token", "t"),
                ("id", "s-9"),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.data["id"], "s-9");
    assert_eq!(result.data["state"], "running");
}

#[tokio::test]
async fn session_kill_is_idempotent() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/admin/sessions/s-1"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let result = SessionKillCommand
        .execute(
            args(&[
                ("endpoint", &server.uri()),
                ("admin-token", "t"),
                ("id", "s-1"),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.data, serde_json::json!({"cancelled": "s-1"}));
}

#[tokio::test]
async fn session_browse_aliases_to_list_when_not_a_tty() {
    // Cargo's test harness pipes stdin/stdout, so the TTY check returns false
    // and `browse` should behave identically to `list`.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/admin/sessions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": "x", "agent_id": "y", "session_key": "z", "state": "running"}
        ])))
        .mount(&server)
        .await;

    let result = SessionBrowseCommand
        .execute(
            args(&[("endpoint", &server.uri()), ("admin-token", "t")]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    assert!(result.data.is_array());
    assert_eq!(result.data[0]["id"], "x");
}
