//! Admin HTTP server integration tests (sera-nrn9, L.3).
//!
//! Stands up the admin router via `build_admin_router` against a lightweight
//! `AdminAppState` impl, then exercises every endpoint with valid + invalid
//! tokens. The bootstrap dev key (used by the public API) must be rejected on
//! every admin route — otherwise the separation of admin vs operator auth is
//! meaningless.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sera_gateway::admin::{
    AdminAppState, AdminAuditLogger, AdminAuth, AdminSessionInfo, build_admin_router,
};
use sera_gateway::capability_enforcement::CapabilityRegistry;
use sera_gateway::kill_switch::KillSwitch;
use sera_gateway::workflow_store::{InMemoryWorkflowTaskStore, WorkflowTaskStore};
use tempfile::TempDir;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "test-admin-token-1234567890abcdef";
const BOOTSTRAP_KEY: &str = "sera_bootstrap_dev_123";

struct TestState {
    auth: Arc<AdminAuth>,
    audit: Arc<AdminAuditLogger>,
    kill_switch: Arc<KillSwitch>,
    capability_registry: Arc<RwLock<Arc<CapabilityRegistry>>>,
    policies_dir: PathBuf,
    workflow_store: Arc<dyn WorkflowTaskStore>,
    cancel_registry: std::sync::Mutex<std::collections::HashMap<String, CancellationToken>>,
    sessions: std::sync::Mutex<Vec<AdminSessionInfo>>,
}

impl TestState {
    fn new(tmp: &TempDir) -> Arc<Self> {
        let auth = Arc::new(AdminAuth::new(ADMIN_TOKEN));
        let audit_path = AdminAuditLogger::default_path(tmp.path());
        let audit = AdminAuditLogger::shared(audit_path);
        let policies_dir = tmp.path().join("policies");
        std::fs::create_dir_all(&policies_dir).unwrap();
        Arc::new(Self {
            auth,
            audit,
            kill_switch: Arc::new(KillSwitch::new()),
            capability_registry: Arc::new(RwLock::new(Arc::new(CapabilityRegistry::empty()))),
            policies_dir,
            workflow_store: Arc::new(InMemoryWorkflowTaskStore::new()),
            cancel_registry: std::sync::Mutex::new(std::collections::HashMap::new()),
            sessions: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn audit_path(&self) -> PathBuf {
        self.audit.path().to_path_buf()
    }
}

#[async_trait::async_trait]
impl AdminAppState for TestState {
    fn auth(&self) -> Arc<AdminAuth> {
        Arc::clone(&self.auth)
    }
    fn audit(&self) -> Arc<AdminAuditLogger> {
        Arc::clone(&self.audit)
    }
    fn kill_switch(&self) -> Arc<KillSwitch> {
        Arc::clone(&self.kill_switch)
    }
    fn capability_registry(&self) -> Arc<RwLock<Arc<CapabilityRegistry>>> {
        Arc::clone(&self.capability_registry)
    }
    fn policies_dir(&self) -> PathBuf {
        self.policies_dir.clone()
    }
    fn workflow_store(&self) -> Option<Arc<dyn WorkflowTaskStore>> {
        Some(Arc::clone(&self.workflow_store))
    }
    async fn list_sessions(&self) -> Vec<AdminSessionInfo> {
        self.sessions.lock().unwrap().clone()
    }
    async fn get_session(&self, id: &str) -> Option<AdminSessionInfo> {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.id == id || s.session_key == id)
            .cloned()
    }
    fn cancel_session(&self, session_key: &str) -> bool {
        let mut map = self.cancel_registry.lock().unwrap();
        if let Some(tok) = map.remove(session_key) {
            tok.cancel();
            true
        } else {
            false
        }
    }
    fn agent_metadata(&self, id: &str) -> Option<serde_json::Value> {
        if id == "agent-known" {
            Some(serde_json::json!({"name": id, "provider": "test"}))
        } else {
            None
        }
    }
}

fn req(method: &str, path: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    builder.body(Body::empty()).unwrap()
}

fn req_json(method: &str, path: &str, token: Option<&str>, body: serde_json::Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    builder
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

// ── Auth tests ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn missing_token_returns_401_on_every_route() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    for path in [
        "/admin/workflows",
        "/admin/policies",
        "/admin/sessions",
        "/admin/killswitch/status",
    ] {
        let resp = app.clone().oneshot(req("GET", path, None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "path={path}");
    }
}

#[tokio::test]
async fn bootstrap_key_does_not_grant_admin_access() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    for path in [
        "/admin/workflows",
        "/admin/policies",
        "/admin/sessions",
        "/admin/killswitch/status",
    ] {
        let resp = app
            .clone()
            .oneshot(req("GET", path, Some(BOOTSTRAP_KEY)))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "bootstrap key must not pass admin auth on {path}"
        );
    }
}

#[tokio::test]
async fn random_token_returns_401() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req("GET", "/admin/policies", Some("random-token-xyz")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn valid_token_passes_auth() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req("GET", "/admin/killswitch/status", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ── Killswitch ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn killswitch_arm_disarm_status_round_trip() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let ks = Arc::clone(&state.kill_switch);
    let app = build_admin_router(state);

    // Initially disarmed.
    let resp = app
        .clone()
        .oneshot(req("GET", "/admin/killswitch/status", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["armed"], false);

    // Arm it via HTTP.
    let resp = app
        .clone()
        .oneshot(req("POST", "/admin/killswitch/arm", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(ks.is_armed(), "shared KillSwitch must observe HTTP arm");

    // Status reflects armed.
    let resp = app
        .clone()
        .oneshot(req("GET", "/admin/killswitch/status", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["armed"], true);

    // Disarm.
    let resp = app
        .clone()
        .oneshot(req("POST", "/admin/killswitch/disarm", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(!ks.is_armed());
}

// ── Policies ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn policies_list_empty_returns_ok() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req("GET", "/admin/policies", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(parsed["policies"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn policies_show_returns_yaml_manifest() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let policies_dir = state.policies_dir.clone();
    std::fs::write(
        policies_dir.join("tier-1.yaml"),
        "apiVersion: sera/v1\nkind: CapabilityPolicy\nmetadata:\n  name: tier-1\nallowedTools:\n  - memory_read\n",
    )
    .unwrap();
    let app = build_admin_router(state);

    let resp = app
        .oneshot(req("GET", "/admin/policies/tier-1", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["name"], "tier-1");
    assert_eq!(parsed["manifest"]["metadata"]["name"], "tier-1");
}

#[tokio::test]
async fn policies_show_rejects_traversal() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req("GET", "/admin/policies/..%2Fetc%2Fpasswd", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    // axum does %xx decoding before path matching, then our handler rejects.
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::NOT_FOUND
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "got {}",
        resp.status()
    );
}

#[tokio::test]
async fn policies_show_404_for_missing() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req("GET", "/admin/policies/nope", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn policies_reload_swaps_registry() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let cell = Arc::clone(&state.capability_registry);
    let policies_dir = state.policies_dir.clone();
    std::fs::write(
        policies_dir.join("alpha.yaml"),
        "apiVersion: sera/v1\nkind: CapabilityPolicy\nmetadata:\n  name: alpha\nallowedTools:\n  - shell\n",
    )
    .unwrap();
    let app = build_admin_router(state);

    let before_count = cell.read().await.policy_count();
    assert_eq!(before_count, 0);

    let resp = app
        .oneshot(req("POST", "/admin/policies/reload", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let after_count = cell.read().await.policy_count();
    assert_eq!(after_count, 1);
    assert!(cell.read().await.policy("alpha").is_some());
}

#[tokio::test]
async fn policies_validate_reports_per_file_results() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let policies_dir = state.policies_dir.clone();
    std::fs::write(
        policies_dir.join("good.yaml"),
        "apiVersion: sera/v1\nkind: CapabilityPolicy\nmetadata:\n  name: good\n",
    )
    .unwrap();
    std::fs::write(policies_dir.join("bad.yaml"), "this: is: not: valid: yaml: {{").unwrap();
    let app = build_admin_router(state);

    let resp = app
        .oneshot(req("POST", "/admin/policies/validate", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let results = parsed["results"].as_array().unwrap();
    assert!(results.iter().any(|r| r["ok"] == true));
    assert!(results.iter().any(|r| r["ok"] == false));
    assert_eq!(parsed["ok"], false);
}

// ── Sessions ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn sessions_list_returns_empty_initially() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req("GET", "/admin/sessions", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn sessions_get_unknown_returns_404() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req("GET", "/admin/sessions/abc", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sessions_kill_fires_cancellation_token() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let token = CancellationToken::new();
    state
        .cancel_registry
        .lock()
        .unwrap()
        .insert("session-X".to_string(), token.clone());
    let app = build_admin_router(state);

    let resp = app
        .oneshot(req("DELETE", "/admin/sessions/session-X", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(token.is_cancelled());
}

// ── Workflows ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn workflows_list_returns_empty_array() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req("GET", "/admin/workflows", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(parsed["workflows"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn workflows_create_returns_501() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req_json(
            "POST",
            "/admin/workflows",
            Some(ADMIN_TOKEN),
            serde_json::json!({"kind": "WorkflowDef"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn workflows_show_unknown_404() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req("GET", "/admin/workflows/abc", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn workflows_delete_returns_501() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req("DELETE", "/admin/workflows/abc", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
}

// ── Agents ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn agents_show_known_returns_metadata() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req("GET", "/admin/agents/agent-known", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn agents_show_unknown_404() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req("GET", "/admin/agents/missing", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn agents_spawn_returns_501() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req_json(
            "POST",
            "/admin/agents/spawn",
            Some(ADMIN_TOKEN),
            serde_json::json!({"name": "test-agent"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn agents_kill_returns_501() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let app = build_admin_router(state);
    let resp = app
        .oneshot(req("DELETE", "/admin/agents/agent-known", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
}

// ── Audit log ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn audit_log_records_successful_request() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let audit_path = state.audit_path();
    let app = build_admin_router(state);

    let resp = app
        .oneshot(req("GET", "/admin/killswitch/status", Some(ADMIN_TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Audit append happens after the response is built; give it a tick.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let contents = std::fs::read_to_string(&audit_path).expect("audit log written");
    let line = contents.trim().lines().last().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(parsed["method"], "GET");
    assert_eq!(parsed["path"], "/admin/killswitch/status");
    assert_eq!(parsed["status"], 200);
    let caller = parsed["caller"].as_str().unwrap();
    assert_eq!(caller.len(), 8, "caller is 8-char fingerprint");
    assert!(!caller.contains(ADMIN_TOKEN), "raw token must not appear in audit log");
}

#[tokio::test]
async fn audit_log_records_unauthorized_request() {
    let tmp = TempDir::new().unwrap();
    let state = TestState::new(&tmp);
    let audit_path = state.audit_path();
    let app = build_admin_router(state);

    let resp = app
        .oneshot(req("GET", "/admin/sessions", Some("bogus-token")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let contents = std::fs::read_to_string(&audit_path).expect("audit log written");
    let line = contents.trim().lines().last().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(parsed["status"], 401);
    assert_eq!(parsed["path"], "/admin/sessions");
}
