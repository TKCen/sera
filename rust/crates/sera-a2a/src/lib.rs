//! sera-a2a — A2A (Agent-to-Agent) protocol adapter.
//!
//! Vendored types from the [A2A specification](https://github.com/a2aproject/A2A).
//! SERA agents can discover, delegate to, and receive delegations from external
//! A2A agents. The adapter converts between A2A task format and SERA's
//! internal event model.
//!
//! ## Feature flags
//!
//! - `acp-compat`: enables the legacy ACP message shape translator for
//!   operators migrating from the retired IBM/BeeAI ACP protocol (merged
//!   into A2A on 2025-08-25). See SPEC-interop §5.
//!
//! See SPEC-interop §4 for the full protocol specification.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sera_errors::{SeraError, SeraErrorCode};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum A2aError {
    #[error("discovery failed: {reason}")]
    DiscoveryFailed { reason: String },
    #[error("task delegation failed: {reason}")]
    DelegationFailed { reason: String },
    #[error("agent not found: {agent_id}")]
    AgentNotFound { agent_id: String },
    #[error("protocol error: {reason}")]
    Protocol { reason: String },
    #[error("serialization error: {reason}")]
    Serialization { reason: String },
    #[error("unauthorized: {reason}")]
    Unauthorized { reason: String },
}

impl From<A2aError> for SeraError {
    fn from(err: A2aError) -> Self {
        let code = match &err {
            A2aError::DiscoveryFailed { .. } => SeraErrorCode::Unavailable,
            A2aError::DelegationFailed { .. } => SeraErrorCode::Internal,
            A2aError::AgentNotFound { .. } => SeraErrorCode::NotFound,
            A2aError::Protocol { .. } => SeraErrorCode::Internal,
            A2aError::Serialization { .. } => SeraErrorCode::Serialization,
            A2aError::Unauthorized { .. } => SeraErrorCode::Unauthorized,
        };
        SeraError::new(code, err.to_string())
    }
}

// ---------------------------------------------------------------------------
// Vendored A2A types (from a2aproject/A2A specification)
// ---------------------------------------------------------------------------

/// An A2A Agent Card describes an agent's capabilities and endpoint.
///
/// Vendored from `a2aproject/A2A` specification — the canonical discovery
/// document that A2A agents publish at `/.well-known/agent.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCard {
    pub name: String,
    pub description: String,
    pub url: String,
    #[serde(default)]
    pub skills: Vec<AgentSkill>,
    #[serde(default)]
    pub authentication: Option<AuthenticationInfo>,
    pub version: String,
}

/// A skill advertised by an A2A agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub input_modes: Vec<String>,
    #[serde(default)]
    pub output_modes: Vec<String>,
}

/// Authentication info from the Agent Card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationInfo {
    #[serde(rename = "type")]
    pub auth_type: String,
    #[serde(default)]
    pub credentials: Option<String>,
}

/// A2A Task — the central unit of work in the A2A protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2ATask {
    pub id: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub history: Vec<Message>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Task lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Submitted,
    Working,
    InputRequired,
    Completed,
    Canceled,
    Failed,
    Unknown,
}

/// An artifact produced by a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub parts: Vec<Part>,
    #[serde(default)]
    pub index: u32,
}

/// A content part within a message or artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Part {
    Text { text: String },
    File { file: FileContent },
    Data { data: serde_json::Value },
}

/// File content (inline or URI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    pub name: Option<String>,
    pub mime_type: Option<String>,
    /// Base64-encoded bytes if inline.
    pub bytes: Option<String>,
    /// URI if external.
    pub uri: Option<String>,
}

/// A2A message exchanged between agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub parts: Vec<Part>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Role of the message sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Agent,
}

// ---------------------------------------------------------------------------
// JSON-RPC request/response wrappers
// ---------------------------------------------------------------------------

/// A2A JSON-RPC request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aRequest {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// A2A JSON-RPC response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aResponse {
    pub jsonrpc: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<A2aRpcError>,
}

/// A2A JSON-RPC error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Adapter trait
// ---------------------------------------------------------------------------

/// Adapter for A2A protocol interoperability.
///
/// Converts between A2A task format and SERA's internal event model.
/// External A2A agents are registered as `ExternalAgentPrincipal` in
/// SERA's principal registry.
#[async_trait]
pub trait A2aAdapter: Send + Sync + 'static {
    /// Discover external A2A agents at the given endpoint.
    async fn discover(&self, endpoint: &str) -> Result<Vec<AgentCard>, A2aError>;

    /// Send a task to an external A2A agent.
    async fn send_task(&self, agent_url: &str, task: &A2ATask) -> Result<A2ATask, A2aError>;

    /// Get the status of a previously delegated task.
    async fn get_task(&self, agent_url: &str, task_id: &str) -> Result<A2ATask, A2aError>;

    /// Cancel a previously delegated task.
    async fn cancel_task(&self, agent_url: &str, task_id: &str) -> Result<A2ATask, A2aError>;
}

/// SERA's A2A agent card builder — produces the card SERA publishes
/// at `/.well-known/agent.json`.
pub fn sera_agent_card(name: &str, url: &str, skills: Vec<AgentSkill>) -> AgentCard {
    AgentCard {
        name: name.to_owned(),
        description: format!("SERA agent: {name}"),
        url: url.to_owned(),
        skills,
        authentication: None,
        version: "1.0".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Transport + outbound client (SPEC-interop §4.3)
// ---------------------------------------------------------------------------

/// Transport abstraction for sending A2A JSON-RPC requests.
///
/// Implementations may wrap HTTP, stdio, or an in-memory loopback.
/// Keeping the transport injectable lets the `A2aClient` stay testable
/// without real network calls.
#[async_trait]
pub trait A2aTransport: Send + Sync + 'static {
    /// Send a serialized A2A request to the given endpoint URL and
    /// return the raw response. Implementations are responsible for
    /// framing (e.g. HTTP POST, stdio line-delimited JSON).
    async fn send(&self, endpoint: &str, request: &A2aRequest) -> Result<A2aResponse, A2aError>;
}

/// Standard A2A JSON-RPC method names (SPEC-interop §4 / A2A spec).
pub mod methods {
    pub const TASKS_SEND: &str = "tasks/send";
    pub const TASKS_GET: &str = "tasks/get";
    pub const TASKS_CANCEL: &str = "tasks/cancel";
}

/// Outbound A2A client. Serializes typed requests over a pluggable
/// transport and maps responses back into typed results.
///
/// Correlation ids are generated per-call (UUIDv4); the caller never
/// has to track them.
pub struct A2aClient {
    transport: std::sync::Arc<dyn A2aTransport>,
}

impl A2aClient {
    /// Build a client around the given transport.
    pub fn new<T: A2aTransport>(transport: T) -> Self {
        Self {
            transport: std::sync::Arc::new(transport),
        }
    }

    /// Build a client from an already-shared transport (useful when
    /// the same transport is reused across multiple clients).
    pub fn from_arc(transport: std::sync::Arc<dyn A2aTransport>) -> Self {
        Self { transport }
    }

    fn new_request(
        method: &str,
        params: serde_json::Value,
    ) -> Result<A2aRequest, A2aError> {
        Ok(A2aRequest {
            jsonrpc: "2.0".to_owned(),
            id: uuid::Uuid::new_v4().to_string(),
            method: method.to_owned(),
            params,
        })
    }

    async fn call<R: for<'de> Deserialize<'de>>(
        &self,
        endpoint: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<R, A2aError> {
        let req = Self::new_request(method, params)?;
        let resp = self.transport.send(endpoint, &req).await?;
        if resp.id != req.id {
            return Err(A2aError::Protocol {
                reason: format!(
                    "response id mismatch: expected {}, got {}",
                    req.id, resp.id
                ),
            });
        }
        if let Some(err) = resp.error {
            return Err(A2aError::Protocol {
                reason: format!("rpc error {}: {}", err.code, err.message),
            });
        }
        let result = resp.result.ok_or_else(|| A2aError::Protocol {
            reason: "response missing both result and error".to_owned(),
        })?;
        serde_json::from_value(result).map_err(|e| A2aError::Serialization {
            reason: e.to_string(),
        })
    }

    /// Send a task to an external A2A agent (`tasks/send`).
    pub async fn send_task(&self, endpoint: &str, task: &A2ATask) -> Result<A2ATask, A2aError> {
        let params = serde_json::to_value(task).map_err(|e| A2aError::Serialization {
            reason: e.to_string(),
        })?;
        self.call(endpoint, methods::TASKS_SEND, params).await
    }

    /// Fetch task status (`tasks/get`).
    pub async fn get_task(&self, endpoint: &str, task_id: &str) -> Result<A2ATask, A2aError> {
        let params = serde_json::json!({ "id": task_id });
        self.call(endpoint, methods::TASKS_GET, params).await
    }

    /// Cancel a task (`tasks/cancel`).
    pub async fn cancel_task(&self, endpoint: &str, task_id: &str) -> Result<A2ATask, A2aError> {
        let params = serde_json::json!({ "id": task_id });
        self.call(endpoint, methods::TASKS_CANCEL, params).await
    }
}

// ---------------------------------------------------------------------------
// Inbound router (SPEC-interop §4.3)
// ---------------------------------------------------------------------------

/// Inbound A2A router — dispatches incoming JSON-RPC requests to a
/// handler and produces the JSON-RPC response.
///
/// Implementations are expected to be cheap-to-clone / Arc-wrapped so
/// the gateway can share them across connections.
#[async_trait]
pub trait A2aRouter: Send + Sync + 'static {
    /// Handle a single request. Implementations must always return a
    /// response — transport-level failures are the caller's concern.
    async fn handle(&self, request: A2aRequest) -> A2aResponse;
}

type BoxHandler = Box<
    dyn Fn(A2aRequest) -> futures_core_like::BoxFuture<'static, Result<serde_json::Value, A2aError>>
        + Send
        + Sync,
>;

/// Minimal "BoxFuture" alias so we don't pull in `futures` for one type.
mod futures_core_like {
    use std::future::Future;
    use std::pin::Pin;
    pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
}

/// In-process router backed by a user-supplied closure.
///
/// Useful for:
/// - tests (loopback transport → router → closure)
/// - embedding SERA's own A2A handler inside the gateway
pub struct InProcRouter {
    handler: BoxHandler,
}

impl InProcRouter {
    /// Build a router from an async closure.
    pub fn new<F, Fut>(handler: F) -> Self
    where
        F: Fn(A2aRequest) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<serde_json::Value, A2aError>> + Send + 'static,
    {
        let handler: BoxHandler = Box::new(move |req| Box::pin(handler(req)));
        Self { handler }
    }

    fn make_error_response(id: String, err: &A2aError) -> A2aResponse {
        // JSON-RPC error codes: -32000..-32099 is reserved "server error".
        // Map our coarse A2aError variants onto stable codes.
        let code = match err {
            A2aError::AgentNotFound { .. } => -32004,
            A2aError::Unauthorized { .. } => -32001,
            A2aError::Serialization { .. } => -32700, // parse error
            A2aError::Protocol { .. } => -32600,      // invalid request
            A2aError::DiscoveryFailed { .. } | A2aError::DelegationFailed { .. } => -32000,
        };
        A2aResponse {
            jsonrpc: "2.0".to_owned(),
            id,
            result: None,
            error: Some(A2aRpcError {
                code,
                message: err.to_string(),
                data: None,
            }),
        }
    }
}

#[async_trait]
impl A2aRouter for InProcRouter {
    async fn handle(&self, request: A2aRequest) -> A2aResponse {
        let id = request.id.clone();
        match (self.handler)(request).await {
            Ok(result) => A2aResponse {
                jsonrpc: "2.0".to_owned(),
                id,
                result: Some(result),
                error: None,
            },
            Err(err) => Self::make_error_response(id, &err),
        }
    }
}

/// Loopback transport that forwards requests to an [`A2aRouter`] —
/// useful for tests and for embedding A2A between in-process components
/// without any network stack.
pub struct LoopbackTransport<R: A2aRouter> {
    router: std::sync::Arc<R>,
}

impl<R: A2aRouter> LoopbackTransport<R> {
    pub fn new(router: R) -> Self {
        Self {
            router: std::sync::Arc::new(router),
        }
    }

    pub fn from_arc(router: std::sync::Arc<R>) -> Self {
        Self { router }
    }
}

#[async_trait]
impl<R: A2aRouter> A2aTransport for LoopbackTransport<R> {
    async fn send(&self, _endpoint: &str, request: &A2aRequest) -> Result<A2aResponse, A2aError> {
        Ok(self.router.handle(request.clone()).await)
    }
}

// ---------------------------------------------------------------------------
// Capability negotiation (SPEC-interop §4)
// ---------------------------------------------------------------------------

/// A2A capability descriptor. Peers announce what optional features
/// they implement so callers can gate behaviour without runtime probes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capabilities {
    /// Flat list of supported capability ids (e.g. `tasks/send`,
    /// `streaming`, `cancel`, `push-notifications`).
    #[serde(default)]
    pub supported: Vec<String>,
}

impl Capabilities {
    /// Build from any iterable of capability ids.
    pub fn new<I, S>(caps: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            supported: caps.into_iter().map(Into::into).collect(),
        }
    }

    /// Returns `true` iff `cap` is present in the supported set.
    pub fn supports(&self, cap: &str) -> bool {
        self.supported.iter().any(|c| c == cap)
    }

    /// Intersection — useful when negotiating a common feature set
    /// between local and remote peers.
    pub fn intersect(&self, other: &Capabilities) -> Capabilities {
        let mut out: Vec<String> = self
            .supported
            .iter()
            .filter(|c| other.supports(c))
            .cloned()
            .collect();
        out.sort();
        out.dedup();
        Capabilities { supported: out }
    }
}

// ---------------------------------------------------------------------------
// ACP compatibility (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "acp-compat")]
pub mod acp_compat {
    //! Legacy ACP message shape translator.
    //!
    //! Accepts the retired ACP message format and converts it into A2A
    //! messages. This module is feature-gated behind `acp-compat` and
    //! intended for a 12-month transition window.
    //!
    //! See SPEC-interop §5 and SPEC-dependencies §10.16.

    use super::*;

    /// A minimal ACP message shape for compatibility translation.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct AcpMessage {
        pub sender: String,
        pub recipient: String,
        pub content: serde_json::Value,
        #[serde(default)]
        pub metadata: serde_json::Value,
    }

    /// Convert a legacy ACP message into an A2A Message.
    pub fn acp_to_a2a(msg: &AcpMessage) -> Message {
        Message {
            role: MessageRole::User,
            parts: vec![Part::Data {
                data: msg.content.clone(),
            }],
            metadata: msg.metadata.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_status_serde_roundtrip() {
        let s = TaskStatus::Working;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"working\"");
        let back: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn agent_card_serialize() {
        let card = sera_agent_card("test-agent", "http://localhost:8080", vec![]);
        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("test-agent"));
        assert!(json.contains("http://localhost:8080"));
    }

    #[test]
    fn part_text_serde() {
        let p = Part::Text {
            text: "hello".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        let back: Part = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, Part::Text { text } if text == "hello"));
    }

    #[test]
    fn a2a_error_to_sera_error() {
        let err = A2aError::AgentNotFound {
            agent_id: "x".into(),
        };
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::NotFound);
    }

    #[test]
    fn message_role_roundtrip() {
        let r = MessageRole::Agent;
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, "\"agent\"");
    }

    #[test]
    fn json_rpc_request_serde() {
        let req = A2aRequest {
            jsonrpc: "2.0".into(),
            id: "1".into(),
            method: "tasks/send".into(),
            params: serde_json::json!({}),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("tasks/send"));
    }

    // ---------- Transport + client ----------

    struct EchoTransport;

    #[async_trait]
    impl A2aTransport for EchoTransport {
        async fn send(
            &self,
            _endpoint: &str,
            request: &A2aRequest,
        ) -> Result<A2aResponse, A2aError> {
            // Echo back the params as the result so callers can assert
            // correlation and payload shape.
            Ok(A2aResponse {
                jsonrpc: "2.0".into(),
                id: request.id.clone(),
                result: Some(request.params.clone()),
                error: None,
            })
        }
    }

    struct FailingTransport;

    #[async_trait]
    impl A2aTransport for FailingTransport {
        async fn send(
            &self,
            _endpoint: &str,
            request: &A2aRequest,
        ) -> Result<A2aResponse, A2aError> {
            Ok(A2aResponse {
                jsonrpc: "2.0".into(),
                id: request.id.clone(),
                result: None,
                error: Some(A2aRpcError {
                    code: -32004,
                    message: "not found".into(),
                    data: None,
                }),
            })
        }
    }

    #[tokio::test]
    async fn client_send_task_roundtrips_payload() {
        let client = A2aClient::new(EchoTransport);
        let task = A2ATask {
            id: "t-1".into(),
            status: TaskStatus::Submitted,
            artifacts: vec![],
            history: vec![],
            metadata: serde_json::json!({}),
        };
        let out = client.send_task("http://peer", &task).await.unwrap();
        assert_eq!(out.id, "t-1");
        assert_eq!(out.status, TaskStatus::Submitted);
    }

    #[tokio::test]
    async fn client_maps_rpc_error_to_a2a_error() {
        let client = A2aClient::new(FailingTransport);
        let err = client.get_task("http://peer", "missing").await.unwrap_err();
        match err {
            A2aError::Protocol { reason } => assert!(reason.contains("not found")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn client_detects_correlation_id_mismatch() {
        struct BadIdTransport;
        #[async_trait]
        impl A2aTransport for BadIdTransport {
            async fn send(
                &self,
                _endpoint: &str,
                _request: &A2aRequest,
            ) -> Result<A2aResponse, A2aError> {
                Ok(A2aResponse {
                    jsonrpc: "2.0".into(),
                    id: "not-the-request-id".into(),
                    result: Some(serde_json::json!({})),
                    error: None,
                })
            }
        }
        let client = A2aClient::new(BadIdTransport);
        let err = client
            .get_task("http://peer", "whatever")
            .await
            .unwrap_err();
        assert!(matches!(err, A2aError::Protocol { .. }));
    }

    // ---------- Inbound router ----------

    #[tokio::test]
    async fn inproc_router_dispatches_to_closure() {
        let router = InProcRouter::new(|req: A2aRequest| async move {
            assert_eq!(req.method, methods::TASKS_SEND);
            Ok(serde_json::json!({ "ok": true }))
        });
        let resp = router
            .handle(A2aRequest {
                jsonrpc: "2.0".into(),
                id: "abc".into(),
                method: methods::TASKS_SEND.into(),
                params: serde_json::json!({}),
            })
            .await;
        assert_eq!(resp.id, "abc");
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["ok"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn inproc_router_maps_error_to_rpc_error() {
        let router = InProcRouter::new(|_req: A2aRequest| async move {
            Err(A2aError::AgentNotFound {
                agent_id: "x".into(),
            })
        });
        let resp = router
            .handle(A2aRequest {
                jsonrpc: "2.0".into(),
                id: "req-1".into(),
                method: methods::TASKS_GET.into(),
                params: serde_json::json!({}),
            })
            .await;
        assert!(resp.result.is_none());
        let err = resp.error.expect("error present");
        assert_eq!(err.code, -32004);
        assert!(err.message.contains("agent not found"));
    }

    #[tokio::test]
    async fn loopback_transport_roundtrip_through_router() {
        let router = InProcRouter::new(|req: A2aRequest| async move {
            // Bounce params back as the task result.
            Ok(req.params)
        });
        let client = A2aClient::new(LoopbackTransport::new(router));
        let task = A2ATask {
            id: "loop-1".into(),
            status: TaskStatus::Working,
            artifacts: vec![],
            history: vec![],
            metadata: serde_json::json!({}),
        };
        let out = client.send_task("loopback", &task).await.unwrap();
        assert_eq!(out.id, "loop-1");
        assert_eq!(out.status, TaskStatus::Working);
    }

    // ---------- Capabilities ----------

    #[test]
    fn capabilities_supports_and_intersect() {
        let local = Capabilities::new(["tasks/send", "tasks/get", "streaming"]);
        let remote = Capabilities::new(["tasks/send", "streaming", "push-notifications"]);
        assert!(local.supports("tasks/send"));
        assert!(!local.supports("push-notifications"));
        let common = local.intersect(&remote);
        assert_eq!(common.supported, vec!["streaming", "tasks/send"]);
    }

    #[test]
    fn capabilities_serde_roundtrip() {
        let caps = Capabilities::new(["tasks/send"]);
        let json = serde_json::to_string(&caps).unwrap();
        let back: Capabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(caps, back);
    }

    #[cfg(feature = "acp-compat")]
    #[test]
    fn acp_to_a2a_conversion() {
        use acp_compat::*;
        let acp = AcpMessage {
            sender: "legacy".into(),
            recipient: "sera".into(),
            content: serde_json::json!({"text": "hello"}),
            metadata: serde_json::json!({}),
        };
        let msg = acp_to_a2a(&acp);
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.parts.len(), 1);
    }
}
