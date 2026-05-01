//! Acceptance tests for the gateway-side audit-row half of `sera-eq0m`.
//!
//! `sera-eq0m`'s full acceptance contract has two halves:
//!
//! * Kernel-layer egress denial — a strict-sandbox harness that dials
//!   `api.anthropic.com` directly is refused at the network namespace, while
//!   the same harness can reach `inference.local`. Already proven by the
//!   Docker-bridge tests landed under `sera-k6qn` (PR #1136) in
//!   `rust/crates/sera-tools/tests/docker_egress_acceptance.rs`.
//! * Audit-row persistence — the gateway writes exactly one row per proxied
//!   call and writes zero rows for direct provider dials that bypass the
//!   gateway. This file covers that half (`sera-eq0m.1`).
//!
//! These tests stand up the **production** audit sink
//! (`SqliteInferenceProxyAudit`) against a real `SqliteDb`, drive the
//! production route handlers (`chat_completions` / `chat_messages`) over a
//! real loopback TCP listener, and translate `inference.local` URLs through
//! the production rewriter (`InferenceLocalResolver`). No row is ever
//! inserted directly into `audit_log`; every assertion is the consequence of
//! exercising the route code path that production runs.
//!
//! ## CI safety
//!
//! - Pure in-process: no docker, no firewall, no host-network mutation.
//! - Each test owns its in-memory SQLite, gateway listener, and mock-provider
//!   listener; tests do not share state and can run in parallel.
//! - No env-var manipulation — the "stale provider key" threat is modelled
//!   as a smuggled `Authorization` / `x-api-key` header, which is what would
//!   actually arrive at the gateway from a misbehaving harness.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Router,
    body::Bytes,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use sera_db::sqlite::{AuditRow, SqliteDb};
use sera_gateway::routes::inference_proxy::{
    InferenceProxyAppState, InferenceProxyAudit, LlmBudgetGate, NoopBudgetGate,
    SqliteInferenceProxyAudit, UpstreamProvider, chat_completions, chat_messages,
};
use sera_tools::inference_local::InferenceLocalResolver;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::{Mutex as TokioMutex, oneshot};

// ---------------------------------------------------------------------------
// Production-shaped AppState
// ---------------------------------------------------------------------------
//
// Mirrors the binary's wiring (`AppState` in `bin/sera.rs`) for the fields
// the proxy route consumes: optional bearer key, OpenAI/Anthropic upstream
// providers, shared reqwest client, real budget gate, real audit sink. Body
// content is never persisted — only the `ProxyAuditEvent` metadata.

struct AcceptanceState {
    api_key: Option<String>,
    upstream: Option<UpstreamProvider>,
    anthropic_upstream: Option<UpstreamProvider>,
    client: reqwest::Client,
    gate: Arc<dyn LlmBudgetGate>,
    audit: Arc<dyn InferenceProxyAudit>,
}

impl InferenceProxyAppState for AcceptanceState {
    fn proxy_api_key(&self) -> &Option<String> {
        &self.api_key
    }
    fn proxy_upstream(&self) -> Option<UpstreamProvider> {
        self.upstream.clone()
    }
    fn proxy_anthropic_upstream(&self) -> Option<UpstreamProvider> {
        self.anthropic_upstream.clone()
    }
    fn proxy_http_client(&self) -> reqwest::Client {
        self.client.clone()
    }
    fn proxy_budget_gate(&self) -> Arc<dyn LlmBudgetGate> {
        Arc::clone(&self.gate)
    }
    fn proxy_audit(&self) -> Arc<dyn InferenceProxyAudit> {
        Arc::clone(&self.audit)
    }
}

// ---------------------------------------------------------------------------
// Mock provider — stands in for api.openai.com / api.anthropic.com
// ---------------------------------------------------------------------------

struct MockProvider {
    base_url: String,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Drop for MockProvider {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

async fn start_mock_provider() -> MockProvider {
    let openai_handler = |_h: HeaderMap, _b: Bytes| async move {
        (
            StatusCode::OK,
            axum::Json(serde_json::json!({
                "id": "chatcmpl-mock-1",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })),
        )
            .into_response()
    };
    let anthropic_handler = |_h: HeaderMap, _b: Bytes| async move {
        (
            StatusCode::OK,
            axum::Json(serde_json::json!({
                "id": "msg_mock_1",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "text", "text": "ok"}],
                "model": "claude-3-5-sonnet"
            })),
        )
            .into_response()
    };
    let app: Router = Router::new()
        .route("/v1/chat/completions", post(openai_handler))
        .route("/v1/messages", post(anthropic_handler));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await
            .ok();
    });

    MockProvider {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        shutdown: Some(tx),
    }
}

// ---------------------------------------------------------------------------
// Gateway under test
// ---------------------------------------------------------------------------

struct GatewayUnderTest {
    addr: String,
    db: Arc<TokioMutex<SqliteDb>>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Drop for GatewayUnderTest {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

/// Stand up a gateway HTTP server backed by the production audit sink and a
/// fresh in-memory SQLite. Returns the listener URL plus a handle to the DB
/// so tests can read `audit_log` directly.
async fn start_gateway(
    api_key: Option<&str>,
    openai_upstream: Option<UpstreamProvider>,
    anthropic_upstream: Option<UpstreamProvider>,
) -> GatewayUnderTest {
    let db = Arc::new(TokioMutex::new(
        SqliteDb::open_in_memory().expect("in-memory sqlite"),
    ));
    let audit: Arc<dyn InferenceProxyAudit> =
        Arc::new(SqliteInferenceProxyAudit::new(Arc::clone(&db)));
    let state = Arc::new(AcceptanceState {
        api_key: api_key.map(String::from),
        upstream: openai_upstream,
        anthropic_upstream,
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client"),
        gate: Arc::new(NoopBudgetGate),
        audit,
    });

    let app = Router::new()
        .route(
            "/v1/chat/completions",
            post(chat_completions::<AcceptanceState>),
        )
        .route("/v1/messages", post(chat_messages::<AcceptanceState>))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind gateway");
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await
            .ok();
    });

    GatewayUnderTest {
        addr: format!("http://{addr}"),
        db,
        shutdown: Some(tx),
    }
}

async fn read_audit_rows(db: &Arc<TokioMutex<SqliteDb>>) -> Vec<AuditRow> {
    db.lock().await.query_audit(50).expect("query audit")
}

fn proxy_rows(rows: &[AuditRow]) -> Vec<&AuditRow> {
    rows.iter()
        .filter(|r| r.event_type == "inference_proxy_call")
        .collect()
}

fn parse_details(row: &AuditRow) -> Value {
    let raw = row
        .details
        .as_deref()
        .expect("inference_proxy_call rows always carry a details JSON");
    serde_json::from_str(raw).expect("details is valid JSON")
}

// ---------------------------------------------------------------------------
// Acceptance test 1 — proxy call via inference.local emits one audit row
// ---------------------------------------------------------------------------
//
// Maps eq0m criterion (2): "inference.local proxy call succeeds and creates
// exactly one audit row for the harness principal/model/provider/outcome."
//
// Drives the request through the production `InferenceLocalResolver` so the
// rewrite step is exercised end-to-end. With auth disabled (`api_key=None`),
// the gateway records the principal as the literal `"anonymous"` (per
// `check_auth`'s fallback); test asserts on that value to match what
// production records, not a hypothetical `"harness"` placeholder.

#[tokio::test]
async fn proxy_call_via_inference_local_emits_exactly_one_audit_row() {
    let mock = start_mock_provider().await;
    let gw = start_gateway(
        None,
        Some(UpstreamProvider {
            base_url: mock.base_url.clone(),
            api_key: "upstream-openai-key".into(),
        }),
        Some(UpstreamProvider {
            base_url: mock.base_url.clone(),
            api_key: "upstream-anthropic-key".into(),
        }),
    )
    .await;

    let resolver = InferenceLocalResolver::new(gw.addr.clone());
    let url = resolver.rewrite("https://inference.local/v1/chat/completions");
    assert!(
        url.starts_with(&gw.addr),
        "resolver must rewrite inference.local to the gateway endpoint; got {url}"
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let resp = client
        .post(&url)
        .header(header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(&serde_json::json!({"model": "gpt-4-mini"})).unwrap())
        .send()
        .await
        .expect("proxy request");
    assert_eq!(resp.status(), StatusCode::OK);

    let rows = read_audit_rows(&gw.db).await;
    let proxy = proxy_rows(&rows);
    assert_eq!(
        proxy.len(),
        1,
        "exactly one inference_proxy_call row expected; got {} rows: {rows:?}",
        proxy.len()
    );
    let row = proxy[0];
    assert_eq!(row.actor_kind, "inference_proxy");
    assert_eq!(
        row.actor_id, "anonymous",
        "auth-disabled mode records `anonymous` per check_auth fallback"
    );
    let details = parse_details(row);
    assert_eq!(details["outcome"], "success");
    assert_eq!(details["status"], 200);
    assert_eq!(details["model_requested"], "gpt-4-mini");
    assert_eq!(details["stream"], false);
    assert_eq!(details["provider_base_url"], mock.base_url);
}

// ---------------------------------------------------------------------------
// Acceptance test 2 — direct provider dial does not emit an audit row
// ---------------------------------------------------------------------------
//
// Maps eq0m criterion (1): "Direct api.anthropic.com / api.openai.com dial
// from a strict sandbox is denied and does not create an inference_proxy_call
// audit row."
//
// The kernel-layer "denied" half is covered by sera-k6qn / PR #1136 (Docker
// `--internal` bridge). This test asserts the second half: when something
// dials the provider directly (bypassing the gateway), the gateway's
// `audit_log` stays empty for `inference_proxy_call`. Only the gateway can
// write that row, so a successful bypass is observable as an audit gap —
// which is exactly the compliance hazard the boundary closes.

#[tokio::test]
async fn direct_provider_dial_does_not_emit_audit_row() {
    let mock = start_mock_provider().await;
    let gw = start_gateway(
        None,
        Some(UpstreamProvider {
            base_url: mock.base_url.clone(),
            api_key: "upstream-openai-key".into(),
        }),
        None,
    )
    .await;

    // Bypass the gateway entirely — dial the mock provider directly. This is
    // what a misbehaving harness with a stale provider key would do if the
    // sandbox netns *failed* to deny the dial.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let direct_url = format!("{}/chat/completions", mock.base_url);
    let resp = client
        .post(&direct_url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, "Bearer sk-leaked-stale-direct-1")
        .body(serde_json::to_vec(&serde_json::json!({"model": "gpt-4"})).unwrap())
        .send()
        .await
        .expect("direct provider request");
    assert_eq!(resp.status(), StatusCode::OK);

    let rows = read_audit_rows(&gw.db).await;
    let proxy = proxy_rows(&rows);
    assert!(
        proxy.is_empty(),
        "direct provider dials must not produce inference_proxy_call rows; \
         the gateway never saw the request, so any row would mean a row was \
         fabricated outside the route's normal write path. got: {rows:?}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance test 3 — stale provider keys do not create bypass audit rows
// ---------------------------------------------------------------------------
//
// Maps eq0m criterion (3): "Stale provider API keys in harness env do not
// create bypass audit records."
//
// The "harness env" is the calling environment, not the gateway's. A stale
// key reaches the gateway, if at all, as a smuggled inbound `Authorization`
// or `x-api-key` header. We model that here: send proxy requests with a
// leaked key in both header positions, plus a parallel direct-bypass dial
// that carries the same leaked key, and assert:
//
//   * The gateway records exactly one row per proxy call (the inbound key
//     is dropped — verified by sister tests in `inference_proxy.rs::tests`).
//   * The bypass dial does not create any extra row (gateway never sees it).
//   * The leaked key never appears in any persisted audit `details` blob —
//     the audit sink only records metadata, never inbound headers.

#[tokio::test]
async fn stale_provider_key_in_headers_does_not_create_bypass_audit_rows() {
    const LEAKED: &str = "sk-leaked-eq0m1-must-not-bypass";
    let mock = start_mock_provider().await;
    let gw = start_gateway(
        None,
        Some(UpstreamProvider {
            base_url: mock.base_url.clone(),
            api_key: "upstream-openai-key".into(),
        }),
        Some(UpstreamProvider {
            base_url: mock.base_url.clone(),
            api_key: "upstream-anthropic-key".into(),
        }),
    )
    .await;

    let resolver = InferenceLocalResolver::new(gw.addr.clone());
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // OpenAI-shaped proxy call with a stale Bearer header.
    let openai_url = resolver.rewrite("https://inference.local/v1/chat/completions");
    let resp = client
        .post(&openai_url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {LEAKED}"))
        .body(serde_json::to_vec(&serde_json::json!({"model": "gpt-4"})).unwrap())
        .send()
        .await
        .expect("openai proxy with stale bearer");
    assert_eq!(resp.status(), StatusCode::OK);

    // Anthropic-shaped proxy call with a stale x-api-key header.
    let anthropic_url = resolver.rewrite("https://inference.local/v1/messages");
    let resp = client
        .post(&anthropic_url)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-api-key", LEAKED)
        .body(
            serde_json::to_vec(&serde_json::json!({"model": "claude-3-5-sonnet"}))
                .unwrap(),
        )
        .send()
        .await
        .expect("anthropic proxy with stale x-api-key");
    assert_eq!(resp.status(), StatusCode::OK);

    // Bypass dial with the same stale key — must not create any row.
    let direct_url = format!("{}/chat/completions", mock.base_url);
    let resp = client
        .post(&direct_url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {LEAKED}"))
        .body(serde_json::to_vec(&serde_json::json!({"model": "gpt-4"})).unwrap())
        .send()
        .await
        .expect("direct bypass with stale bearer");
    assert_eq!(resp.status(), StatusCode::OK);

    let rows = read_audit_rows(&gw.db).await;
    let proxy = proxy_rows(&rows);
    assert_eq!(
        proxy.len(),
        2,
        "expected exactly 2 inference_proxy_call rows (one per proxy call, \
         zero for bypass); got {} rows: {rows:?}",
        proxy.len(),
    );
    for row in &proxy {
        assert_eq!(row.actor_kind, "inference_proxy");
        let details = parse_details(row);
        assert_eq!(details["outcome"], "success");
        let dump = serde_json::to_string(&details).unwrap();
        assert!(
            !dump.contains(LEAKED),
            "leaked key must not appear in audit details; row: {dump}"
        );
    }
    // Sanity: the audit_log as a whole also never quotes the leaked key,
    // even across columns we don't normally inspect.
    for row in &rows {
        let details = row.details.as_deref().unwrap_or("");
        assert!(
            !details.contains(LEAKED),
            "leaked key surfaced in audit_log row {row:?}"
        );
        assert!(
            !row.actor_id.contains(LEAKED),
            "leaked key surfaced in actor_id of row {row:?}"
        );
    }
}
