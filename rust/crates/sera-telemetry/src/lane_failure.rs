//! Lane failure classification for SERA 2.0.

use serde::{Deserialize, Serialize};

/// Classification of lane failures, mapped to OCSF Detection Finding extensions.
///
/// `#[non_exhaustive]` because new failure modes will be added in future phases.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneFailureClass {
    /// LLM call or message delivery to a prompt lane failed.
    PromptDelivery,
    /// Trust gate rejected the request (policy violation).
    TrustGate,
    /// Two branches of the same lane diverged irreconcilably.
    BranchDivergence,
    /// Code compilation step failed inside the sandbox.
    Compile,
    /// Automated test suite failed.
    Test,
    /// Plugin process failed to start.
    PluginStartup,
    /// MCP server process failed to start.
    McpStartup,
    /// MCP handshake (capability negotiation) failed.
    McpHandshake,
    /// Gateway could not route the request to a provider.
    GatewayRouting,
    /// A tool invocation failed at runtime.
    ToolRuntime,
    /// The lane's workspace does not match the expected context.
    WorkspaceMismatch,
    /// Infrastructure-level failure (network, storage, compute).
    Infra,
    /// An orphaned lane was reaped by the supervisor.
    OrphanReaped,
    /// Output violated the constitutional constraint set.
    ConstitutionalViolation,
    /// A hard kill-switch was activated, terminating the lane.
    KillSwitchActivated,
}

impl LaneFailureClass {
    /// Return the OCSF Detection Finding extension string for this class.
    pub fn as_ocsf_extension(&self) -> &'static str {
        match self {
            LaneFailureClass::PromptDelivery => "prompt_delivery",
            LaneFailureClass::TrustGate => "trust_gate",
            LaneFailureClass::BranchDivergence => "branch_divergence",
            LaneFailureClass::Compile => "compile",
            LaneFailureClass::Test => "test",
            LaneFailureClass::PluginStartup => "plugin_startup",
            LaneFailureClass::McpStartup => "mcp_startup",
            LaneFailureClass::McpHandshake => "mcp_handshake",
            LaneFailureClass::GatewayRouting => "gateway_routing",
            LaneFailureClass::ToolRuntime => "tool_runtime",
            LaneFailureClass::WorkspaceMismatch => "workspace_mismatch",
            LaneFailureClass::Infra => "infra",
            LaneFailureClass::OrphanReaped => "orphan_reaped",
            LaneFailureClass::ConstitutionalViolation => "constitutional_violation",
            LaneFailureClass::KillSwitchActivated => "kill_switch_activated",
        }
    }
}
