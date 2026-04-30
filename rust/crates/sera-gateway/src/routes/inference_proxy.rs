//! OpenAI-compatible inference proxy (sera-7ivj PR 1).
//!
//! Implements `POST /v1/chat/completions` as a byte-stream forwarder. The
//! gateway owns the upstream provider credential — the inbound `Authorization`
//! header is stripped before dispatch and replaced with the gateway-resolved
//! upstream key.
//!
//! This is the corrected first slice from the post-`ve9x` reassessment
//! (`artifacts/reports/research/7ivj-post-ve9x-first-slice-reassessment-2026-04-30.md`):
//! no shared LLM crate, no SSE accumulator, no body-builder lift. The proxy
//! forwards bytes, records one audit event per call, and exposes an
//! [`LlmBudgetGate`] trait seam so a future bead can attach token-bucket
//! enforcement without touching the route handler.
#![allow(dead_code)]

use async_trait::async_trait;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Resolved upstream provider profile
// ---------------------------------------------------------------------------

/// Gateway-resolved upstream provider.
///
/// `base_url` is expected to be the OpenAI-compatible v1 root (e.g.
/// `https://api.openai.com/v1` or `http://127.0.0.1:1234/v1`). The proxy
/// appends `/chat/completions` when dispatching.
#[derive(Debug, Clone)]
pub struct UpstreamProvider {
    pub base_url: String,
    pub api_key: String,
}

// ---------------------------------------------------------------------------
// Budget gate seam
// ---------------------------------------------------------------------------

/// Pre-call budget decision.
///
/// `Allow` lets the proxy dispatch upstream. `Deny` short-circuits with a
/// stable 429 and never touches the upstream — credentials are not consumed
/// when budget is exhausted.
#[derive(Debug, Clone)]
pub enum BudgetDecision {
    Allow,
    Deny {
        reason: String,
        retry_after: Option<Duration>,
    },
}

/// Identifying context handed to the budget gate before dispatch.
#[derive(Debug, Clone)]
pub struct BudgetCtx {
    pub principal: String,
    pub model_requested: Option<String>,
}

/// Budget gate seam (sera-plcv M1 attaches here).
///
/// `NoopBudgetGate` is the default in PR 1; the real per-principal token
/// bucket lands in the budget-enforcement bead and slots in by replacing the
/// `Arc<dyn LlmBudgetGate>` returned from [`InferenceProxyAppState`].
#[async_trait]
pub trait LlmBudgetGate: Send + Sync {
    async fn check(&self, ctx: &BudgetCtx) -> BudgetDecision;
}

/// Default no-op gate. Always allows.
pub struct NoopBudgetGate;

#[async_trait]
impl LlmBudgetGate for NoopBudgetGate {
    async fn check(&self, _ctx: &BudgetCtx) -> BudgetDecision {
        BudgetDecision::Allow
    }
}

// ---------------------------------------------------------------------------
// Audit seam
// ---------------------------------------------------------------------------

/// One row per call. Body content is **never** carried — only metadata.
#[derive(Debug, Clone)]
pub struct ProxyAuditEvent {
    pub principal: String,
    pub provider_base_url: String,
    pub model_requested: Option<String>,
    pub stream: bool,
    pub status: u16,
    pub outcome: ProxyOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyOutcome {
    Success,
    RateLimited,
    ProviderError,
    BudgetExceeded,
    Unauthorized,
    NoUpstream,
    UpstreamUnreachable,
}

impl ProxyOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::RateLimited => "rate_limited",
            Self::ProviderError => "provider_error",
            Self::BudgetExceeded => "budget_exceeded",
            Self::Unauthorized => "unauthorized",
            Self::NoUpstream => "no_upstream",
            Self::UpstreamUnreachable => "upstream_unreachable",
        }
    }
}

#[async_trait]
pub trait InferenceProxyAudit: Send + Sync {
    async fn record(&self, event: ProxyAuditEvent);
}

/// Default audit sink — emits one structured `tracing::info!` per call. A
/// later bead replaces this with the hash-chained `sera-telemetry::audit`
/// pipeline; the trait keeps the swap surgical.
pub struct TracingProxyAudit;

#[async_trait]
impl InferenceProxyAudit for TracingProxyAudit {
    async fn record(&self, event: ProxyAuditEvent) {
        tracing::info!(
            target: "sera_gateway::inference_proxy",
            principal = %event.principal,
            provider_base_url = %event.provider_base_url,
            model_requested = ?event.model_requested,
            stream = event.stream,
            status = event.status,
            outcome = event.outcome.as_str(),
            "inference_proxy_call"
        );
    }
}

// ---------------------------------------------------------------------------
// AppState abstraction
// ---------------------------------------------------------------------------

/// AppState surface needed by the inference proxy route. The binary's
/// concrete `AppState` implements this; tests provide a stub.
pub trait InferenceProxyAppState: Send + Sync + 'static {
    /// `None` means auth is disabled (autonomous local mode); `Some` means
    /// every caller must present `Authorization: Bearer <key>`.
    fn proxy_api_key(&self) -> &Option<String>;

    /// Gateway-resolved upstream provider. Returns `None` when no usable
    /// upstream is configured — the route then refuses with a stable 503 so
    /// the caller never sees the upstream name or any credential.
    fn proxy_upstream(&self) -> Option<UpstreamProvider>;

    /// Shared reqwest client (cheap to clone — wraps an internal Arc).
    fn proxy_http_client(&self) -> reqwest::Client;

    /// Budget gate. PR 1 wires `NoopBudgetGate`; later bead swaps in the real
    /// token-bucket impl without touching this route.
    fn proxy_budget_gate(&self) -> Arc<dyn LlmBudgetGate>;

    /// Audit emitter. PR 1 wires `TracingProxyAudit`; later bead swaps in
    /// the hash-chained pipeline.
    fn proxy_audit(&self) -> Arc<dyn InferenceProxyAudit>;
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Body shape we peek at — only `stream` and `model` are needed. Body content
/// is *not* parsed; it is forwarded verbatim.
#[derive(Debug, Default, Deserialize)]
struct PeekBody {
    #[serde(default)]
    stream: Option<bool>,
    #[serde(default)]
    model: Option<String>,
}

/// `POST /v1/chat/completions`. OpenAI-compatible byte-stream forwarder.
pub async fn chat_completions<S: InferenceProxyAppState>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // 1. Auth — refuse unauthenticated callers when an api_key is configured.
    let principal = match check_auth(state.proxy_api_key(), &headers) {
        Ok(p) => p,
        Err(()) => {
            // No audit row for unauthenticated requests — the principal is
            // unknown and we don't want anonymous callers to populate the
            // audit stream.
            return unauthorized();
        }
    };

    // 2. Best-effort body peek (streaming flag + model) for budget/audit.
    let peek: PeekBody = serde_json::from_slice(&body).unwrap_or_default();
    let stream_flag = peek.stream.unwrap_or(false);
    let model_requested = peek.model.clone();

    // 3. Budget gate.
    let gate = state.proxy_budget_gate();
    let budget_ctx = BudgetCtx {
        principal: principal.clone(),
        model_requested: model_requested.clone(),
    };
    if let BudgetDecision::Deny {
        reason,
        retry_after,
    } = gate.check(&budget_ctx).await
    {
        // Resolve provider for audit metadata only; if it can't be resolved
        // we still emit the row with the intended provider blanked out.
        let provider_url = state
            .proxy_upstream()
            .map(|u| u.base_url)
            .unwrap_or_default();
        state
            .proxy_audit()
            .record(ProxyAuditEvent {
                principal: principal.clone(),
                provider_base_url: provider_url,
                model_requested: model_requested.clone(),
                stream: stream_flag,
                status: StatusCode::TOO_MANY_REQUESTS.as_u16(),
                outcome: ProxyOutcome::BudgetExceeded,
            })
            .await;
        return budget_denied(reason, retry_after);
    }

    // 4. Resolve upstream provider. Refuse cleanly if absent.
    let upstream = match state.proxy_upstream() {
        Some(u) => u,
        None => {
            state
                .proxy_audit()
                .record(ProxyAuditEvent {
                    principal: principal.clone(),
                    provider_base_url: String::new(),
                    model_requested: model_requested.clone(),
                    stream: stream_flag,
                    status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                    outcome: ProxyOutcome::NoUpstream,
                })
                .await;
            return service_unavailable("no_upstream");
        }
    };

    // 5. Forward to upstream. The inbound `Authorization` is *not* attached
    //    to the outbound request — we build a fresh request with only the
    //    headers the proxy chooses to set. Inbound `Authorization` therefore
    //    cannot leak upstream.
    let url = build_chat_url(&upstream.base_url);
    let upstream_resp = state
        .proxy_http_client()
        .post(&url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", upstream.api_key),
        )
        .body(body.clone())
        .send()
        .await;

    let response = match upstream_resp {
        Ok(r) => r,
        Err(err) => {
            state
                .proxy_audit()
                .record(ProxyAuditEvent {
                    principal: principal.clone(),
                    provider_base_url: upstream.base_url.clone(),
                    model_requested: model_requested.clone(),
                    stream: stream_flag,
                    status: StatusCode::BAD_GATEWAY.as_u16(),
                    outcome: ProxyOutcome::UpstreamUnreachable,
                })
                .await;
            // The error itself can carry transport detail; do not leak it
            // back to the caller — the upstream URL or other reqwest-internal
            // state could end up in the body.
            tracing::warn!(error = %err, "inference proxy upstream unreachable");
            return upstream_unreachable();
        }
    };

    // 6. Map upstream status to outcome bucket. Body and status are forwarded
    //    verbatim; we never rewrite the upstream payload.
    let upstream_status = response.status();
    let outcome = if upstream_status.is_success() {
        ProxyOutcome::Success
    } else if upstream_status == StatusCode::TOO_MANY_REQUESTS {
        ProxyOutcome::RateLimited
    } else {
        ProxyOutcome::ProviderError
    };

    // Capture audit-relevant headers before consuming the response.
    let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
    let retry_after = response.headers().get(header::RETRY_AFTER).cloned();

    state
        .proxy_audit()
        .record(ProxyAuditEvent {
            principal: principal.clone(),
            provider_base_url: upstream.base_url.clone(),
            model_requested: model_requested.clone(),
            stream: stream_flag,
            status: upstream_status.as_u16(),
            outcome,
        })
        .await;

    // 7. Stream body back. Works for SSE and plain JSON alike.
    let mut builder = Response::builder().status(upstream_status);
    if let Some(ct) = content_type {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }
    if let Some(ra) = retry_after {
        builder = builder.header(header::RETRY_AFTER, ra);
    }
    let stream = response.bytes_stream();
    let body_out = Body::from_stream(stream);

    builder.body(body_out).unwrap_or_else(|err| {
        tracing::error!(error = %err, "inference proxy: failed to assemble response");
        (StatusCode::INTERNAL_SERVER_ERROR, "").into_response()
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the principal id when auth passes, or `Err(())` for 401.
///
/// Mirrors the simple bearer match used by `route_a2a::check_auth` so the
/// proxy doesn't grow a second auth stack.
fn check_auth(api_key: &Option<String>, headers: &HeaderMap) -> Result<String, ()> {
    let expected = match api_key {
        None => return Ok("anonymous".to_string()),
        Some(k) => k,
    };
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match provided {
        Some(k) if k == expected.as_str() => Ok("bootstrap".to_string()),
        _ => Err(()),
    }
}

fn build_chat_url(base: &str) -> String {
    format!("{}/chat/completions", base.trim_end_matches('/'))
}

fn unauthorized() -> Response {
    let mut resp = (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({"error": {"code": "unauthorized"}})),
    )
        .into_response();
    resp.headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    resp
}

fn budget_denied(reason: String, retry_after: Option<Duration>) -> Response {
    let body = serde_json::json!({
        "error": {
            "code": "budget_exceeded",
            "message": reason,
        }
    });
    let mut resp = (StatusCode::TOO_MANY_REQUESTS, axum::Json(body)).into_response();
    if let Some(ra) = retry_after
        && let Ok(v) = HeaderValue::from_str(&ra.as_secs().to_string())
    {
        resp.headers_mut().insert(header::RETRY_AFTER, v);
    }
    resp
}

fn service_unavailable(code: &'static str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({"error": {"code": code}})),
    )
        .into_response()
}

fn upstream_unreachable() -> Response {
    (
        StatusCode::BAD_GATEWAY,
        axum::Json(serde_json::json!({"error": {"code": "upstream_unreachable"}})),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::response::sse::{Event, Sse};
    use axum::routing::post;
    use futures_util::stream;
    use std::sync::Mutex;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tower::ServiceExt;

    // --- Test state ---------------------------------------------------------

    struct TestState {
        api_key: Option<String>,
        upstream: Option<UpstreamProvider>,
        client: reqwest::Client,
        gate: Arc<dyn LlmBudgetGate>,
        audit: Arc<RecordingAudit>,
    }

    impl TestState {
        fn new(api_key: Option<&str>, upstream: Option<UpstreamProvider>) -> Arc<Self> {
            Arc::new(Self {
                api_key: api_key.map(String::from),
                upstream,
                client: reqwest::Client::builder()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .expect("client"),
                gate: Arc::new(NoopBudgetGate),
                audit: Arc::new(RecordingAudit::default()),
            })
        }

        fn with_gate(mut self: Arc<Self>, gate: Arc<dyn LlmBudgetGate>) -> Arc<Self> {
            // Only used at construction in single-threaded tests where the
            // Arc is unique — refusal here would mean the test misuses it.
            let inner = Arc::get_mut(&mut self).expect("unique Arc");
            inner.gate = gate;
            self
        }
    }

    impl InferenceProxyAppState for TestState {
        fn proxy_api_key(&self) -> &Option<String> {
            &self.api_key
        }
        fn proxy_upstream(&self) -> Option<UpstreamProvider> {
            self.upstream.clone()
        }
        fn proxy_http_client(&self) -> reqwest::Client {
            self.client.clone()
        }
        fn proxy_budget_gate(&self) -> Arc<dyn LlmBudgetGate> {
            Arc::clone(&self.gate)
        }
        fn proxy_audit(&self) -> Arc<dyn InferenceProxyAudit> {
            Arc::clone(&self.audit) as Arc<dyn InferenceProxyAudit>
        }
    }

    // --- Recording helpers --------------------------------------------------

    #[derive(Default)]
    struct RecordingAudit {
        events: Mutex<Vec<ProxyAuditEvent>>,
    }

    impl RecordingAudit {
        fn snapshot(&self) -> Vec<ProxyAuditEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl InferenceProxyAudit for RecordingAudit {
        async fn record(&self, event: ProxyAuditEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[derive(Default)]
    struct DenyingGate {
        reason: String,
        retry_after: Option<Duration>,
    }

    #[async_trait]
    impl LlmBudgetGate for DenyingGate {
        async fn check(&self, _ctx: &BudgetCtx) -> BudgetDecision {
            BudgetDecision::Deny {
                reason: self.reason.clone(),
                retry_after: self.retry_after,
            }
        }
    }

    /// Captures what an upstream mock observed about a request.
    #[derive(Debug, Default, Clone)]
    struct CapturedRequest {
        authorization: Option<String>,
        body: Vec<u8>,
    }

    /// Drop-on-drop handle for the mock upstream HTTP server.
    struct MockUpstream {
        base_url: String,
        captured: Arc<Mutex<CapturedRequest>>,
        shutdown: Option<oneshot::Sender<()>>,
    }

    impl Drop for MockUpstream {
        fn drop(&mut self) {
            if let Some(tx) = self.shutdown.take() {
                let _ = tx.send(());
            }
        }
    }

    /// Starts a mock upstream that returns the given response shape on
    /// `POST /v1/chat/completions`.
    #[derive(Clone)]
    enum MockResponse {
        Json {
            status: StatusCode,
            body: serde_json::Value,
            retry_after: Option<&'static str>,
        },
        SseChunks(Vec<&'static str>),
    }

    async fn start_mock_upstream(resp: MockResponse) -> MockUpstream {
        let captured = Arc::new(Mutex::new(CapturedRequest::default()));
        let captured_for_handler = Arc::clone(&captured);

        let handler = move |headers: HeaderMap, body: Bytes| {
            let captured = Arc::clone(&captured_for_handler);
            let resp = resp.clone();
            async move {
                let auth = headers
                    .get(header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .map(String::from);
                {
                    let mut c = captured.lock().unwrap();
                    c.authorization = auth;
                    c.body = body.to_vec();
                }
                match resp {
                    MockResponse::Json {
                        status,
                        body,
                        retry_after,
                    } => {
                        let mut response = (status, axum::Json(body)).into_response();
                        if let Some(ra) = retry_after {
                            response
                                .headers_mut()
                                .insert(header::RETRY_AFTER, HeaderValue::from_static(ra));
                        }
                        response
                    }
                    MockResponse::SseChunks(chunks) => {
                        let events: Vec<Result<Event, std::convert::Infallible>> = chunks
                            .into_iter()
                            .map(|c| Ok(Event::default().data(c)))
                            .collect();
                        Sse::new(stream::iter(events)).into_response()
                    }
                }
            }
        };

        let app: Router = Router::new().route("/v1/chat/completions", post(handler));

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

        MockUpstream {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            captured,
            shutdown: Some(tx),
        }
    }

    fn route(state: Arc<TestState>) -> Router {
        Router::new()
            .route("/v1/chat/completions", post(chat_completions::<TestState>))
            .with_state(state)
    }

    fn json_body(v: serde_json::Value) -> Body {
        Body::from(serde_json::to_vec(&v).unwrap())
    }

    // --- Tests --------------------------------------------------------------

    #[tokio::test]
    async fn unauthenticated_returns_401() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({"ok": true}),
            retry_after: None,
        })
        .await;
        let state = TestState::new(
            Some("expected-key"),
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "upstream-key".into(),
            }),
        );
        let app = route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "x"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        // No upstream call.
        assert!(mock.captured.lock().unwrap().body.is_empty());
        // No audit row for unauthenticated callers.
        assert!(state.audit.snapshot().is_empty());
    }

    #[tokio::test]
    async fn non_streaming_forwards_and_audits_success() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({
                "id": "chatcmpl-1",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}}],
                "usage": {"prompt_tokens": 4, "completion_tokens": 2, "total_tokens": 6}
            }),
            retry_after: None,
        })
        .await;
        let state = TestState::new(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "upstream-key".into(),
            }),
        );
        let app = route(Arc::clone(&state));

        let request_body = serde_json::json!({"model": "gpt-4", "messages": [], "stream": false});
        let resp = app
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(request_body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["id"], "chatcmpl-1");

        let captured = mock.captured.lock().unwrap().clone();
        // Upstream sees the gateway-injected Bearer key.
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer upstream-key")
        );
        // Body is forwarded verbatim.
        let forwarded: serde_json::Value = serde_json::from_slice(&captured.body).unwrap();
        assert_eq!(forwarded, request_body);

        let events = state.audit.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].outcome, ProxyOutcome::Success);
        assert_eq!(events[0].status, 200);
        assert_eq!(events[0].model_requested.as_deref(), Some("gpt-4"));
        assert!(!events[0].stream);
    }

    #[tokio::test]
    async fn streaming_forwards_sse_bytes_and_audits_once() {
        let mock = start_mock_upstream(MockResponse::SseChunks(vec![
            r#"{"choices":[{"delta":{"role":"assistant","content":""}}]}"#,
            r#"{"choices":[{"delta":{"content":"hi"}}]}"#,
            "[DONE]",
        ]))
        .await;
        let state = TestState::new(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "upstream-key".into(),
            }),
        );
        let app = route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(
                        serde_json::json!({"model": "gpt-4", "stream": true}),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        assert!(
            ct.starts_with("text/event-stream"),
            "expected SSE content-type, got {ct}"
        );
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("\"hi\""));
        assert!(body_str.contains("[DONE]"));

        // Single audit row for the whole stream — never per chunk.
        let events = state.audit.snapshot();
        assert_eq!(events.len(), 1);
        assert!(events[0].stream);
        assert_eq!(events[0].outcome, ProxyOutcome::Success);
    }

    #[tokio::test]
    async fn inbound_authorization_header_is_not_forwarded() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({"ok": true}),
            retry_after: None,
        })
        .await;
        let state = TestState::new(
            Some("gateway-key"),
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "upstream-secret".into(),
            }),
        );
        let app = route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, "Bearer gateway-key")
                    .body(json_body(serde_json::json!({"model": "x"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let captured = mock.captured.lock().unwrap().clone();
        // Critically: the upstream must see `upstream-secret`, NOT the
        // inbound `gateway-key`. The inbound bearer must never reach
        // upstream.
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer upstream-secret"),
        );
        assert_ne!(
            captured.authorization.as_deref(),
            Some("Bearer gateway-key"),
        );
    }

    #[tokio::test]
    async fn upstream_429_is_propagated_with_retry_after() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::TOO_MANY_REQUESTS,
            body: serde_json::json!({"error": {"message": "rate limited"}}),
            retry_after: Some("30"),
        })
        .await;
        let state = TestState::new(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "k".into(),
            }),
        );
        let app = route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "x"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let ra = resp
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        assert_eq!(ra.as_deref(), Some("30"));

        let events = state.audit.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].outcome, ProxyOutcome::RateLimited);
        assert_eq!(events[0].status, 429);
    }

    #[tokio::test]
    async fn upstream_500_does_not_leak_credentials() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: serde_json::json!({"error": {"message": "boom"}}),
            retry_after: None,
        })
        .await;
        let state = TestState::new(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "very-secret-upstream-key".into(),
            }),
        );
        let app = route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "x"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            !body_str.contains("very-secret-upstream-key"),
            "upstream key leaked into error body: {body_str}"
        );

        let events = state.audit.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].outcome, ProxyOutcome::ProviderError);
    }

    #[tokio::test]
    async fn budget_gate_denial_returns_429_and_skips_upstream() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({"ok": true}),
            retry_after: None,
        })
        .await;
        let state = TestState::new(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "k".into(),
            }),
        );
        let state = state.with_gate(Arc::new(DenyingGate {
            reason: "monthly budget exhausted".into(),
            retry_after: Some(Duration::from_secs(60)),
        }));
        let app = route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "gpt-4"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after = resp
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        assert_eq!(retry_after.as_deref(), Some("60"));

        // Upstream must not have been called.
        assert!(mock.captured.lock().unwrap().body.is_empty());

        let events = state.audit.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].outcome, ProxyOutcome::BudgetExceeded);
        assert_eq!(events[0].status, 429);
    }

    #[tokio::test]
    async fn no_upstream_returns_503() {
        let state = TestState::new(None, None);
        let app = route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "x"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        let events = state.audit.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].outcome, ProxyOutcome::NoUpstream);
    }

    #[test]
    fn build_chat_url_handles_trailing_slash() {
        assert_eq!(
            build_chat_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            build_chat_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/chat/completions"
        );
    }
}
