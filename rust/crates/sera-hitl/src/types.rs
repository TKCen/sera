//! Core approval types for the HITL approval system.
//!
//! SPEC-hitl-approval §2-3: ApprovalScope, ApprovalUrgency, ApprovalEvidence,
//! ApprovalTarget, ApprovalRouting, ApprovalPolicy, RiskThreshold, ApprovalSpec.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sera_types::principal::PrincipalRef;
use sera_types::tool::RiskLevel;

// ── Scope ─────────────────────────────────────────────────────────────────────

/// What the approval request covers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApprovalScope {
    /// A specific tool invocation.
    ToolCall {
        tool_name: String,
        risk_level: RiskLevel,
    },
    /// A session-level action (pause, terminate, handoff, etc.).
    SessionAction { action: String },
    /// A write to a memory scope.
    MemoryWrite { scope: String },
    /// A change to a configuration path.
    ConfigChange { path: String },
}

// ── Urgency ───────────────────────────────────────────────────────────────────

/// How urgently the approval must be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalUrgency {
    Low,
    Medium,
    High,
    Critical,
}

// ── Evidence ──────────────────────────────────────────────────────────────────

/// Supporting evidence attached to an approval request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalEvidence {
    /// Serialised tool arguments (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_args: Option<serde_json::Value>,
    /// Computed risk score (0.0–1.0) from the risk engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_score: Option<f64>,
    /// The principal whose action triggered the approval request.
    pub principal: PrincipalRef,
    /// Human-readable session context for the approver.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_context: Option<String>,
    /// Arbitrary additional key-value evidence.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub additional: HashMap<String, serde_json::Value>,
}

// ── Target ────────────────────────────────────────────────────────────────────

/// Who should handle an approval request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalTarget {
    /// A specific agent identified by its agent ID.
    Agent { id: String },
    /// A specific principal (human or service).
    Principal(PrincipalRef),
    /// Any principal with the given role name.
    Role { name: String },
}

// ── Routing ───────────────────────────────────────────────────────────────────

/// How the approval chain is selected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ApprovalRouting {
    /// Fixed list of targets — always use this chain regardless of risk.
    Static { targets: Vec<ApprovalTarget> },
    /// Policy-driven — choose chain based on computed risk score.
    Dynamic(ApprovalPolicy),
    /// No external approval — the agent decides autonomously.
    Autonomous,
}

/// Policy for dynamic approval routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalPolicy {
    /// Ordered list of risk thresholds (highest `min_risk_score` checked first).
    pub risk_thresholds: Vec<RiskThreshold>,
    /// Chain used when no threshold matches.
    pub fallback_chain: Vec<ApprovalTarget>,
}

/// A risk threshold that maps a minimum score to an escalation chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskThreshold {
    /// Inclusive lower bound for this threshold (0.0–1.0).
    pub min_risk_score: f64,
    /// Targets to involve when this threshold is met.
    pub chain: Vec<ApprovalTarget>,
    /// Number of approvals required from `chain` to satisfy this threshold.
    pub required_approvals: u32,
}

// ── Spec ──────────────────────────────────────────────────────────────────────

/// Full specification for a single approval request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalSpec {
    /// What action requires approval.
    pub scope: ApprovalScope,
    /// Human-readable description of the request.
    pub description: String,
    /// Urgency level for time-sensitive routing.
    pub urgency: ApprovalUrgency,
    /// How to route the request to approvers.
    pub routing: ApprovalRouting,
    /// How long to wait before treating the ticket as expired.
    #[serde(with = "duration_secs")]
    pub timeout: Duration,
    /// Minimum number of approvals needed to proceed.
    pub required_approvals: u32,
    /// Supporting evidence for the approver.
    pub evidence: ApprovalEvidence,
}

// ── Duration serde helper (seconds as u64) ────────────────────────────────────

mod duration_secs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        d.as_secs().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let secs = u64::deserialize(d)?;
        Ok(Duration::from_secs(secs))
    }
}
