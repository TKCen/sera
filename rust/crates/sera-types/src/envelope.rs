//! SQ/EQ envelope types — shared between gateway and runtime.
//!
//! Per SPEC-gateway §3.1–§3.2: every client interaction enters via a Submission
//! and every response exits via an Event stream.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runtime::TokenUsage;

// ── Submission (SQ input) ───────────────────────────────────────────────────

/// A submission entering the gateway's submission queue.
///
/// `session_key` and `parent_session_key` are envelope-level routing metadata
/// (sera-zx5w): the gateway stamps each submission with the target session's
/// key so the runtime can correlate frames without embedding correlation data
/// inside every [`Op`] variant. Both fields are optional for backwards
/// compatibility with pre-zx5w senders.
///
/// `trace` is `#[serde(default)]` so submissions arriving over NDJSON from
/// non-W3C-aware callers (e.g. the runtime harness) deserialize successfully.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Submission {
    pub id: Uuid,
    pub op: Op,
    #[serde(default)]
    pub trace: W3cTraceContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_artifact: Option<String>,
    /// Session this submission targets. Used by the runtime to route frames
    /// and by the gateway for per-session persistence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    /// Parent session key — set when this submission belongs to a child
    /// session spawned by another turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_key: Option<String>,
}

/// Operations that can be submitted to the gateway.
///
/// `items` is `Vec<serde_json::Value>` rather than `Vec<ContentBlock>` because
/// the wire NDJSON carries role-scoped conversation messages (e.g.
/// `{"role":"user","content":"..."}`) that pass through the runtime to the LLM
/// without structural re-typing. Callers that construct typed content blocks
/// must serialize them to `serde_json::Value` at the call site (sera-zx5w).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Op {
    UserTurn {
        items: Vec<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approval_policy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sandbox_policy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_override: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        final_output_schema: Option<serde_json::Value>,
    },
    Steer {
        items: Vec<serde_json::Value>,
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
///
/// `parent_session_key` is carried on every frame so consumers can route
/// events for child sessions without parsing the `msg` body (sera-zx5w).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub submission_id: Uuid,
    pub msg: EventMsg,
    #[serde(default)]
    pub trace: W3cTraceContext,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_key: Option<String>,
}

/// Event message variants.
///
/// `ToolCallBegin`/`ToolCallEnd` use the wire names emitted by the runtime
/// (sera-zx5w); their `tool` / `arguments` / `result` shape mirrors the
/// legacy runtime-local envelope so existing gateways parse unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventMsg {
    StreamingDelta {
        delta: String,
    },
    TurnStarted {
        turn_id: Uuid,
    },
    TurnCompleted {
        turn_id: Uuid,
        #[serde(default)]
        tokens: TokenUsage,
    },
    ToolCallBegin {
        turn_id: Uuid,
        call_id: String,
        tool: String,
        arguments: serde_json::Value,
    },
    ToolCallEnd {
        turn_id: Uuid,
        call_id: String,
        result: String,
    },
    HitlRequest {
        approval_id: Uuid,
        description: String,
    },
    CompactionStarted,
    CompactionCompleted {
        tokens_before: u32,
        tokens_after: u32,
    },
    SessionTransition {
        from: String,
        to: String,
    },
    Error {
        code: String,
        message: String,
    },
}

/// W3C trace context for distributed tracing.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct W3cTraceContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traceparent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracestate: Option<String>,
}

// ── Protocol negotiation ────────────────────────────────────────────────────

/// Protocol capabilities advertised by the runtime in the first NDJSON frame.
///
/// The gateway reads this list to determine which features the connected
/// runtime honours. Unknown strings are ignored (forward-compatible).
/// Missing field → empty capability set (pre-v2 behaviour).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProtocolCapabilities {
    /// List of feature tokens this runtime supports.
    ///
    /// Well-known tokens (non-exhaustive):
    /// - `"steer"` — runtime honours `Op::Steer` messages
    /// - `"hitl"` — runtime can gate tool calls on HITL approval
    /// - `"hooks@v2"` — runtime runs ConstitutionalGate hooks on input + output
    /// - `"parent_session_key"` — runtime propagates `parent_session_key` on child frames
    #[serde(default)]
    pub features: Vec<String>,
}

impl ProtocolCapabilities {
    /// Returns `true` when `feature` is in the supported feature list.
    ///
    /// Comparison is case-sensitive and exact.
    pub fn supports(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

/// First NDJSON frame emitted by the runtime on startup (before any turn).
///
/// Consumers MUST treat all fields except `protocol_version` as optional
/// (use `#[serde(default)]`) to stay compatible with older runtimes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeFrame {
    /// Always `"handshake"` — discriminator for frame consumers.
    #[serde(default = "HandshakeFrame::default_frame_type")]
    pub frame_type: String,
    /// Semantic version of the runtime NDJSON protocol (e.g. `"2.0"`).
    pub protocol_version: String,
    /// Capabilities this runtime supports.
    #[serde(default)]
    pub capabilities: ProtocolCapabilities,
    /// Agent identifier echoed back so the gateway can correlate the connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Parent session key, if this runtime was spawned as a child session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_key: Option<String>,
}

impl HandshakeFrame {
    fn default_frame_type() -> String {
        "handshake".to_string()
    }

    /// Build the standard v2 handshake frame with the canonical feature set.
    pub fn v2(agent_id: impl Into<String>, parent_session_key: Option<String>) -> Self {
        Self {
            frame_type: "handshake".to_string(),
            protocol_version: "2.0".to_string(),
            capabilities: ProtocolCapabilities {
                features: vec![
                    "steer".to_string(),
                    "hitl".to_string(),
                    "hooks@v2".to_string(),
                    "parent_session_key".to_string(),
                ],
            },
            agent_id: Some(agent_id.into()),
            parent_session_key,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── ProtocolCapabilities ──────────────────────────────────────────────────

    #[test]
    fn supports_returns_true_for_known_feature() {
        let caps = ProtocolCapabilities {
            features: vec![
                "steer".to_string(),
                "hitl".to_string(),
                "hooks@v2".to_string(),
            ],
        };
        assert!(caps.supports("steer"));
        assert!(caps.supports("hitl"));
        assert!(caps.supports("hooks@v2"));
    }

    #[test]
    fn supports_returns_false_for_unknown_feature() {
        let caps = ProtocolCapabilities {
            features: vec!["steer".to_string()],
        };
        assert!(!caps.supports("unknown_feature"));
        assert!(!caps.supports("STEER")); // case-sensitive
        assert!(!caps.supports(""));
    }

    #[test]
    fn supports_empty_features_always_false() {
        let caps = ProtocolCapabilities::default();
        assert!(!caps.supports("steer"));
        assert!(!caps.supports("hitl"));
    }

    // ── HandshakeFrame serde ──────────────────────────────────────────────────

    #[test]
    fn handshake_frame_v2_roundtrip() {
        let frame = HandshakeFrame::v2("agent-001", Some("parent-sess-xyz".to_string()));
        let json = serde_json::to_string(&frame).unwrap();
        let parsed: HandshakeFrame = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.protocol_version, "2.0");
        assert_eq!(parsed.frame_type, "handshake");
        assert_eq!(parsed.agent_id.as_deref(), Some("agent-001"));
        assert_eq!(
            parsed.parent_session_key.as_deref(),
            Some("parent-sess-xyz")
        );
        assert!(parsed.capabilities.supports("steer"));
        assert!(parsed.capabilities.supports("hitl"));
        assert!(parsed.capabilities.supports("hooks@v2"));
        assert!(parsed.capabilities.supports("parent_session_key"));
    }

    #[test]
    fn handshake_frame_without_parent_session_key() {
        let frame = HandshakeFrame::v2("agent-002", None);
        let json = serde_json::to_string(&frame).unwrap();
        // The field key "parent_session_key" (as a JSON object key) should be absent.
        // Note: the feature token "parent_session_key" appears inside the features array
        // as a value, so we check the struct field is absent by deserializing.
        let parsed: HandshakeFrame = serde_json::from_str(&json).unwrap();
        assert!(parsed.parent_session_key.is_none());
        // Verify the field is not serialized as a top-level key (skip_serializing_if)
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("parent_session_key").is_none());
    }

    #[test]
    fn handshake_frame_legacy_missing_fields_parse_with_defaults() {
        // Simulate a legacy frame missing capabilities + parent_session_key
        let legacy = r#"{"protocol_version":"1.0","frame_type":"handshake"}"#;
        let parsed: HandshakeFrame = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.protocol_version, "1.0");
        assert!(parsed.capabilities.features.is_empty());
        assert!(!parsed.capabilities.supports("steer"));
        assert!(parsed.parent_session_key.is_none());
        assert!(parsed.agent_id.is_none());
    }

    #[test]
    fn protocol_capabilities_serde_roundtrip() {
        let caps = ProtocolCapabilities {
            features: vec!["steer".to_string(), "hitl".to_string()],
        };
        let json = serde_json::to_string(&caps).unwrap();
        let parsed: ProtocolCapabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.features.len(), 2);
        assert!(parsed.supports("steer"));
        assert!(parsed.supports("hitl"));
    }

    #[test]
    fn protocol_capabilities_empty_json_parses_to_default() {
        // Missing `features` field → defaults to empty vec via #[serde(default)]
        let parsed: ProtocolCapabilities = serde_json::from_str("{}").unwrap();
        assert!(parsed.features.is_empty());
    }
}
