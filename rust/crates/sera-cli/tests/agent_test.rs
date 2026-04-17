//! Integration tests for `sera agent` commands.
//!
//! Uses [`MockTokenStore`] to avoid touching the OS keyring, and a
//! [`wiremock`] server to stub the gateway endpoints.

use std::sync::Arc;

use sera_cli::commands::agent::{AgentListCommand, AgentRunCommand, AgentShowCommand};
use sera_cli::token_store::{MockTokenStore, TokenStore};
use sera_commands::{Command, CommandArgs, CommandContext};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn args(pairs: &[(&str, &str)]) -> CommandArgs {
    let mut a = CommandArgs::new();
    for (k, v) in pairs {
        a.insert(*k, *v);
    }
    a
}

fn seeded_store(token: &str) -> Arc<MockTokenStore> {
    let store = Arc::new(MockTokenStore::new());
    store.save(token).unwrap();
    store
}

// ---------------------------------------------------------------------------
// agent list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_list_returns_table_with_agents() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/agents"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "id": "agent-1",
                "name": "alpha",
                "template_ref": "claude-opus",
                "status": "running",
                "circle": null,
                "display_name": "Alpha Agent",
                "lifecycle_mode": "persistent",
                "workspace_path": "/workspaces/alpha",
                "container_id": "cnt-abc",
                "created_at": "2026-04-01T00:00:00Z",
                "updated_at": "2026-04-01T00:00:00Z"
            },
            {
                "id": "agent-2",
                "name": "beta",
                "template_ref": "claude-haiku",
                "status": "stopped",
                "circle": "engineering",
                "display_name": "Beta Agent",
                "lifecycle_mode": "ephemeral",
                "workspace_path": "/workspaces/beta",
                "container_id": null,
                "created_at": "2026-04-02T00:00:00Z",
                "updated_at": "2026-04-02T00:00:00Z"
            }
        ])))
        .mount(&server)
        .await;

    let store = seeded_store("test-token");
    let cmd = AgentListCommand::with_store(store);
    let a = args(&[("endpoint", &server.uri())]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_ok(), "agent list should succeed: {:?}", result);

    let data = result.unwrap().data;
    let arr = data.as_array().expect("data should be array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["name"], "alpha");
    assert_eq!(arr[1]["name"], "beta");
}

#[tokio::test]
async fn agent_list_json_flag_returns_raw_data() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/agents"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "id": "agent-1",
                "name": "alpha",
                "template_ref": "claude-opus",
                "status": "running",
                "circle": null
            }
        ])))
        .mount(&server)
        .await;

    let store = seeded_store("test-token");
    let cmd = AgentListCommand::with_store(store);
    let a = args(&[("endpoint", &server.uri()), ("json", "true")]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_ok(), "agent list --json should succeed: {:?}", result);

    let data = result.unwrap().data;
    assert!(data.is_array());
    assert_eq!(data[0]["id"], "agent-1");
}

#[tokio::test]
async fn agent_list_returns_exit_code_2_without_token() {
    let store = Arc::new(MockTokenStore::new()); // empty store
    let cmd = AgentListCommand::with_store(store);
    let a = args(&[("endpoint", "http://localhost:8080")]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_err(), "should error when no token");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("login") || err.contains("authenticated"),
        "got: {err}"
    );
}

#[tokio::test]
async fn agent_list_401_gives_meaningful_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/agents"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let store = seeded_store("bad-token");
    let cmd = AgentListCommand::with_store(store);
    let a = args(&[("endpoint", &server.uri())]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("login") || err.contains("rejected"), "got: {err}");
}

// ---------------------------------------------------------------------------
// agent show
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_show_prints_detail_fields() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/agents/agent-abc"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "agent-abc",
            "name": "gamma",
            "display_name": "Gamma Agent",
            "template_ref": "claude-opus",
            "status": "running",
            "lifecycle_mode": "persistent",
            "circle": "research",
            "workspace_path": "/workspaces/gamma",
            "container_id": "cnt-xyz",
            "last_heartbeat_at": "2026-04-17T10:00:00Z",
            "created_at": "2026-04-01T00:00:00Z",
            "updated_at": "2026-04-17T10:00:00Z",
            "resolved_config": null,
            "resolved_capabilities": null,
            "overrides": null
        })))
        .mount(&server)
        .await;

    let store = seeded_store("test-token");
    let cmd = AgentShowCommand::with_store(store);
    let a = args(&[("endpoint", &server.uri()), ("id", "agent-abc")]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_ok(), "agent show should succeed: {:?}", result);

    let data = result.unwrap().data;
    assert_eq!(data["id"], "agent-abc");
    assert_eq!(data["name"], "gamma");
    assert_eq!(data["status"], "running");
    assert_eq!(data["template_ref"], "claude-opus");
}

#[tokio::test]
async fn agent_show_json_flag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/agents/agent-abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "agent-abc",
            "name": "gamma",
            "template_ref": "claude-opus",
            "status": "running"
        })))
        .mount(&server)
        .await;

    let store = seeded_store("test-token");
    let cmd = AgentShowCommand::with_store(store);
    let a = args(&[
        ("endpoint", &server.uri()),
        ("id", "agent-abc"),
        ("json", "true"),
    ]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_ok(), "agent show --json should succeed: {:?}", result);
    assert_eq!(result.unwrap().data["id"], "agent-abc");
}

#[tokio::test]
async fn agent_show_404_returns_not_found_exit_code() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/agents/missing-id"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let store = seeded_store("test-token");
    let cmd = AgentShowCommand::with_store(store);
    let a = args(&[("endpoint", &server.uri()), ("id", "missing-id")]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    // Should succeed (no Err) but with a non-zero exit code
    assert!(result.is_ok(), "show on 404 should not panic: {:?}", result);
    assert_ne!(
        result.unwrap().exit_code,
        0,
        "exit code should be non-zero for 404"
    );
}

#[tokio::test]
async fn agent_show_exit_code_2_without_token() {
    let store = Arc::new(MockTokenStore::new());
    let cmd = AgentShowCommand::with_store(store);
    let a = args(&[("endpoint", "http://localhost:8080"), ("id", "some-id")]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("login") || err.contains("authenticated"),
        "got: {err}"
    );
}

// ---------------------------------------------------------------------------
// agent run
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_run_posts_turn_and_returns_reply() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "reply": "Hello from alpha!",
            "thought": null,
            "thoughts": [],
            "citations": [],
            "session_id": "sess-001",
            "message_id": null,
            "usage": null
        })))
        .mount(&server)
        .await;

    let store = seeded_store("test-token");
    let cmd = AgentRunCommand::with_store(store);
    let a = args(&[
        ("endpoint", &server.uri()),
        ("id", "agent-1"),
        ("prompt", "say hello"),
    ]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_ok(), "agent run should succeed: {:?}", result);

    let data = result.unwrap().data;
    assert_eq!(data["reply"], "Hello from alpha!");
    assert_eq!(data["session_id"], "sess-001");
}

#[tokio::test]
async fn agent_run_with_thoughts_renders_tool_calls() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "reply": "Done.",
            "thought": null,
            "thoughts": [
                {"step": "bash", "content": "ls /workspace"},
                {"step": "observation", "content": "file1.txt file2.txt"}
            ],
            "citations": [],
            "session_id": "sess-002",
            "message_id": null,
            "usage": {"promptTokens": 100, "completionTokens": 50, "totalTokens": 150}
        })))
        .mount(&server)
        .await;

    let store = seeded_store("test-token");
    let cmd = AgentRunCommand::with_store(store);
    let a = args(&[
        ("endpoint", &server.uri()),
        ("id", "agent-1"),
        ("prompt", "list files"),
    ]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_ok(), "agent run with thoughts should succeed: {:?}", result);

    let data = result.unwrap().data;
    let thoughts = data["thoughts"].as_array().unwrap();
    assert_eq!(thoughts.len(), 2);
    assert_eq!(thoughts[0]["step"], "bash");
}

#[tokio::test]
async fn agent_run_raw_flag_returns_full_json() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "reply": "raw reply",
            "thought": null,
            "thoughts": [],
            "citations": [],
            "session_id": "sess-raw",
            "message_id": null,
            "usage": null
        })))
        .mount(&server)
        .await;

    let store = seeded_store("test-token");
    let cmd = AgentRunCommand::with_store(store);
    let a = args(&[
        ("endpoint", &server.uri()),
        ("id", "agent-1"),
        ("prompt", "test"),
        ("raw", "true"),
    ]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_ok(), "agent run --raw should succeed: {:?}", result);
    assert_eq!(result.unwrap().data["reply"], "raw reply");
}

#[tokio::test]
async fn agent_run_exit_code_2_without_token() {
    let store = Arc::new(MockTokenStore::new());
    let cmd = AgentRunCommand::with_store(store);
    let a = args(&[
        ("endpoint", "http://localhost:8080"),
        ("id", "agent-1"),
        ("prompt", "hello"),
    ]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("login") || err.contains("authenticated"),
        "got: {err}"
    );
}

#[tokio::test]
async fn agent_run_401_gives_meaningful_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let store = seeded_store("expired-token");
    let cmd = AgentRunCommand::with_store(store);
    let a = args(&[
        ("endpoint", &server.uri()),
        ("id", "agent-1"),
        ("prompt", "hello"),
    ]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("login") || err.contains("rejected"), "got: {err}");
}

// ---------------------------------------------------------------------------
// Autonomous gateway shape tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_list_autonomous_shape_renders_table() {
    // The autonomous gateway returns {name, provider, model, has_tools} — no id/template_ref/status.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/agents"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "name": "sera",
                "provider": "lm-studio",
                "model": "qwen/qwen3.6-35b-a3b",
                "has_tools": true
            }
        ])))
        .mount(&server)
        .await;

    let store = seeded_store("test-token");
    let cmd = AgentListCommand::with_store(store);
    let a = args(&[("endpoint", &server.uri())]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_ok(), "agent list (autonomous shape) should succeed: {:?}", result);

    let data = result.unwrap().data;
    let arr = data.as_array().expect("data should be array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "sera");
    assert_eq!(arr[0]["provider"], "lm-studio");
    assert_eq!(arr[0]["has_tools"], true);
}

#[tokio::test]
async fn agent_show_autonomous_shape_renders_detail() {
    // GET /api/agents/sera returns autonomous shape.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/agents/sera"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "sera",
            "provider": "lm-studio",
            "model": "qwen/qwen3.6-35b-a3b",
            "has_tools": true
        })))
        .mount(&server)
        .await;

    let store = seeded_store("test-token");
    let cmd = AgentShowCommand::with_store(store);
    let a = args(&[("endpoint", &server.uri()), ("id", "sera")]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_ok(), "agent show (autonomous shape) should succeed: {:?}", result);

    let data = result.unwrap().data;
    assert_eq!(data["name"], "sera");
    assert_eq!(data["provider"], "lm-studio");
    assert_eq!(data["has_tools"], true);
}

#[tokio::test]
async fn agent_run_autonomous_response_shape() {
    // Autonomous gateway returns {response, session_id, usage} — not {reply, ...}.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "response": "Hello from autonomous mode!",
            "session_id": "sess-auto-001",
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        })))
        .mount(&server)
        .await;

    let store = seeded_store("test-token");
    let cmd = AgentRunCommand::with_store(store);
    let a = args(&[
        ("endpoint", &server.uri()),
        ("id", "sera"),
        ("prompt", "hello"),
    ]);
    let result = cmd.execute(a, &CommandContext::new()).await;
    assert!(result.is_ok(), "agent run (autonomous response shape) should succeed: {:?}", result);

    let data = result.unwrap().data;
    assert_eq!(data["response"], "Hello from autonomous mode!");
    assert_eq!(data["session_id"], "sess-auto-001");
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

#[test]
fn registry_contains_agent_commands() {
    let registry = sera_cli::build_registry();
    assert!(registry.get("agent:list").is_some(), "agent:list not in registry");
    assert!(registry.get("agent:show").is_some(), "agent:show not in registry");
    assert!(registry.get("agent:run").is_some(), "agent:run not in registry");
    // Total: 4 original + 3 agent = 7
    assert_eq!(registry.len(), 7);
}
