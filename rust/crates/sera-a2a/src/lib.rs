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
pub struct Task {
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
    async fn send_task(&self, agent_url: &str, task: &Task) -> Result<Task, A2aError>;

    /// Get the status of a previously delegated task.
    async fn get_task(&self, agent_url: &str, task_id: &str) -> Result<Task, A2aError>;

    /// Cancel a previously delegated task.
    async fn cancel_task(&self, agent_url: &str, task_id: &str) -> Result<Task, A2aError>;
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
