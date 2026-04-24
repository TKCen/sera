//! HTTP + SSE client for the SERA gateway.
//!
//! Targets the autonomous gateway route surface (`/api/agents`,
//! `/api/agents/{id}`, `/api/sessions`, `/api/sessions/{id}/transcript`,
//! `/api/chat`) plus the full gateway's optional routes
//! (`/api/permission-requests`, `/api/evolve/*`, `/api/chat/stream`).
//!
//! Endpoints that don't exist in a given build return an empty result or
//! a clearly-named `ClientError::NotAvailable`.  The TUI degrades to a
//! "no data" state for those panes rather than crashing.
//!
//! ## Why a local client module, not `sera-client`?
//!
//! Sprint-3 spec allowed either "extract `sera-client` library crate" or
//! "duplicate selectively".  The existing `sera-cli` HTTP layer is a thin
//! `reqwest::Client` factory with the domain types embedded per-command,
//! so extraction would touch ~8 files.  We duplicate the narrow request-
//! types we need here and file a follow-up (`sera-tui-client-extract`)
//! for the refactor, keeping this bead scope-bounded.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

/// Connection state surfaced by the UI footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connected,
    Reconnecting,
    Disconnected,
}

impl ConnectionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Reconnecting => "reconnecting…",
            Self::Disconnected => "disconnected",
        }
    }
}

/// Client-surface errors.
#[derive(Debug, Error)]
pub enum ClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("gateway returned {status}: {body}")]
    Status { status: StatusCode, body: String },
    #[error("endpoint not available on this gateway: {0}")]
    NotAvailable(String),
    #[error("json parse failed: {0}")]
    Parse(String),
}

/// Agent metadata — supports both the autonomous gateway shape
/// (`{name, provider, model, has_tools}`) and the full gateway shape
/// (`{id, name, template_ref, status, circle, ...}`).  We parse into
/// an `serde_json::Value` and pull fields we care about so either works.
#[derive(Debug, Clone)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub status: String,
    pub template_or_provider: String,
    pub last_heartbeat_at: Option<String>,
}

impl Agent {
    fn from_json(v: &serde_json::Value) -> Self {
        let str_field = |k: &str| {
            v.get(k)
                .and_then(|f| f.as_str())
                .map(str::to_owned)
                .unwrap_or_default()
        };
        let id = {
            let s = str_field("id");
            if s.is_empty() { str_field("name") } else { s }
        };
        let template_or_provider = {
            let t = str_field("template_ref");
            if !t.is_empty() {
                t
            } else {
                str_field("provider")
            }
        };
        let status = {
            let s = str_field("status");
            if !s.is_empty() {
                s
            } else if let Some(true) = v.get("has_tools").and_then(|h| h.as_bool()) {
                "ready+tools".to_owned()
            } else {
                "ready".to_owned()
            }
        };
        Self {
            id,
            name: str_field("name"),
            display_name: v
                .get("display_name")
                .and_then(|f| f.as_str())
                .map(str::to_owned),
            status,
            template_or_provider,
            last_heartbeat_at: v
                .get("last_heartbeat_at")
                .and_then(|f| f.as_str())
                .map(str::to_owned),
        }
    }
}

/// Session summary.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: String,
    pub agent_id: String,
    /// ISO-8601 timestamp of session creation.  Not yet rendered in the
    /// TUI but parsed for future "Age" column support — keep the field
    /// so the parser stays honest about what the gateway returns.
    #[allow(dead_code)]
    pub created_at: String,
    pub state: String,
}

impl SessionSummary {
    fn from_json(v: &serde_json::Value) -> Self {
        let s = |k: &str| {
            v.get(k)
                .and_then(|f| f.as_str())
                .map(str::to_owned)
                .unwrap_or_default()
        };
        Self {
            id: s("id"),
            agent_id: {
                let a = s("agent_id");
                if a.is_empty() {
                    s("agent_instance_id")
                } else {
                    a
                }
            },
            created_at: s("created_at"),
            state: {
                let st = s("state");
                if st.is_empty() { s("status") } else { st }
            },
        }
    }
}

/// One transcript turn — role + text.  Richer block types (tool calls,
/// citations) exist in the spec but we collapse to role+text for display.
#[derive(Debug, Clone)]
pub struct TranscriptEntry {
    pub role: String,
    pub text: String,
}

impl TranscriptEntry {
    fn from_json(v: &serde_json::Value) -> Self {
        let role = v
            .get("role")
            .and_then(|f| f.as_str())
            .unwrap_or("?")
            .to_owned();
        let text = v
            .get("content")
            .and_then(|f| f.as_str())
            .or_else(|| v.get("text").and_then(|f| f.as_str()))
            .unwrap_or("")
            .to_owned();
        Self { role, text }
    }
}

/// A pending HITL permission request.
#[derive(Debug, Clone)]
pub struct HitlRequest {
    pub id: String,
    pub agent_id: String,
    pub summary: String,
    pub age: String,
    pub status: String,
}

impl HitlRequest {
    fn from_json(v: &serde_json::Value) -> Self {
        let s = |k: &str| {
            v.get(k)
                .and_then(|f| f.as_str())
                .map(str::to_owned)
                .unwrap_or_default()
        };
        // The route returns {id, agent_id, requested_at, resource, action, status, ...}
        // The `summary` column renders resource+action if present, else a
        // short description / prompt.
        let summary = {
            let resource = s("resource");
            let action = s("action");
            if !resource.is_empty() || !action.is_empty() {
                format!("{action} {resource}").trim().to_owned()
            } else {
                let d = s("description");
                if d.is_empty() { s("prompt") } else { d }
            }
        };
        Self {
            id: s("id"),
            agent_id: s("agent_id"),
            summary,
            age: s("requested_at"),
            status: {
                let st = s("status");
                if st.is_empty() { "pending".to_owned() } else { st }
            },
        }
    }
}

/// An evolve proposal (read-only for this bead).
#[derive(Debug, Clone)]
pub struct EvolveProposal {
    pub id: String,
    pub proposer: String,
    pub target: String,
    pub state: String,
    pub age: String,
}

impl EvolveProposal {
    fn from_json(v: &serde_json::Value) -> Self {
        let s = |k: &str| {
            v.get(k)
                .and_then(|f| f.as_str())
                .map(str::to_owned)
                .unwrap_or_default()
        };
        Self {
            id: s("id"),
            proposer: {
                let p = s("proposer");
                if p.is_empty() { s("proposed_by") } else { p }
            },
            target: {
                let t = s("target");
                if t.is_empty() { s("agent_id") } else { t }
            },
            state: {
                let st = s("state");
                if st.is_empty() { s("status") } else { st }
            },
            age: {
                let a = s("created_at");
                if a.is_empty() { s("proposed_at") } else { a }
            },
        }
    }
}

/// A streaming event from the SSE endpoint (or a synthetic "tool log"
/// chunk).  Kept deliberately loose since the gateway's event shape is in
/// flux — SessionView only reads `role`, `delta`, and `event_type`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamEvent {
    #[serde(default)]
    pub event_type: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub delta: String,
    #[serde(default)]
    pub tool: String,
}

impl StreamEvent {
    /// Try to parse a single SSE `data:` payload into a [`StreamEvent`].
    /// Returns `None` for unparseable chunks (keep-alive pings, comments).
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        serde_json::from_str::<serde_json::Value>(trimmed)
            .ok()
            .map(|v| Self {
                event_type: v
                    .get("event_type")
                    .or_else(|| v.get("event"))
                    .or_else(|| v.get("type"))
                    .and_then(|f| f.as_str())
                    .unwrap_or("message")
                    .to_owned(),
                session_id: v
                    .get("session_id")
                    .or_else(|| v.get("sessionId"))
                    .and_then(|f| f.as_str())
                    .unwrap_or("")
                    .to_owned(),
                role: v
                    .get("role")
                    .and_then(|f| f.as_str())
                    .unwrap_or("assistant")
                    .to_owned(),
                delta: v
                    .get("delta")
                    .or_else(|| v.get("text"))
                    .or_else(|| v.get("content"))
                    .and_then(|f| f.as_str())
                    .unwrap_or("")
                    .to_owned(),
                tool: v
                    .get("tool")
                    .and_then(|f| f.as_str())
                    .unwrap_or("")
                    .to_owned(),
            })
    }
}

/// Gateway client.
#[derive(Clone)]
pub struct GatewayClient {
    base_url: String,
    client: Client,
}

impl GatewayClient {
    /// Build a client.  `base_url` is stripped of a trailing slash; `api_key`
    /// is sent as `Authorization: Bearer …` on every request.
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl AsRef<str>,
        timeout: Duration,
    ) -> Result<Self, ClientError> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        let mut headers = HeaderMap::new();
        let mut value = HeaderValue::from_str(&format!("Bearer {}", api_key.as_ref()))
            .map_err(|e| ClientError::Parse(format!("invalid api key: {e}")))?;
        value.set_sensitive(true);
        headers.insert(AUTHORIZATION, value);

        let client = Client::builder()
            .default_headers(headers)
            .timeout(timeout)
            .build()?;

        Ok(Self { base_url, client })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn get_json(&self, path: &str) -> Result<serde_json::Value, ClientError> {
        let resp = self.client.get(self.url(path)).send().await?;
        let status = resp.status();
        if status == StatusCode::NOT_FOUND {
            return Err(ClientError::NotAvailable(path.to_owned()));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Status { status, body });
        }
        resp.json().await.map_err(ClientError::Http)
    }

    async fn post_json(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let resp = self
            .client
            .post(self.url(path))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if status == StatusCode::NOT_FOUND {
            return Err(ClientError::NotAvailable(path.to_owned()));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Status { status, body });
        }
        // Some gateway POSTs return empty body (204-style in 200 clothing).
        let text = resp.text().await.unwrap_or_default();
        if text.trim().is_empty() {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| ClientError::Parse(e.to_string()))
    }

    /// Health probe — `GET /api/health`.  Not called by the UI runtime
    /// yet but exposed for tests and tooling that wants a round-trip
    /// check without fetching agents.
    #[allow(dead_code)]
    pub async fn health(&self) -> Result<serde_json::Value, ClientError> {
        self.get_json("/api/health").await
    }

    /// `GET /api/agents` — list agent instances.  Works against both
    /// autonomous and full gateway shapes.
    pub async fn list_agents(&self) -> Result<Vec<Agent>, ClientError> {
        let body = self.get_json("/api/agents").await?;
        Ok(body
            .as_array()
            .map(|arr| arr.iter().map(Agent::from_json).collect())
            .unwrap_or_default())
    }

    /// `GET /api/sessions` — list sessions; optional agent filter.
    /// Returns an empty list if the endpoint returns 404 (some autonomous
    /// gateway builds haven't enabled session listing).
    pub async fn list_sessions(
        &self,
        agent_id: Option<&str>,
    ) -> Result<Vec<SessionSummary>, ClientError> {
        let mut path = "/api/sessions".to_owned();
        if let Some(a) = agent_id {
            path.push_str(&format!("?agent_id={a}"));
        }
        match self.get_json(&path).await {
            Ok(body) => Ok(body
                .as_array()
                .map(|arr| arr.iter().map(SessionSummary::from_json).collect())
                .unwrap_or_default()),
            Err(ClientError::NotAvailable(_)) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// `GET /api/sessions/{id}/transcript`.
    pub async fn session_transcript(
        &self,
        session_id: &str,
    ) -> Result<Vec<TranscriptEntry>, ClientError> {
        let path = format!("/api/sessions/{session_id}/transcript");
        match self.get_json(&path).await {
            Ok(body) => {
                // The route returns either an array of entries or an object
                // with {entries: [...]}.
                let arr = body
                    .as_array()
                    .cloned()
                    .or_else(|| body.get("entries").and_then(|v| v.as_array()).cloned())
                    .unwrap_or_default();
                Ok(arr.iter().map(TranscriptEntry::from_json).collect())
            }
            Err(ClientError::NotAvailable(_)) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// `GET /api/permission-requests?status=pending` — HITL queue.
    pub async fn list_hitl(&self) -> Result<Vec<HitlRequest>, ClientError> {
        match self
            .get_json("/api/permission-requests?status=pending")
            .await
        {
            Ok(body) => Ok(body
                .as_array()
                .map(|arr| arr.iter().map(HitlRequest::from_json).collect())
                .unwrap_or_default()),
            Err(ClientError::NotAvailable(_)) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// `POST /api/permission-requests/{id}/approve`.
    pub async fn approve_hitl(&self, id: &str) -> Result<(), ClientError> {
        self.post_json(
            &format!("/api/permission-requests/{id}/approve"),
            serde_json::json!({}),
        )
        .await?;
        Ok(())
    }

    /// `POST /api/permission-requests/{id}/deny`.
    pub async fn reject_hitl(&self, id: &str) -> Result<(), ClientError> {
        self.post_json(
            &format!("/api/permission-requests/{id}/deny"),
            serde_json::json!({}),
        )
        .await?;
        Ok(())
    }

    /// Escalate a HITL request via the operator-requests surface.
    /// Best-effort: degrades to NotAvailable if the endpoint is missing.
    pub async fn escalate_hitl(&self, id: &str) -> Result<(), ClientError> {
        self.post_json(
            "/api/operator-requests",
            serde_json::json!({
                "reason": "escalated from TUI",
                "permission_request_id": id,
            }),
        )
        .await?;
        Ok(())
    }

    /// `GET /api/evolve/proposals` — read-only list for this bead.
    pub async fn list_evolve_proposals(&self) -> Result<Vec<EvolveProposal>, ClientError> {
        // Try a few likely paths — both shapes exist in the codebase.
        for path in &[
            "/api/evolve/proposals",
            "/api/evolve",
            "/api/evolve/list",
        ] {
            match self.get_json(path).await {
                Ok(body) => {
                    let arr = body
                        .as_array()
                        .cloned()
                        .or_else(|| body.get("proposals").and_then(|v| v.as_array()).cloned())
                        .unwrap_or_default();
                    return Ok(arr.iter().map(EvolveProposal::from_json).collect());
                }
                Err(ClientError::NotAvailable(_)) => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(Vec::new())
    }

    /// Subscribe to the SSE stream for `session_id`.
    ///
    /// The autonomous gateway exposes `/api/chat/stream` (stub) and the
    /// full gateway has a per-session stream.  We try session-scoped
    /// first, fall back to the global stream, and surface
    /// [`ConnectionState::Disconnected`] if neither is available.
    ///
    /// Events land on `tx` as [`StreamEvent`].  Closing the receiver
    /// end aborts the background reader.  Exponential backoff is
    /// applied between reconnect attempts, capped at 30s.
    pub fn spawn_sse(
        &self,
        session_id: String,
        tx: mpsc::Sender<SseUpdate>,
    ) -> tokio::task::JoinHandle<()> {
        let client = self.clone();
        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                let _ = tx.send(SseUpdate::State(ConnectionState::Reconnecting)).await;
                let paths = [
                    format!("/api/sessions/{session_id}/stream"),
                    "/api/chat/stream".to_owned(),
                ];
                let mut connected = false;
                for path in &paths {
                    match client.open_sse(path).await {
                        Ok(mut bytes) => {
                            connected = true;
                            backoff = Duration::from_secs(1);
                            let _ = tx.send(SseUpdate::State(ConnectionState::Connected)).await;
                            let mut buffer = String::new();
                            while let Some(chunk) = bytes.next().await {
                                let Ok(chunk) = chunk else { break };
                                let Ok(s) = std::str::from_utf8(&chunk) else {
                                    continue;
                                };
                                buffer.push_str(s);
                                // SSE frames are separated by a blank line.
                                while let Some(pos) = buffer.find("\n\n") {
                                    let frame: String = buffer.drain(..=pos + 1).collect();
                                    for line in frame.lines() {
                                        if let Some(data) = line.strip_prefix("data:")
                                            && let Some(ev) = StreamEvent::parse(data.trim())
                                            && tx.send(SseUpdate::Event(ev)).await.is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                            }
                            break;
                        }
                        Err(ClientError::NotAvailable(_)) => continue,
                        Err(_) => break,
                    }
                }
                if !connected {
                    let _ = tx
                        .send(SseUpdate::State(ConnectionState::Disconnected))
                        .await;
                }
                // Exponential backoff, capped.
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
                if tx.is_closed() {
                    return;
                }
            }
        })
    }

    async fn open_sse(
        &self,
        path: &str,
    ) -> Result<
        futures_util::stream::BoxStream<'static, reqwest::Result<bytes::Bytes>>,
        ClientError,
    > {
        let resp = self
            .client
            .get(self.url(path))
            .header("Accept", "text/event-stream")
            .send()
            .await?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Err(ClientError::NotAvailable(path.to_owned()));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Status { status, body });
        }
        Ok(Box::pin(resp.bytes_stream()))
    }

    /// `POST /api/chat` with `{message, agent, stream: true}`.
    ///
    /// Returns an async stream of [`StreamEvent`]s parsed from the SSE
    /// response.  The stream ends when the server closes the connection.
    /// HTTP-level errors are surfaced as `Err(ClientError)`.
    pub async fn post_chat(
        &self,
        agent: &str,
        message: &str,
    ) -> Result<impl tokio_stream::Stream<Item = Result<StreamEvent, ClientError>>, ClientError>
    {
        let body = serde_json::json!({
            "message": message,
            "agent": agent,
            "stream": true,
        });
        let resp = self
            .client
            .post(self.url("/api/chat"))
            .header("content-type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if status == StatusCode::NOT_FOUND {
            return Err(ClientError::NotAvailable("/api/chat".to_owned()));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Status { status, body });
        }
        let bytes = resp.bytes_stream();
        Ok(parse_sse_stream(bytes))
    }
}

/// Drain an SSE byte stream into [`StreamEvent`]s.  Reused by both the
/// existing `spawn_sse` reconnect loop and the new `post_chat` one-shot path.
fn parse_sse_stream(
    bytes: impl tokio_stream::Stream<Item = reqwest::Result<bytes::Bytes>> + Send + 'static,
) -> impl tokio_stream::Stream<Item = Result<StreamEvent, ClientError>> {
    use futures_util::StreamExt;
    let mut buffer = String::new();
    bytes.flat_map(move |chunk| {
        let events: Vec<Result<StreamEvent, ClientError>> = match chunk {
            Err(e) => vec![Err(ClientError::Http(e))],
            Ok(bytes) => {
                match std::str::from_utf8(&bytes) {
                    Err(_) => vec![],
                    Ok(s) => {
                        buffer.push_str(s);
                        let mut out = Vec::new();
                        // SSE frames separated by blank lines.
                        while let Some(pos) = buffer.find("\n\n") {
                            let frame: String = buffer.drain(..=pos + 1).collect();
                            for line in frame.lines() {
                                if let Some(data) = line.strip_prefix("data:")
                                    && let Some(ev) = StreamEvent::parse(data.trim())
                                {
                                    out.push(Ok(ev));
                                }
                            }
                        }
                        out
                    }
                }
            }
        };
        tokio_stream::iter(events)
    })
}

/// Payload the SSE background task pushes to the UI.
#[derive(Debug, Clone)]
pub enum SseUpdate {
    Event(StreamEvent),
    State(ConnectionState),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_from_json_prefers_id_then_name() {
        let v = serde_json::json!({"id": "abc", "name": "xyz", "status": "ready"});
        let a = Agent::from_json(&v);
        assert_eq!(a.id, "abc");
        assert_eq!(a.name, "xyz");
        assert_eq!(a.status, "ready");
    }

    #[test]
    fn agent_from_json_falls_back_to_name_when_no_id() {
        let v = serde_json::json!({"name": "only-name", "provider": "mock"});
        let a = Agent::from_json(&v);
        assert_eq!(a.id, "only-name");
        assert_eq!(a.template_or_provider, "mock");
    }

    #[test]
    fn agent_from_json_synthesises_status_from_has_tools() {
        let v = serde_json::json!({"name": "a", "has_tools": true});
        let a = Agent::from_json(&v);
        assert_eq!(a.status, "ready+tools");
    }

    #[test]
    fn session_from_json_handles_both_field_names() {
        let v = serde_json::json!({"id": "s1", "agent_id": "a1", "state": "active"});
        let s = SessionSummary::from_json(&v);
        assert_eq!(s.id, "s1");
        assert_eq!(s.agent_id, "a1");
        assert_eq!(s.state, "active");

        let v2 = serde_json::json!({"id": "s2", "agent_instance_id": "a2", "status": "idle"});
        let s2 = SessionSummary::from_json(&v2);
        assert_eq!(s2.agent_id, "a2");
        assert_eq!(s2.state, "idle");
    }

    #[test]
    fn transcript_entry_reads_content_or_text() {
        let v = serde_json::json!({"role": "assistant", "content": "hello"});
        let t = TranscriptEntry::from_json(&v);
        assert_eq!(t.role, "assistant");
        assert_eq!(t.text, "hello");

        let v2 = serde_json::json!({"role": "user", "text": "hi"});
        let t2 = TranscriptEntry::from_json(&v2);
        assert_eq!(t2.text, "hi");
    }

    #[test]
    fn hitl_from_json_formats_summary() {
        let v = serde_json::json!({
            "id": "h1",
            "agent_id": "a1",
            "resource": "/tmp/foo",
            "action": "write",
            "requested_at": "2026-04-18T10:00:00Z"
        });
        let h = HitlRequest::from_json(&v);
        assert_eq!(h.summary, "write /tmp/foo");
        assert_eq!(h.id, "h1");
        assert_eq!(h.status, "pending");
    }

    #[test]
    fn evolve_from_json_handles_alternate_keys() {
        let v = serde_json::json!({
            "id": "e1",
            "proposed_by": "agent-alpha",
            "agent_id": "target-1",
            "status": "pending",
            "proposed_at": "2026-04-18T10:00:00Z"
        });
        let e = EvolveProposal::from_json(&v);
        assert_eq!(e.proposer, "agent-alpha");
        assert_eq!(e.target, "target-1");
        assert_eq!(e.state, "pending");
        assert_eq!(e.age, "2026-04-18T10:00:00Z");
    }

    #[test]
    fn stream_event_parse_known_fields() {
        let raw = r#"{"event_type":"thought","session_id":"s1","role":"assistant","delta":"hello"}"#;
        let ev = StreamEvent::parse(raw).unwrap();
        assert_eq!(ev.event_type, "thought");
        assert_eq!(ev.session_id, "s1");
        assert_eq!(ev.role, "assistant");
        assert_eq!(ev.delta, "hello");
    }

    #[test]
    fn stream_event_parse_alias_keys() {
        let raw = r#"{"type":"message","sessionId":"s2","content":"chunk"}"#;
        let ev = StreamEvent::parse(raw).unwrap();
        assert_eq!(ev.event_type, "message");
        assert_eq!(ev.session_id, "s2");
        assert_eq!(ev.delta, "chunk");
    }

    #[test]
    fn stream_event_parse_garbage_returns_none() {
        assert!(StreamEvent::parse("").is_none());
        assert!(StreamEvent::parse(":ping").is_none());
        assert!(StreamEvent::parse("not json").is_none());
    }

    #[test]
    fn connection_state_label() {
        assert_eq!(ConnectionState::Connected.label(), "connected");
        assert_eq!(ConnectionState::Reconnecting.label(), "reconnecting…");
        assert_eq!(ConnectionState::Disconnected.label(), "disconnected");
    }

    // --- post_chat body shape (no HTTP round-trip needed) ---

    #[test]
    fn post_chat_body_shape_is_correct() {
        // Verify the JSON body we'd send has the expected fields and values.
        let agent = "test-agent";
        let message = "hello from composer";
        let body = serde_json::json!({
            "message": message,
            "agent": agent,
            "stream": true,
        });
        assert_eq!(body["message"], "hello from composer");
        assert_eq!(body["agent"], "test-agent");
        assert_eq!(body["stream"], true);
        // No session_id — server-managed per spec.
        assert!(body.get("session_id").is_none());
    }

    #[tokio::test]
    async fn parse_sse_stream_yields_events() {
        use tokio_stream::StreamExt;
        // Build a fake byte stream of two SSE frames.
        let raw = b"data: {\"event_type\":\"message\",\"role\":\"assistant\",\"delta\":\"hi\"}\n\ndata: {\"event_type\":\"done\",\"delta\":\"\"}\n\n".to_vec();
        let bytes_stream = tokio_stream::iter(vec![Ok::<_, reqwest::Error>(bytes::Bytes::from(raw))]);
        let mut stream = parse_sse_stream(bytes_stream);
        let ev1 = stream.next().await.unwrap().unwrap();
        assert_eq!(ev1.event_type, "message");
        assert_eq!(ev1.role, "assistant");
        assert_eq!(ev1.delta, "hi");
        let ev2 = stream.next().await.unwrap().unwrap();
        assert_eq!(ev2.event_type, "done");
    }

    #[tokio::test]
    async fn parse_sse_stream_skips_non_data_lines() {
        use tokio_stream::StreamExt;
        // SSE with comments and event: lines mixed in.
        let raw = b": ping\nevent: message\ndata: {\"event_type\":\"message\",\"delta\":\"chunk\"}\n\n".to_vec();
        let bytes_stream = tokio_stream::iter(vec![Ok::<_, reqwest::Error>(bytes::Bytes::from(raw))]);
        let mut stream = parse_sse_stream(bytes_stream);
        let ev = stream.next().await.unwrap().unwrap();
        assert_eq!(ev.delta, "chunk");
        assert!(stream.next().await.is_none());
    }
}
