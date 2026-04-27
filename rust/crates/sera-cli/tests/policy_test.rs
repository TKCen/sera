//! Integration tests for `sera policy` verbs.

use sera_cli::commands::policy::{
    PolicyListCommand, PolicyReloadCommand, PolicyShowCommand, PolicyValidateCommand,
};
use sera_commands::{Command, CommandArgs, CommandContext};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn args(pairs: &[(&str, &str)]) -> CommandArgs {
    let mut a = CommandArgs::new();
    for (k, v) in pairs {
        a.insert(*k, *v);
    }
    a
}

#[tokio::test]
async fn policy_list_returns_dir_and_names() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/admin/policies"))
        .and(header("authorization", "Bearer admin-tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "dir": "/etc/sera/policies",
            "policies": ["read-only", "shell-safe"]
        })))
        .mount(&server)
        .await;

    let result = PolicyListCommand
        .execute(
            args(&[("endpoint", &server.uri()), ("admin-token", "admin-tok")]),
            &CommandContext::new(),
        )
        .await
        .unwrap();

    let golden = serde_json::json!({
        "dir": "/etc/sera/policies",
        "policies": ["read-only", "shell-safe"]
    });
    assert_eq!(result.data, golden);
}

#[tokio::test]
async fn policy_show_pretty_prints_manifest() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/admin/policies/read-only"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "read-only",
            "manifest": {
                "apiVersion": "sera.dev/v1",
                "kind": "CapabilityPolicy",
                "metadata": {"name": "read-only"},
                "spec": {"allowedTools": ["memory_read"]}
            }
        })))
        .mount(&server)
        .await;

    let result = PolicyShowCommand
        .execute(
            args(&[
                ("endpoint", &server.uri()),
                ("admin-token", "t"),
                ("name", "read-only"),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.data["name"], "read-only");
    assert_eq!(
        result.data["manifest"]["kind"], "CapabilityPolicy",
        "manifest body must round-trip"
    );
}

#[tokio::test]
async fn policy_reload_returns_loaded_count() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/admin/policies/reload"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "loaded": 3,
            "policies": ["a", "b", "c"]
        })))
        .mount(&server)
        .await;

    let result = PolicyReloadCommand
        .execute(
            args(&[("endpoint", &server.uri()), ("admin-token", "t")]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.data["loaded"], 3);
}

#[tokio::test]
async fn policy_validate_marks_failure_with_nonzero_exit_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/admin/policies/validate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "dir": "/etc/sera/policies",
            "ok": false,
            "results": [
                {"file": "good", "ok": true},
                {"file": "broken", "ok": false, "error": "unexpected token"}
            ]
        })))
        .mount(&server)
        .await;

    let result = PolicyValidateCommand
        .execute(
            args(&[("endpoint", &server.uri()), ("admin-token", "t")]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1, "validate must fail when any file errors");
    assert_eq!(result.data["ok"], false);
}
