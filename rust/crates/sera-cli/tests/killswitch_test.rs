//! Integration tests for `sera killswitch` verbs.

use sera_cli::commands::killswitch::{
    KillswitchArmCommand, KillswitchDisarmCommand, KillswitchStatusCommand,
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
async fn killswitch_arm_returns_ok_payload() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/admin/killswitch/arm"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let result = KillswitchArmCommand
        .execute(
            args(&[("endpoint", &server.uri()), ("admin-token", "t")]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.data, serde_json::json!({"ok": true}));
}

#[tokio::test]
async fn killswitch_disarm_returns_ok_payload() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/admin/killswitch/disarm"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let result = KillswitchDisarmCommand
        .execute(
            args(&[("endpoint", &server.uri()), ("admin-token", "t")]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.data, serde_json::json!({"ok": true}));
}

#[tokio::test]
async fn killswitch_status_reflects_armed_flag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/admin/killswitch/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "armed": true
        })))
        .mount(&server)
        .await;

    let result = KillswitchStatusCommand
        .execute(
            args(&[("endpoint", &server.uri()), ("admin-token", "t")]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.data, serde_json::json!({"armed": true}));
}
