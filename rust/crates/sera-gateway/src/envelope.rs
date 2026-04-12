//! SQ/EQ envelope types — Submission Queue and Event Queue messages.
//!
//! Per SPEC-gateway §3.1–§3.2: every client interaction enters via a Submission
//! and every response exits via an Event stream.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use sera_types::content_block::ContentBlock;
use sera_types::runtime::TokenUsage;

// ── Submission (SQ input) ───────────────────────────────────────────────────

/// A submission entering the gateway's submission queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Submission {
    pub id: Uuid,
    pub op: Op,
    pub trace: W3cTraceContext,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_artifact: Option<String>,
}

/// Operations that can be submitted to the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Op {
    UserTurn {
        items: Vec<ContentBlock>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        approval_policy: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sandbox_policy: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_override: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        final_output_schema: Option<serde_json::Value>,
    },
    Steer {
        items: Vec<ContentBlock>,
    },
    Interrupt,
    System(SystemOp),
    ApprovalResponse {
        approval_id: Uuid,
        decision: ApprovalDecision,
    },
    Register(RegisterOp),
}

/// System operations (shutdown, health, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "system_op", rename_all = "snake_case")]
pub enum SystemOp {
    Shutdown,
    HealthCheck,
    ReloadConfig,
}

/// Approval decisions for HITL requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approved,
    Denied { reason: Option<String> },
}

/// Registration operations (agent, connector, plugin).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "register_op", rename_all = "snake_case")]
pub enum RegisterOp {
    Agent { manifest: serde_json::Value },
    Connector { config: serde_json::Value },
    Plugin { config: serde_json::Value },
}

// ── Event (EQ output) ───────────────────────────────────────────────────────

/// An event emitted from the gateway's event queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub submission_id: Uuid,
    pub msg: EventMsg,
    pub trace: W3cTraceContext,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Event message variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventMsg {
    StreamingDelta { delta: String },
    TurnStarted { turn_id: Uuid },
    TurnCompleted { turn_id: Uuid, tokens: TokenUsage },
    ToolCallStarted { call_id: String, tool_name: String },
    ToolCallCompleted { call_id: String, result: serde_json::Value },
    HitlRequest { approval_id: Uuid, description: String },
    CompactionStarted,
    CompactionCompleted { tokens_before: u32, tokens_after: u32 },
    SessionTransition { from: String, to: String },
    Error { code: String, message: String },
}

/// W3C trace context for distributed tracing.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct W3cTraceContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traceparent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracestate: Option<String>,
}

/// Event context attached to every event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventContext {
    pub agent_id: String,
    pub session_key: String,
    pub sender: String,
    pub recipient: String,
    pub principal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_key: Option<String>,
    pub generation: GenerationMarker,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

/// Generation marker for binary identity tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationMarker {
    pub label: String,
    pub binary_identity: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

impl GenerationMarker {
    /// Create a generation marker from the current build.
    pub fn current() -> Self {
        Self {
            label: env!("CARGO_PKG_VERSION").to_string(),
            binary_identity: format!("sera-gateway@{}", env!("CARGO_PKG_VERSION")),
            started_at: chrono::Utc::now(),
        }
    }
}

/// Deduplication key for submissions.
#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct DedupeKey {
    pub channel: String,
    pub account: String,
    pub peer: String,
    pub session_key: String,
    pub message_id: String,
}

/// Queue processing mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueMode {
    Collect,
    Followup,
    Steer,
    SteerBacklog,
    Interrupt,
}

/// Worker failure classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerFailureKind {
    TrustGate,
    PromptDelivery,
    Protocol,
    Provider,
}
