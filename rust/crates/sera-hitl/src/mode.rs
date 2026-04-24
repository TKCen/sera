//! Enforcement mode — controls the global approval gate behaviour.

use serde::{Deserialize, Serialize};

/// Controls how strictly the HITL approval gate is applied.
///
/// - `Autonomous` skips all approval checks (single-agent, trusted environments).
/// - `Standard` applies policy-driven approval routing.
/// - `Strict` forces approval for every tool call regardless of policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitlMode {
    /// No approvals required — all tool calls proceed immediately.
    Autonomous,
    /// Policy-driven — approval routing determines whether approval is needed.
    Standard,
    /// Every tool call requires at least one approval regardless of risk level.
    Strict,
}
