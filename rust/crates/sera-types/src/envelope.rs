//! SQ/EQ envelope types — shared between gateway and runtime.
//!
//! Per SPEC-gateway §3.1–§3.2: every client interaction enters via a Submission
//! and every response exits via an Event stream.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::principal::PrincipalRef;
use crate::runtime::TokenUsage;
use crate::sandbox::EgressEndpoint;

// ── Submission (SQ input) ───────────────────────────────────────────────────

/// A submission entering the gateway's submission queue.
///
/// `session_key` and `parent_session_key` are envelope-level routing metadata
/// (sera-zx5w): the gateway stamps each submission with the target session's
/// key so the runtime can correlate frames without embedding correlation data
/// inside every [`Op`] variant. Both fields are optional for backwards
/// compatibility with pre-zx5w senders.
///
/// `parent_task_id` (sera-pmil) tags submissions that belong to a subagent
/// invocation: the parent's turn calls a handoff/Task tool which becomes a
/// `Task` block in the parent's transcript; the spawned child's submission
/// carries the parent's `call_id` here so every [`Event`] the child emits can
/// be routed to the matching `Task` block in the parent's TUI thread.
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
    /// Parent `Task` block correlator — set when this submission is the
    /// subagent invocation triggered by a `Task`/handoff tool call in the
    /// parent's turn. Carries the `call_id` of that tool call so every Event
    /// emitted while servicing this submission can be tagged for routing to
    /// the parent's Task block in the TUI (sera-pmil / J.2.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
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

/// Registration operations (agent, connector, plugin, external Pattern-B agent).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "register_op", rename_all = "snake_case")]
pub enum RegisterOp {
    Agent { manifest: serde_json::Value },
    Connector { config: serde_json::Value },
    Plugin { config: serde_json::Value },
    /// Pattern B (sera-pcvp): a profile-as-subagent session
    /// (Claude Code tmux pane, OMC orchestrator pane, Hermes profile,
    /// external A2A/ACP agent) registers itself as an ExternalAgentPrincipal
    /// for the lifetime of one session. See SPEC-gateway §1d / §3.1.
    ExternalAgent(ExternalAgentRegistration),
}

/// Pattern B registration payload. Every field is required.
///
/// Recorded by the gateway as the typed handshake that pins the declared
/// scope of an external session at registration. The gateway MUST refuse a
/// registration whose `delegated_by` resolves to an unknown principal —
/// this is the anti-laundering invariant for Pattern B.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalAgentRegistration {
    /// Wire protocol the external session speaks (A2A, ACP-compat, custom CLI).
    pub protocol: ExternalProtocol,

    /// Stable identifier the external session asserts about itself
    /// (e.g. tmux session name, ACP agent id, A2A agent_card.name).
    /// Combined with `protocol` to derive `Principal::id` =
    /// `"ext:{protocol}:{external_id}"` for backwards-compat with
    /// `Principal::external_agent`.
    pub external_id: String,

    /// The SERA principal that delegated this session into existence.
    /// REQUIRED — gateway MUST refuse a registration whose `delegated_by`
    /// is absent or whose principal id is unknown.
    pub delegated_by: PrincipalRef,

    /// Egress allowlist the registering session declares. Enforced by the
    /// sera-eq0m kernel layer; recorded here so the gateway pins the
    /// declared scope at registration rather than leaving it implicit.
    /// `EgressEndpoint::InferenceLocal` is the canonical first entry for
    /// any Pattern B session that talks to an LLM through the proxy.
    pub declared_egress: Vec<EgressEndpoint>,

    /// Opaque handle for the budget bucket this session is bound to.
    /// In M1 this is just a newtype wrapper over a principal-id string;
    /// sera-plcv's per-principal token bucket will key on it without
    /// re-shaping the envelope.
    pub budget_handle: BudgetHandle,
}

/// Wire protocol an external Pattern-B session speaks.
///
/// `Custom(String)` covers launchers / harnesses that do not negotiate over
/// A2A or ACP-compat (e.g. a clawhip-launched Claude Code pane).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalProtocol {
    A2a,
    AcpCompat,
    Custom(String),
}

/// Newtype wrapper over the budget bucket key. Today this is the principal
/// id; the wrapper exists so sera-plcv can replace the inner shape without
/// touching every RegisterOp consumer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BudgetHandle(pub String);

// ── Event (EQ output) ───────────────────────────────────────────────────────

/// An event emitted from the gateway's event queue.
///
/// `parent_session_key` is carried on every frame so consumers can route
/// events for child sessions without parsing the `msg` body (sera-zx5w).
///
/// `parent_task_id` (sera-pmil) carries the parent turn's tool `call_id` for
/// every frame emitted by a subagent invocation. The TUI uses this to route
/// child SSE events to the matching `Task` block in the parent's transcript.
/// `None` means the event belongs to the top-level turn, not a subagent.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
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
        /// Structured outcome of the tool execution. Defaults to `Success`
        /// so older runtimes that do not set the field still deserialize.
        /// Set by the runtime via [`sera-tqzd`] when `ToolDispatcher::dispatch`
        /// returns an error, so failures are never reduced to an opaque
        /// content string at the operator/audit boundary.
        #[serde(default)]
        status: ToolCallStatus,
        /// Canonical [`ToolError`](crate::tool::ToolError) variant name when
        /// `status == Failure` (e.g. `"policy_denied"`, `"execution_failed"`).
        /// `None` on success or when the runtime cannot classify the error.
        /// Used by the gateway audit ledger ([`sera-q66q`]) so a closeout
        /// reader can filter on failure class without reparsing free text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_class: Option<String>,
    },
    HitlRequest {
        approval_id: Uuid,
        description: String,
    },
    /// HITL Phase 2 (sera-93h4): emitted when a previously suspended turn's
    /// approval ticket has been approved. Clients listening on a session-
    /// scoped event channel use this to know it is safe to resubmit the
    /// blocked message — the original chat response already closed with 403,
    /// so resume semantics are "re-send the same submission with the ticket
    /// now approved" rather than a server-side replay.
    Resumed {
        ticket_id: String,
        session_key: String,
    },
    /// Pattern B external-agent registration was accepted and pinned by the gateway.
    ExternalAgentRegistered {
        session_key: String,
        principal: crate::principal::PrincipalRef,
        delegated_by: crate::principal::PrincipalRef,
    },
    /// Guardian pre-approval LLM risk assessment (sera-k600 / SPEC-hitl-approval §2b).
    /// Emitted by the runtime when a `SecurityAnalyzer` returns a structured
    /// `GuardianAssessment` for a proposed tool call. Carries the analyzer
    /// name (e.g. `"invariant"`, `"gray_swan"`, `"heuristic"`) plus the
    /// assessment payload as a `serde_json::Value` so this crate stays free
    /// of a `sera-hitl` dependency cycle.
    GuardianAssessment {
        analyzer: String,
        assessment: serde_json::Value,
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

/// Outcome of a tool execution surfaced on [`EventMsg::ToolCallEnd`].
///
/// Lives alongside the wire envelope so both the runtime emitter and the
/// gateway consumer share the same enum. `Success` is the serde default so
/// older runtime builds — which never set the field — keep parsing.
///
/// Anchors: `sera-tqzd` (failure must be operator-visible) and `sera-q66q`
/// (audit ledger entry per execution).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    #[default]
    Success,
    Failure,
}

impl ToolCallStatus {
    /// Stable lowercase tag — used by the gateway audit payload so dashboards
    /// can filter without recomputing the enum case.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
        }
    }
}

/// JSON field on a transcript `tool` message that flags a failed dispatch
/// (`"success"` / `"failure"`). Underscore-prefixed so the field is treated
/// as runtime/gateway metadata rather than an OpenAI tool-message contract
/// field. Older builds that never see it default to [`ToolCallStatus::Success`].
pub const TOOL_STATUS_MARKER: &str = "_sera_status";

/// JSON field on a transcript `tool` message carrying the canonical
/// [`crate::tool::ToolError::class_name`] when [`TOOL_STATUS_MARKER`] is
/// `"failure"`. Absent on success.
pub const TOOL_ERROR_CLASS_MARKER: &str = "_sera_error_class";

/// Parse the failure markers off a transcript `tool` message, returning
/// `(status, error_class)`. Missing / malformed markers degrade to
/// `(Success, None)` so the helper is safe on any tool-result JSON shape.
///
/// Single source of truth shared between the runtime's NDJSON emitter
/// ([`sera-runtime`]) and the gateway's embedded-transport projection
/// ([`sera-gateway`]) — keeps the wire / projection paths from drifting.
pub fn read_tool_status_markers(msg: &serde_json::Value) -> (ToolCallStatus, Option<String>) {
    let status = match msg.get(TOOL_STATUS_MARKER).and_then(|v| v.as_str()) {
        Some("failure") => ToolCallStatus::Failure,
        Some("success") | None => ToolCallStatus::Success,
        // Unknown future strings are treated as success to preserve the
        // forward-compat default; the wire field always wins if set.
        Some(_) => ToolCallStatus::Success,
    };
    let error_class = msg
        .get(TOOL_ERROR_CLASS_MARKER)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (status, error_class)
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
    /// - `"parent_task_id"` — runtime propagates `parent_task_id` on subagent frames (sera-pmil)
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
                    "parent_task_id".to_string(),
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
        assert!(parsed.capabilities.supports("parent_task_id"));
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

    // ── parent_task_id (sera-pmil) ───────────────────────────────────────────

    #[test]
    fn event_parent_task_id_roundtrip() {
        let evt = Event {
            id: Uuid::new_v4(),
            submission_id: Uuid::new_v4(),
            msg: EventMsg::StreamingDelta {
                delta: "hi".to_string(),
            },
            trace: W3cTraceContext::default(),
            timestamp: chrono::Utc::now(),
            parent_session_key: Some("parent-sess".to_string()),
            parent_task_id: Some("call_abc123".to_string()),
        };
        let json = serde_json::to_string(&evt).unwrap();
        assert!(
            json.contains("\"parent_task_id\":\"call_abc123\""),
            "json = {json}"
        );
        let parsed: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.parent_task_id.as_deref(), Some("call_abc123"));
    }

    #[test]
    fn event_parent_task_id_omitted_when_none() {
        let evt = Event {
            id: Uuid::new_v4(),
            submission_id: Uuid::new_v4(),
            msg: EventMsg::TurnStarted {
                turn_id: Uuid::new_v4(),
            },
            trace: W3cTraceContext::default(),
            timestamp: chrono::Utc::now(),
            parent_session_key: None,
            parent_task_id: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        assert!(
            !json.contains("parent_task_id"),
            "field must be skipped when None; json = {json}"
        );
    }

    #[test]
    fn event_legacy_payload_without_parent_task_id_parses() {
        // A producer that predates sera-pmil emits no `parent_task_id` field.
        // Consumers must still be able to deserialize the event and observe
        // `parent_task_id == None` (forward-compat / `#[serde(default)]`).
        let legacy = serde_json::json!({
            "id": Uuid::new_v4(),
            "submission_id": Uuid::new_v4(),
            "msg": {"type": "streaming_delta", "delta": "x"},
            "timestamp": "2024-01-01T00:00:00Z"
        });
        let parsed: Event = serde_json::from_value(legacy).unwrap();
        assert!(parsed.parent_task_id.is_none());
        assert!(parsed.parent_session_key.is_none());
    }

    #[test]
    fn submission_parent_task_id_roundtrip() {
        let sub = Submission {
            id: Uuid::new_v4(),
            op: Op::UserTurn {
                items: vec![],
                cwd: None,
                approval_policy: None,
                sandbox_policy: None,
                model_override: None,
                effort: None,
                final_output_schema: None,
            },
            trace: W3cTraceContext::default(),
            change_artifact: None,
            session_key: Some("child-sess".to_string()),
            parent_session_key: Some("parent-sess".to_string()),
            parent_task_id: Some("call_xyz".to_string()),
        };
        let json = serde_json::to_string(&sub).unwrap();
        assert!(
            json.contains("\"parent_task_id\":\"call_xyz\""),
            "json = {json}"
        );
        let parsed: Submission = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.parent_task_id.as_deref(), Some("call_xyz"));
    }

    #[test]
    fn submission_parent_task_id_omitted_when_none() {
        let sub = Submission {
            id: Uuid::new_v4(),
            op: Op::Interrupt,
            trace: W3cTraceContext::default(),
            change_artifact: None,
            session_key: None,
            parent_session_key: None,
            parent_task_id: None,
        };
        let json = serde_json::to_string(&sub).unwrap();
        assert!(
            !json.contains("parent_task_id"),
            "field must be skipped when None; json = {json}"
        );
    }

    #[test]
    fn submission_legacy_payload_without_parent_task_id_parses() {
        let legacy = serde_json::json!({
            "id": Uuid::new_v4(),
            "op": {"type": "interrupt"}
        });
        let parsed: Submission = serde_json::from_value(legacy).unwrap();
        assert!(parsed.parent_task_id.is_none());
    }

    // ── RegisterOp::ExternalAgent (sera-pcvp) ────────────────────────────────

    use crate::principal::{PrincipalId, PrincipalKind};

    fn sample_external_agent_registration() -> ExternalAgentRegistration {
        ExternalAgentRegistration {
            protocol: ExternalProtocol::A2a,
            external_id: "claude-code-pane-1".to_string(),
            delegated_by: PrincipalRef {
                id: PrincipalId::new("agent:parent-1"),
                kind: PrincipalKind::Agent,
            },
            declared_egress: vec![EgressEndpoint::InferenceLocal],
            budget_handle: BudgetHandle("ext:a2a:claude-code-pane-1".to_string()),
        }
    }

    #[test]
    fn external_agent_register_op_serde_roundtrip() {
        let op = RegisterOp::ExternalAgent(sample_external_agent_registration());
        let json = serde_json::to_value(&op).unwrap();
        assert_eq!(json["register_op"], "external_agent");
        assert_eq!(json["protocol"], "a2a");
        assert_eq!(json["external_id"], "claude-code-pane-1");
        assert_eq!(json["delegated_by"]["id"], "agent:parent-1");
        assert_eq!(json["delegated_by"]["kind"], "agent");
        assert_eq!(json["declared_egress"][0]["type"], "inference_local");
        assert_eq!(json["budget_handle"], "ext:a2a:claude-code-pane-1");

        let parsed: RegisterOp = serde_json::from_value(json).unwrap();
        match parsed {
            RegisterOp::ExternalAgent(r) => {
                assert_eq!(r.external_id, "claude-code-pane-1");
                assert_eq!(r.protocol, ExternalProtocol::A2a);
                assert_eq!(r.delegated_by.id.0, "agent:parent-1");
                assert_eq!(r.delegated_by.kind, PrincipalKind::Agent);
                assert_eq!(r.declared_egress.len(), 1);
                assert_eq!(r.budget_handle.0, "ext:a2a:claude-code-pane-1");
            }
            other => panic!("expected ExternalAgent, got {other:?}"),
        }
    }

    #[test]
    fn external_agent_register_op_minimal_payload() {
        // Missing `delegated_by` — must fail to deserialize.
        let missing_delegator = serde_json::json!({
            "register_op": "external_agent",
            "protocol": "a2a",
            "external_id": "x",
            "declared_egress": [{"type": "inference_local"}],
            "budget_handle": "k"
        });
        assert!(serde_json::from_value::<RegisterOp>(missing_delegator).is_err());

        // Missing `declared_egress` — must fail to deserialize.
        let missing_egress = serde_json::json!({
            "register_op": "external_agent",
            "protocol": "a2a",
            "external_id": "x",
            "delegated_by": {"id": "agent:p", "kind": "agent"},
            "budget_handle": "k"
        });
        assert!(serde_json::from_value::<RegisterOp>(missing_egress).is_err());
    }

    #[test]
    fn external_protocol_serde_snake_case() {
        assert_eq!(
            serde_json::to_value(ExternalProtocol::A2a).unwrap(),
            serde_json::json!("a2a"),
        );
        assert_eq!(
            serde_json::to_value(ExternalProtocol::AcpCompat).unwrap(),
            serde_json::json!("acp_compat"),
        );
        assert_eq!(
            serde_json::to_value(ExternalProtocol::Custom("hermes".to_string())).unwrap(),
            serde_json::json!({"custom": "hermes"}),
        );

        // Round-trip the variants.
        let parsed_a2a: ExternalProtocol = serde_json::from_value(serde_json::json!("a2a")).unwrap();
        assert_eq!(parsed_a2a, ExternalProtocol::A2a);
        let parsed_acp: ExternalProtocol =
            serde_json::from_value(serde_json::json!("acp_compat")).unwrap();
        assert_eq!(parsed_acp, ExternalProtocol::AcpCompat);
        let parsed_custom: ExternalProtocol =
            serde_json::from_value(serde_json::json!({"custom": "hermes"})).unwrap();
        assert_eq!(parsed_custom, ExternalProtocol::Custom("hermes".to_string()));
    }

    #[test]
    fn budget_handle_is_transparent_string() {
        // Serializes to a bare string, not a wrapping object.
        let handle = BudgetHandle("ext:a2a:bot".to_string());
        let json = serde_json::to_value(&handle).unwrap();
        assert_eq!(json, serde_json::json!("ext:a2a:bot"));

        // And deserializes from a bare string — proves `#[serde(transparent)]`.
        let parsed: BudgetHandle = serde_json::from_value(serde_json::json!("ext:a2a:bot")).unwrap();
        assert_eq!(parsed.0, "ext:a2a:bot");
    }

    #[test]
    fn register_op_legacy_variants_still_parse() {
        // Adding `ExternalAgent` must not break the existing variants.
        let agent: RegisterOp = serde_json::from_value(serde_json::json!({
            "register_op": "agent",
            "manifest": {"name": "old"}
        }))
        .unwrap();
        assert!(matches!(agent, RegisterOp::Agent { .. }));

        let connector: RegisterOp = serde_json::from_value(serde_json::json!({
            "register_op": "connector",
            "config": {"channel": "discord"}
        }))
        .unwrap();
        assert!(matches!(connector, RegisterOp::Connector { .. }));

        let plugin: RegisterOp = serde_json::from_value(serde_json::json!({
            "register_op": "plugin",
            "config": {"name": "p"}
        }))
        .unwrap();
        assert!(matches!(plugin, RegisterOp::Plugin { .. }));
    }

    // ── ToolCallStatus + marker reader (sera-tqzd / sera-q66q) ────────────────

    #[test]
    fn tool_call_status_default_is_success() {
        // Wire-default must be Success so old runtimes that never set the
        // field don't get reclassified as failures.
        assert_eq!(ToolCallStatus::default(), ToolCallStatus::Success);
    }

    #[test]
    fn tool_call_status_serializes_snake_case() {
        let success = serde_json::to_value(ToolCallStatus::Success).unwrap();
        let failure = serde_json::to_value(ToolCallStatus::Failure).unwrap();
        assert_eq!(success, serde_json::Value::String("success".into()));
        assert_eq!(failure, serde_json::Value::String("failure".into()));
    }

    #[test]
    fn read_tool_status_markers_defaults_to_success() {
        let msg = serde_json::json!({
            "role": "tool",
            "tool_call_id": "c1",
            "content": "ok",
        });
        let (status, class) = read_tool_status_markers(&msg);
        assert_eq!(status, ToolCallStatus::Success);
        assert!(class.is_none());
    }

    #[test]
    fn read_tool_status_markers_recognises_failure() {
        let msg = serde_json::json!({
            "role": "tool",
            "tool_call_id": "c2",
            "content": "[tool error: …]",
            TOOL_STATUS_MARKER: "failure",
            TOOL_ERROR_CLASS_MARKER: "policy_denied",
        });
        let (status, class) = read_tool_status_markers(&msg);
        assert_eq!(status, ToolCallStatus::Failure);
        assert_eq!(class.as_deref(), Some("policy_denied"));
    }

    #[test]
    fn read_tool_status_markers_ignores_unknown_status() {
        // Forward-compat: unknown status values fall back to Success rather
        // than synthesising a phantom failure. The wire field always wins
        // when set to a known value.
        let msg = serde_json::json!({
            "role": "tool",
            TOOL_STATUS_MARKER: "weird_future_state",
        });
        let (status, class) = read_tool_status_markers(&msg);
        assert_eq!(status, ToolCallStatus::Success);
        assert!(class.is_none());
    }

    #[test]
    fn tool_call_end_with_status_round_trips() {
        // Forward-compat: a runtime that emits the new fields must
        // round-trip through serde, and an older runtime that omits them
        // must deserialize with the Success default.
        let new = EventMsg::ToolCallEnd {
            turn_id: uuid::Uuid::nil(),
            call_id: "c3".into(),
            result: "[tool error: timeout]".into(),
            status: ToolCallStatus::Failure,
            error_class: Some("timeout".into()),
        };
        let json = serde_json::to_value(&new).unwrap();
        assert_eq!(json["status"], serde_json::Value::String("failure".into()));
        assert_eq!(json["error_class"], serde_json::Value::String("timeout".into()));
        let parsed: EventMsg = serde_json::from_value(json).unwrap();
        match parsed {
            EventMsg::ToolCallEnd { status, error_class, .. } => {
                assert_eq!(status, ToolCallStatus::Failure);
                assert_eq!(error_class.as_deref(), Some("timeout"));
            }
            other => panic!("expected ToolCallEnd, got {:?}", other),
        }

        // Older wire shape — no status / error_class — must default to Success.
        let legacy = serde_json::json!({
            "type": "tool_call_end",
            "turn_id": uuid::Uuid::nil(),
            "call_id": "c4",
            "result": "ok",
        });
        let parsed: EventMsg = serde_json::from_value(legacy).unwrap();
        match parsed {
            EventMsg::ToolCallEnd { status, error_class, .. } => {
                assert_eq!(status, ToolCallStatus::Success);
                assert!(error_class.is_none());
            }
            other => panic!("expected ToolCallEnd, got {:?}", other),
        }
    }
}
