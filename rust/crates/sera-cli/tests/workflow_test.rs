//! Integration tests for `sera workflow` verbs.
//!
//! Each test stands up a wiremock server, points the AdminClient at it via
//! `--endpoint` + `--admin-token` overrides, and asserts the resulting
//! `CommandResult.data` against a small golden JSON snapshot.

use sera_cli::commands::workflow::{
    WorkflowCreateCommand, WorkflowDeleteCommand, WorkflowListCommand, WorkflowShowCommand,
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
async fn workflow_list_renders_table_and_returns_payload() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/admin/workflows"))
        .and(header("authorization", "Bearer test-admin"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "workflows": [
                {"id": "wf-1", "agent_id": "agent-a", "status": "running", "title": "Recap"},
                {"id": "wf-2", "agent_id": "agent-b", "status": "blocked", "title": "Audit"}
            ]
        })))
        .mount(&server)
        .await;

    let cmd = WorkflowListCommand;
    let result = cmd
        .execute(
            args(&[("endpoint", &server.uri()), ("admin-token", "test-admin")]),
            &CommandContext::new(),
        )
        .await
        .unwrap();

    let golden = serde_json::json!({
        "workflows": [
            {"id": "wf-1", "agent_id": "agent-a", "status": "running", "title": "Recap"},
            {"id": "wf-2", "agent_id": "agent-b", "status": "blocked", "title": "Audit"}
        ]
    });
    assert_eq!(result.data, golden);
}

#[tokio::test]
async fn workflow_show_returns_record() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/admin/workflows/wf-99"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "wf-99",
            "agent_id": "agent-x",
            "status": "running",
            "title": "Inspect",
            "await_type": null,
            "resolved_at": null,
            "resume_token": "tok"
        })))
        .mount(&server)
        .await;

    let result = WorkflowShowCommand
        .execute(
            args(&[
                ("endpoint", &server.uri()),
                ("admin-token", "t"),
                ("id", "wf-99"),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.data["id"], "wf-99");
    assert_eq!(result.data["status"], "running");
}

#[tokio::test]
async fn workflow_create_surfaces_l5_not_yet_wired_on_501() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/admin/workflows"))
        .respond_with(ResponseTemplate::new(501).set_body_json(serde_json::json!({
            "error": "workflow_store_not_ready",
            "detail": "admin workflow CRUD is gated on L.5 wiring"
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("wf.yaml");
    std::fs::write(&manifest_path, "name: test\n").unwrap();

    let err = WorkflowCreateCommand
        .execute(
            args(&[
                ("endpoint", &server.uri()),
                ("admin-token", "t"),
                ("file", manifest_path.to_str().unwrap()),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("L.5"), "expected L.5 marker, got: {err}");
}

#[tokio::test]
async fn workflow_delete_surfaces_l5_not_yet_wired_on_501() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/admin/workflows/wf-1"))
        .respond_with(ResponseTemplate::new(501).set_body_json(serde_json::json!({
            "error": "workflow_store_not_ready"
        })))
        .mount(&server)
        .await;

    let err = WorkflowDeleteCommand
        .execute(
            args(&[
                ("endpoint", &server.uri()),
                ("admin-token", "t"),
                ("id", "wf-1"),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("L.5"), "expected L.5 marker, got: {err}");
}
