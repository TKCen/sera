//! Inference proxy routes (sera-7ivj).
//!
//! PR 1 implements `POST /v1/chat/completions` (OpenAI-compatible) as a
//! byte-stream forwarder. PR 2 adds `POST /v1/messages` (Anthropic-compatible)
//! using the same plumbing: same auth, same budget gate seam, same audit seam,
//! and the same response-forwarding tail.
//!
//! In both routes the gateway owns the upstream credential. The inbound
//! `Authorization` and `x-api-key` headers are not forwarded — the outbound
//! request is built fresh and only carries headers the proxy chooses to set.
//! When the resolved upstream has no key configured (e.g. local LM Studio
//! with auth disabled), the proxy omits the upstream auth header rather than
//! synthesising an empty `Bearer ` / `x-api-key:` value.
//!
//! No shared LLM crate, no SSE accumulator, no body-builder lift (per the
//! post-`ve9x` reassessment, `artifacts/reports/research/7ivj-post-ve9x-first-slice-reassessment-2026-04-30.md`).
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
/// `base_url` is the v1 root (e.g. `https://api.openai.com/v1`,
/// `https://api.anthropic.com/v1`, or `http://127.0.0.1:1234/v1`). Each route
/// appends its own URL suffix (`/chat/completions` or `/messages`) when
/// dispatching.
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

/// Event-type written to `audit_log` for every proxy call. Keeps a single
/// stable name so operators can filter the trail with one `WHERE event_type`.
const PROXY_AUDIT_EVENT_TYPE: &str = "inference_proxy_call";

/// Actor-kind tag — distinguishes proxy events from `human` / `agent` actors.
const PROXY_AUDIT_ACTOR_KIND: &str = "inference_proxy";

/// Durable audit sink — persists one row per proxy call to the gateway's
/// shared `SqliteDb`. Replaces [`TracingProxyAudit`] in the production
/// `AppState` so an operator can inspect proxy traffic via `query_audit`
/// after the fact.
///
/// Body content is **not** persisted — only the metadata already present in
/// [`ProxyAuditEvent`]. Failures are best-effort: on insert error we log a
/// warning but never fail the proxy response.
pub struct SqliteInferenceProxyAudit {
    db: Arc<tokio::sync::Mutex<sera_db::sqlite::SqliteDb>>,
}

impl SqliteInferenceProxyAudit {
    pub fn new(db: Arc<tokio::sync::Mutex<sera_db::sqlite::SqliteDb>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl InferenceProxyAudit for SqliteInferenceProxyAudit {
    async fn record(&self, event: ProxyAuditEvent) {
        let details = serde_json::json!({
            "provider_base_url": event.provider_base_url,
            "model_requested": event.model_requested,
            "stream": event.stream,
            "status": event.status,
            "outcome": event.outcome.as_str(),
        })
        .to_string();
        let db = self.db.lock().await;
        if let Err(e) = db.append_audit(
            PROXY_AUDIT_EVENT_TYPE,
            &event.principal,
            PROXY_AUDIT_ACTOR_KIND,
            Some(&details),
        ) {
            tracing::warn!(
                error = %e,
                principal = %event.principal,
                outcome = event.outcome.as_str(),
                "inference_proxy: failed to persist audit row"
            );
        }
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

    /// Gateway-resolved OpenAI-shaped upstream (used by
    /// `POST /v1/chat/completions`). Returns `None` when no usable upstream is
    /// configured — the route then refuses with a stable 503 so the caller
    /// never sees the upstream name or any credential.
    fn proxy_upstream(&self) -> Option<UpstreamProvider>;

    /// Gateway-resolved Anthropic-shaped upstream (used by
    /// `POST /v1/messages`). Independent of [`Self::proxy_upstream`]: an
    /// operator may configure either, both, or neither. The default returns
    /// `None`, which makes `POST /v1/messages` 503 — opt-in via the binary's
    /// concrete impl.
    fn proxy_anthropic_upstream(&self) -> Option<UpstreamProvider> {
        None
    }

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
        Err(()) => return unauthorized(),
    };

    // 2. Best-effort body peek (streaming flag + model) for budget/audit.
    let peek: PeekBody = serde_json::from_slice(&body).unwrap_or_default();
    let stream_flag = peek.stream.unwrap_or(false);
    let model_requested = peek.model.clone();

    // 3. Budget gate.
    if let Some(resp) = check_budget_or_record(
        &*state,
        &principal,
        model_requested.as_deref(),
        stream_flag,
        &state.proxy_upstream(),
    )
    .await
    {
        return resp;
    }

    // 4. Resolve upstream provider. Refuse cleanly if absent.
    let upstream = match state.proxy_upstream() {
        Some(u) => u,
        None => {
            return record_no_upstream(
                &*state,
                &principal,
                model_requested.as_deref(),
                stream_flag,
            )
            .await;
        }
    };

    // 5. Build the outbound request. The inbound `Authorization` is *not*
    //    attached — we build a fresh request with only the headers the proxy
    //    chooses to set, so the inbound bearer cannot leak upstream. When the
    //    resolved provider has no configured key (e.g. local LM Studio with
    //    auth disabled), we omit the outbound `Authorization` header rather
    //    than synthesising an empty `Bearer ` value.
    let url = build_chat_url(&upstream.base_url);
    let mut upstream_req = state
        .proxy_http_client()
        .post(&url)
        .header(header::CONTENT_TYPE, "application/json");
    if !upstream.api_key.is_empty() {
        upstream_req = upstream_req.header(
            header::AUTHORIZATION,
            format!("Bearer {}", upstream.api_key),
        );
    }

    forward_to_upstream(
        &*state,
        upstream_req,
        body,
        ForwardMeta {
            principal,
            provider_base_url: upstream.base_url,
            model_requested,
            stream: stream_flag,
        },
    )
    .await
}

/// Default Anthropic API version when the client omits `anthropic-version`.
///
/// Anthropic requires every request to declare an API version; missing the
/// header would surface as a 4xx upstream. Forwarding a stable default keeps
/// well-behaved Anthropic-compatible clients working without each having to
/// re-declare the version, while clients that *do* set the header still take
/// precedence (their value is forwarded verbatim).
pub const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

const ANTHROPIC_VERSION_HEADER: &str = "anthropic-version";
const ANTHROPIC_API_KEY_HEADER: &str = "x-api-key";

/// `POST /v1/messages`. Anthropic-compatible byte-stream forwarder.
///
/// Mirrors the [`chat_completions`] scaffold — same auth, same body peek,
/// same budget gate, same audit and forwarding tail — but routes to a
/// distinct upstream ([`InferenceProxyAppState::proxy_anthropic_upstream`])
/// and dispatches with Anthropic protocol headers (`x-api-key`,
/// `anthropic-version`) instead of `Authorization: Bearer`.
pub async fn chat_messages<S: InferenceProxyAppState>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // 1. Auth.
    let principal = match check_auth(state.proxy_api_key(), &headers) {
        Ok(p) => p,
        Err(()) => return unauthorized(),
    };

    // 2. Body peek.
    let peek: PeekBody = serde_json::from_slice(&body).unwrap_or_default();
    let stream_flag = peek.stream.unwrap_or(false);
    let model_requested = peek.model.clone();

    // 3. Budget gate.
    if let Some(resp) = check_budget_or_record(
        &*state,
        &principal,
        model_requested.as_deref(),
        stream_flag,
        &state.proxy_anthropic_upstream(),
    )
    .await
    {
        return resp;
    }

    // 4. Resolve Anthropic upstream.
    let upstream = match state.proxy_anthropic_upstream() {
        Some(u) => u,
        None => {
            return record_no_upstream(
                &*state,
                &principal,
                model_requested.as_deref(),
                stream_flag,
            )
            .await;
        }
    };

    // 5. Build outbound request. Inbound `Authorization` AND inbound
    //    `x-api-key` are dropped naturally — we never copy headers from the
    //    request, only set the ones the proxy chose. The default
    //    `anthropic-version` is applied only when the client omitted the
    //    header; an explicit client value is forwarded as-is.
    let url = build_messages_url(&upstream.base_url);
    let anthropic_version = headers
        .get(ANTHROPIC_VERSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .unwrap_or_else(|| DEFAULT_ANTHROPIC_VERSION.to_string());

    let mut upstream_req = state
        .proxy_http_client()
        .post(&url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(ANTHROPIC_VERSION_HEADER, &anthropic_version);
    if !upstream.api_key.is_empty() {
        upstream_req = upstream_req.header(ANTHROPIC_API_KEY_HEADER, &upstream.api_key);
    }

    forward_to_upstream(
        &*state,
        upstream_req,
        body,
        ForwardMeta {
            principal,
            provider_base_url: upstream.base_url,
            model_requested,
            stream: stream_flag,
        },
    )
    .await
}

/// Audit/identity context shared by both proxy routes' forwarding tail.
struct ForwardMeta {
    principal: String,
    provider_base_url: String,
    model_requested: Option<String>,
    stream: bool,
}

/// Pre-dispatch budget gate. Returns `Some(response)` when the gate denies and
/// the route should short-circuit. The audit row is emitted with the upstream
/// base_url that the route *would* have called (for traceability), or empty
/// when the upstream was not configurable.
async fn check_budget_or_record<S: InferenceProxyAppState + ?Sized>(
    state: &S,
    principal: &str,
    model_requested: Option<&str>,
    stream: bool,
    upstream: &Option<UpstreamProvider>,
) -> Option<Response> {
    let gate = state.proxy_budget_gate();
    let ctx = BudgetCtx {
        principal: principal.to_string(),
        model_requested: model_requested.map(String::from),
    };
    let BudgetDecision::Deny {
        reason,
        retry_after,
    } = gate.check(&ctx).await
    else {
        return None;
    };
    let provider_url = upstream
        .as_ref()
        .map(|u| u.base_url.clone())
        .unwrap_or_default();
    state
        .proxy_audit()
        .record(ProxyAuditEvent {
            principal: principal.to_string(),
            provider_base_url: provider_url,
            model_requested: model_requested.map(String::from),
            stream,
            status: StatusCode::TOO_MANY_REQUESTS.as_u16(),
            outcome: ProxyOutcome::BudgetExceeded,
        })
        .await;
    Some(budget_denied(reason, retry_after))
}

/// Emit the `no_upstream` audit row and return a stable 503.
async fn record_no_upstream<S: InferenceProxyAppState + ?Sized>(
    state: &S,
    principal: &str,
    model_requested: Option<&str>,
    stream: bool,
) -> Response {
    state
        .proxy_audit()
        .record(ProxyAuditEvent {
            principal: principal.to_string(),
            provider_base_url: String::new(),
            model_requested: model_requested.map(String::from),
            stream,
            status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
            outcome: ProxyOutcome::NoUpstream,
        })
        .await;
    service_unavailable("no_upstream")
}

/// Send the prepared request, emit one audit row, and stream the upstream
/// body back to the caller. Body and status are forwarded verbatim — the
/// proxy never rewrites the upstream payload, so SSE / event-stream chunks
/// pass through unchanged for both OpenAI and Anthropic clients.
async fn forward_to_upstream<S: InferenceProxyAppState + ?Sized>(
    state: &S,
    upstream_req: reqwest::RequestBuilder,
    body: Bytes,
    meta: ForwardMeta,
) -> Response {
    let upstream_resp = upstream_req.body(body).send().await;

    let response = match upstream_resp {
        Ok(r) => r,
        Err(err) => {
            state
                .proxy_audit()
                .record(ProxyAuditEvent {
                    principal: meta.principal,
                    provider_base_url: meta.provider_base_url,
                    model_requested: meta.model_requested,
                    stream: meta.stream,
                    status: StatusCode::BAD_GATEWAY.as_u16(),
                    outcome: ProxyOutcome::UpstreamUnreachable,
                })
                .await;
            // The reqwest error can carry transport detail (URLs, etc.); do
            // not propagate it to the caller body.
            tracing::warn!(error = %err, "inference proxy upstream unreachable");
            return upstream_unreachable();
        }
    };

    let upstream_status = response.status();
    let outcome = if upstream_status.is_success() {
        ProxyOutcome::Success
    } else if upstream_status == StatusCode::TOO_MANY_REQUESTS {
        ProxyOutcome::RateLimited
    } else {
        ProxyOutcome::ProviderError
    };

    let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
    let retry_after = response.headers().get(header::RETRY_AFTER).cloned();

    state
        .proxy_audit()
        .record(ProxyAuditEvent {
            principal: meta.principal,
            provider_base_url: meta.provider_base_url,
            model_requested: meta.model_requested,
            stream: meta.stream,
            status: upstream_status.as_u16(),
            outcome,
        })
        .await;

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

fn build_messages_url(base: &str) -> String {
    format!("{}/messages", base.trim_end_matches('/'))
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
        anthropic_upstream: Option<UpstreamProvider>,
        client: reqwest::Client,
        gate: Arc<dyn LlmBudgetGate>,
        audit: Arc<RecordingAudit>,
    }

    impl TestState {
        fn new(api_key: Option<&str>, upstream: Option<UpstreamProvider>) -> Arc<Self> {
            Arc::new(Self {
                api_key: api_key.map(String::from),
                upstream,
                anthropic_upstream: None,
                client: reqwest::Client::builder()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .expect("client"),
                gate: Arc::new(NoopBudgetGate),
                audit: Arc::new(RecordingAudit::default()),
            })
        }

        fn new_anthropic(
            api_key: Option<&str>,
            anthropic_upstream: Option<UpstreamProvider>,
        ) -> Arc<Self> {
            Arc::new(Self {
                api_key: api_key.map(String::from),
                upstream: None,
                anthropic_upstream,
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
        x_api_key: Option<String>,
        anthropic_version: Option<String>,
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
                let x_api_key = headers
                    .get(ANTHROPIC_API_KEY_HEADER)
                    .and_then(|v| v.to_str().ok())
                    .map(String::from);
                let anthropic_version = headers
                    .get(ANTHROPIC_VERSION_HEADER)
                    .and_then(|v| v.to_str().ok())
                    .map(String::from);
                {
                    let mut c = captured.lock().unwrap();
                    c.authorization = auth;
                    c.x_api_key = x_api_key;
                    c.anthropic_version = anthropic_version;
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

        let app: Router = Router::new()
            .route("/v1/chat/completions", post(handler.clone()))
            .route("/v1/messages", post(handler));

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
    async fn empty_upstream_key_omits_authorization_header() {
        // Codex review #3168181233 (P1): when the resolved provider has no
        // configured key (e.g. local LM Studio with auth disabled), the
        // proxy must omit the outbound `Authorization` header rather than
        // sending an empty `Bearer ` value.
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
                api_key: String::new(),
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
        // No outbound `Authorization` — the inbound bearer is still stripped,
        // and we don't synthesise a malformed empty bearer to replace it.
        assert!(
            captured.authorization.is_none(),
            "expected no Authorization header upstream, got {:?}",
            captured.authorization,
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

    #[test]
    fn build_messages_url_handles_trailing_slash() {
        assert_eq!(
            build_messages_url("https://api.anthropic.com/v1"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            build_messages_url("https://api.anthropic.com/v1/"),
            "https://api.anthropic.com/v1/messages"
        );
    }

    // ---------------------------------------------------------------------
    // Anthropic /v1/messages — sera-7ivj PR 2
    // ---------------------------------------------------------------------

    fn messages_route(state: Arc<TestState>) -> Router {
        Router::new()
            .route("/v1/messages", post(chat_messages::<TestState>))
            .with_state(state)
    }

    #[tokio::test]
    async fn messages_unauthenticated_returns_401() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({"ok": true}),
            retry_after: None,
        })
        .await;
        let state = TestState::new_anthropic(
            Some("expected-key"),
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "upstream-key".into(),
            }),
        );
        let app = messages_route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/messages")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "claude-3-5-sonnet"})))
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
    async fn messages_non_streaming_forwards_and_audits_success() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({
                "id": "msg_01",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "text", "text": "hi"}],
                "model": "claude-3-5-sonnet",
                "usage": {"input_tokens": 4, "output_tokens": 2}
            }),
            retry_after: None,
        })
        .await;
        let state = TestState::new_anthropic(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "upstream-key".into(),
            }),
        );
        let app = messages_route(Arc::clone(&state));

        let request_body = serde_json::json!({
            "model": "claude-3-5-sonnet",
            "max_tokens": 32,
            "messages": [{"role": "user", "content": "hi"}],
            "stream": false,
        });
        let resp = app
            .oneshot(
                Request::post("/v1/messages")
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
        assert_eq!(parsed["id"], "msg_01");

        let captured = mock.captured.lock().unwrap().clone();
        // Body forwarded verbatim — no translation between protocols.
        let forwarded: serde_json::Value = serde_json::from_slice(&captured.body).unwrap();
        assert_eq!(forwarded, request_body);
        // Anthropic auth header injected; OpenAI-style Authorization absent.
        assert_eq!(captured.x_api_key.as_deref(), Some("upstream-key"));
        assert!(captured.authorization.is_none());

        let events = state.audit.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].outcome, ProxyOutcome::Success);
        assert_eq!(events[0].status, 200);
        assert_eq!(
            events[0].model_requested.as_deref(),
            Some("claude-3-5-sonnet")
        );
        assert!(!events[0].stream);
    }

    #[tokio::test]
    async fn messages_streaming_forwards_sse_bytes_and_audits_once() {
        // Anthropic-shaped SSE events; bytes pass through unchanged.
        let mock = start_mock_upstream(MockResponse::SseChunks(vec![
            r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_01","role":"assistant"}}"#,
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#,
            r#"event: message_stop
data: {"type":"message_stop"}"#,
        ]))
        .await;
        let state = TestState::new_anthropic(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "upstream-key".into(),
            }),
        );
        let app = messages_route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/messages")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({
                        "model": "claude-3-5-sonnet",
                        "max_tokens": 32,
                        "stream": true,
                    })))
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
        // Anthropic event names round-trip unchanged.
        assert!(body_str.contains("message_start"));
        assert!(body_str.contains("content_block_delta"));
        assert!(body_str.contains("message_stop"));
        // No translation into OpenAI's `[DONE]` sentinel.
        assert!(!body_str.contains("[DONE]"));

        // Single audit row for the whole stream — never per chunk.
        let events = state.audit.snapshot();
        assert_eq!(events.len(), 1);
        assert!(events[0].stream);
        assert_eq!(events[0].outcome, ProxyOutcome::Success);
    }

    #[tokio::test]
    async fn messages_inbound_auth_and_x_api_key_are_not_forwarded() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({"ok": true}),
            retry_after: None,
        })
        .await;
        let state = TestState::new_anthropic(
            Some("gateway-key"),
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "upstream-secret".into(),
            }),
        );
        let app = messages_route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/messages")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, "Bearer gateway-key")
                    // Caller attempts to smuggle a different upstream key.
                    .header(ANTHROPIC_API_KEY_HEADER, "attacker-key")
                    .body(json_body(serde_json::json!({"model": "claude-3-5-sonnet"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let captured = mock.captured.lock().unwrap().clone();
        // Inbound bearer never reaches upstream.
        assert!(captured.authorization.is_none());
        // The x-api-key seen upstream is the gateway-injected one, NOT the
        // inbound caller-supplied value.
        assert_eq!(captured.x_api_key.as_deref(), Some("upstream-secret"));
        assert_ne!(captured.x_api_key.as_deref(), Some("attacker-key"));
    }

    #[tokio::test]
    async fn messages_empty_upstream_key_omits_x_api_key_header() {
        // Mirror the chat-completions empty-key invariant: when the resolved
        // provider has no configured key, omit the outbound auth header
        // rather than synthesising an empty `x-api-key:` value.
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({"ok": true}),
            retry_after: None,
        })
        .await;
        let state = TestState::new_anthropic(
            Some("gateway-key"),
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: String::new(),
            }),
        );
        let app = messages_route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/messages")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, "Bearer gateway-key")
                    .header(ANTHROPIC_API_KEY_HEADER, "attacker-key")
                    .body(json_body(serde_json::json!({"model": "claude-3-5-sonnet"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let captured = mock.captured.lock().unwrap().clone();
        // No outbound bearer.
        assert!(captured.authorization.is_none());
        // No outbound x-api-key — the inbound is dropped, and we don't
        // synthesise an empty replacement.
        assert!(
            captured.x_api_key.is_none(),
            "expected no x-api-key header upstream, got {:?}",
            captured.x_api_key,
        );
    }

    #[tokio::test]
    async fn messages_anthropic_version_default_applied_when_missing() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({"ok": true}),
            retry_after: None,
        })
        .await;
        let state = TestState::new_anthropic(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "k".into(),
            }),
        );
        let app = messages_route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/messages")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "claude-3-5-sonnet"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let captured = mock.captured.lock().unwrap().clone();
        assert_eq!(
            captured.anthropic_version.as_deref(),
            Some(DEFAULT_ANTHROPIC_VERSION)
        );
    }

    #[tokio::test]
    async fn messages_anthropic_version_forwarded_when_provided() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({"ok": true}),
            retry_after: None,
        })
        .await;
        let state = TestState::new_anthropic(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "k".into(),
            }),
        );
        let app = messages_route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/messages")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(ANTHROPIC_VERSION_HEADER, "2024-01-01")
                    .body(json_body(serde_json::json!({"model": "claude-3-5-sonnet"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let captured = mock.captured.lock().unwrap().clone();
        // Client-provided version wins over the default.
        assert_eq!(captured.anthropic_version.as_deref(), Some("2024-01-01"));
    }

    #[tokio::test]
    async fn messages_budget_gate_denial_returns_429_and_skips_upstream() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({"ok": true}),
            retry_after: None,
        })
        .await;
        let state = TestState::new_anthropic(
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
        let app = messages_route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/messages")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "claude-3-5-sonnet"})))
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
    async fn messages_upstream_error_does_not_leak_credentials() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: serde_json::json!({"error": {"message": "boom"}}),
            retry_after: None,
        })
        .await;
        let state = TestState::new_anthropic(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "very-secret-anthropic-key".into(),
            }),
        );
        let app = messages_route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/messages")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "claude-3-5-sonnet"})))
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
            !body_str.contains("very-secret-anthropic-key"),
            "upstream key leaked into error body: {body_str}"
        );

        let events = state.audit.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].outcome, ProxyOutcome::ProviderError);
    }

    #[tokio::test]
    async fn messages_no_upstream_returns_503() {
        let state = TestState::new_anthropic(None, None);
        let app = messages_route(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/messages")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "claude-3-5-sonnet"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        let events = state.audit.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].outcome, ProxyOutcome::NoUpstream);
    }

    // ---------------------------------------------------------------------
    // SqliteInferenceProxyAudit — sera-7ivj PR3 durable persistence
    // ---------------------------------------------------------------------
    //
    // Goal: prove that the production audit sink writes one row per proxy
    // call into the gateway's existing `audit_log` table — covering the
    // five outcomes the previous tracing-only sink could not surface to an
    // operator. Body content is never persisted; only metadata.

    use sera_db::sqlite::SqliteDb;
    use tokio::sync::Mutex as TokioMutex;

    fn open_audit_db() -> Arc<TokioMutex<SqliteDb>> {
        Arc::new(TokioMutex::new(
            SqliteDb::open_in_memory().expect("in-memory sqlite"),
        ))
    }

    /// State variant that carries an `Arc<dyn InferenceProxyAudit>` directly,
    /// so persistence-focused tests can drive the route with a real
    /// `SqliteInferenceProxyAudit` (the production sink) instead of the
    /// in-memory `RecordingAudit` used by the trait-surface tests above.
    struct PersistentTestState {
        api_key: Option<String>,
        upstream: Option<UpstreamProvider>,
        anthropic_upstream: Option<UpstreamProvider>,
        client: reqwest::Client,
        gate: Arc<dyn LlmBudgetGate>,
        audit: Arc<dyn InferenceProxyAudit>,
    }

    impl InferenceProxyAppState for PersistentTestState {
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

    fn make_persistent_state(
        api_key: Option<&str>,
        upstream: Option<UpstreamProvider>,
        anthropic_upstream: Option<UpstreamProvider>,
        gate: Arc<dyn LlmBudgetGate>,
        db: Arc<TokioMutex<SqliteDb>>,
    ) -> Arc<PersistentTestState> {
        Arc::new(PersistentTestState {
            api_key: api_key.map(String::from),
            upstream,
            anthropic_upstream,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("client"),
            gate,
            audit: Arc::new(SqliteInferenceProxyAudit::new(db)),
        })
    }

    fn chat_route_persistent(state: Arc<PersistentTestState>) -> Router {
        Router::new()
            .route(
                "/v1/chat/completions",
                post(chat_completions::<PersistentTestState>),
            )
            .with_state(state)
    }

    fn messages_route_persistent(state: Arc<PersistentTestState>) -> Router {
        Router::new()
            .route("/v1/messages", post(chat_messages::<PersistentTestState>))
            .with_state(state)
    }

    /// Read every audit row out of the shared SqliteDb so tests can assert
    /// on persisted shape without depending on row-id ordering.
    async fn read_audit_rows(db: &Arc<TokioMutex<SqliteDb>>) -> Vec<sera_db::sqlite::AuditRow> {
        let guard = db.lock().await;
        guard.query_audit(50).expect("query audit")
    }

    fn parse_details(row: &sera_db::sqlite::AuditRow) -> serde_json::Value {
        let raw = row
            .details
            .as_deref()
            .expect("details json present on proxy audit row");
        serde_json::from_str(raw).expect("details is valid JSON")
    }

    #[tokio::test]
    async fn sqlite_audit_persists_openai_success_row() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({"id": "chatcmpl-1"}),
            retry_after: None,
        })
        .await;
        let db = open_audit_db();
        let state = make_persistent_state(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "upstream-key".into(),
            }),
            None,
            Arc::new(NoopBudgetGate),
            Arc::clone(&db),
        );
        let app = chat_route_persistent(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "gpt-4"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let rows = read_audit_rows(&db).await;
        assert_eq!(rows.len(), 1, "expected exactly one persisted audit row");
        assert_eq!(rows[0].event_type, "inference_proxy_call");
        assert_eq!(rows[0].actor_kind, "inference_proxy");
        assert_eq!(rows[0].actor_id, "anonymous");
        let details = parse_details(&rows[0]);
        assert_eq!(details["outcome"], "success");
        assert_eq!(details["status"], 200);
        assert_eq!(details["model_requested"], "gpt-4");
        assert_eq!(details["stream"], false);
        assert_eq!(details["provider_base_url"], mock.base_url);
    }

    #[tokio::test]
    async fn sqlite_audit_persists_anthropic_success_row() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({"id": "msg_01"}),
            retry_after: None,
        })
        .await;
        let db = open_audit_db();
        let state = make_persistent_state(
            None,
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "upstream-key".into(),
            }),
            Arc::new(NoopBudgetGate),
            Arc::clone(&db),
        );
        let app = messages_route_persistent(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/messages")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "claude-3-5-sonnet"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let rows = read_audit_rows(&db).await;
        assert_eq!(rows.len(), 1);
        let details = parse_details(&rows[0]);
        assert_eq!(details["outcome"], "success");
        assert_eq!(details["model_requested"], "claude-3-5-sonnet");
    }

    #[tokio::test]
    async fn sqlite_audit_persists_budget_denied_row_and_skips_upstream() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::OK,
            body: serde_json::json!({"ok": true}),
            retry_after: None,
        })
        .await;
        let db = open_audit_db();
        let state = make_persistent_state(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "k".into(),
            }),
            None,
            Arc::new(DenyingGate {
                reason: "monthly budget exhausted".into(),
                retry_after: Some(Duration::from_secs(60)),
            }),
            Arc::clone(&db),
        );
        let app = chat_route_persistent(Arc::clone(&state));

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

        // Upstream must not have been called when the gate denied.
        assert!(mock.captured.lock().unwrap().body.is_empty());

        let rows = read_audit_rows(&db).await;
        assert_eq!(rows.len(), 1);
        let details = parse_details(&rows[0]);
        assert_eq!(details["outcome"], "budget_exceeded");
        assert_eq!(details["status"], 429);
    }

    #[tokio::test]
    async fn sqlite_audit_persists_no_upstream_row() {
        let db = open_audit_db();
        let state =
            make_persistent_state(None, None, None, Arc::new(NoopBudgetGate), Arc::clone(&db));
        let app = chat_route_persistent(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "gpt-4"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        let rows = read_audit_rows(&db).await;
        assert_eq!(rows.len(), 1);
        let details = parse_details(&rows[0]);
        assert_eq!(details["outcome"], "no_upstream");
        assert_eq!(details["status"], 503);
        // No upstream resolved → empty provider_base_url, not a leak.
        assert_eq!(details["provider_base_url"], "");
    }

    #[tokio::test]
    async fn sqlite_audit_persists_upstream_error_row() {
        let mock = start_mock_upstream(MockResponse::Json {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: serde_json::json!({"error": {"message": "boom"}}),
            retry_after: None,
        })
        .await;
        let db = open_audit_db();
        let state = make_persistent_state(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "k".into(),
            }),
            None,
            Arc::new(NoopBudgetGate),
            Arc::clone(&db),
        );
        let app = chat_route_persistent(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "gpt-4"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let rows = read_audit_rows(&db).await;
        assert_eq!(rows.len(), 1);
        let details = parse_details(&rows[0]);
        assert_eq!(details["outcome"], "provider_error");
        assert_eq!(details["status"], 500);
    }

    #[tokio::test]
    async fn sqlite_audit_persists_one_row_for_streaming_call() {
        let mock = start_mock_upstream(MockResponse::SseChunks(vec![
            r#"{"choices":[{"delta":{"content":"hi"}}]}"#,
            "[DONE]",
        ]))
        .await;
        let db = open_audit_db();
        let state = make_persistent_state(
            None,
            Some(UpstreamProvider {
                base_url: mock.base_url.clone(),
                api_key: "k".into(),
            }),
            None,
            Arc::new(NoopBudgetGate),
            Arc::clone(&db),
        );
        let app = chat_route_persistent(Arc::clone(&state));

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
        // Drain the SSE body so the upstream future completes; the audit row
        // is emitted synchronously after the upstream response is converted
        // to a stream and before the body is returned to the caller.
        let _ = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();

        let rows = read_audit_rows(&db).await;
        assert_eq!(rows.len(), 1, "streaming must emit exactly one audit row");
        let details = parse_details(&rows[0]);
        assert_eq!(details["outcome"], "success");
        assert_eq!(details["stream"], true);
    }

    #[tokio::test]
    async fn sqlite_audit_does_not_persist_for_unauthenticated_caller() {
        // Existing 401 path must remain audit-silent — the durable sink
        // must not change that contract.
        let db = open_audit_db();
        let state = make_persistent_state(
            Some("expected-key"),
            Some(UpstreamProvider {
                base_url: "http://127.0.0.1:1/v1".into(),
                api_key: "u".into(),
            }),
            None,
            Arc::new(NoopBudgetGate),
            Arc::clone(&db),
        );
        let app = chat_route_persistent(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json_body(serde_json::json!({"model": "gpt-4"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let rows = read_audit_rows(&db).await;
        assert!(
            rows.is_empty(),
            "unauthenticated callers must not produce a persisted audit row"
        );
    }
}
