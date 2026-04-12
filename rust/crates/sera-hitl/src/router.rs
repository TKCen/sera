//! ApprovalRouter — resolves routing configuration into escalation chains.

use sera_types::tool::RiskLevel;

use crate::mode::EnforcementMode;
use crate::types::{ApprovalPolicy, ApprovalRouting, ApprovalTarget};

/// Stateless routing logic for the HITL approval system.
pub struct ApprovalRouter;

impl ApprovalRouter {
    /// Resolve a routing configuration into a flat escalation chain.
    ///
    /// For `Static` routing the chain is returned as-is.
    /// For `Dynamic` routing the best-matching threshold chain is returned
    /// (highest `min_risk_score` that is ≤ `risk_score`), falling back to
    /// `policy.fallback_chain` when no threshold matches.
    /// For `Autonomous` an empty chain is returned.
    pub fn resolve_chain(
        routing: &ApprovalRouting,
        risk_score: Option<f64>,
    ) -> Vec<ApprovalTarget> {
        match routing {
            ApprovalRouting::Autonomous => vec![],
            ApprovalRouting::Static { targets } => targets.clone(),
            ApprovalRouting::Dynamic(policy) => {
                Self::best_chain(policy, risk_score).to_vec()
            }
        }
    }

    /// Determine whether an action requires approval given the enforcement mode,
    /// risk level, and routing configuration.
    ///
    /// - `Autonomous` → never needs approval.
    /// - `Strict` → always needs approval.
    /// - `Standard` → needs approval when the resolved chain is non-empty.
    pub fn needs_approval(
        mode: EnforcementMode,
        risk_level: RiskLevel,
        routing: &ApprovalRouting,
    ) -> bool {
        match mode {
            EnforcementMode::Autonomous => false,
            EnforcementMode::Strict => true,
            EnforcementMode::Standard => {
                // Use the risk level to synthesise a coarse score for threshold matching.
                let score = Self::risk_level_to_score(risk_level);
                !Self::resolve_chain(routing, Some(score)).is_empty()
            }
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Return the chain slice that best matches `risk_score` from `policy`.
    ///
    /// The threshold with the highest `min_risk_score` that is still ≤ the
    /// provided score wins.  Falls back to `fallback_chain` when nothing matches.
    pub(crate) fn best_chain(
        policy: &ApprovalPolicy,
        risk_score: Option<f64>,
    ) -> &[ApprovalTarget] {
        let score = risk_score.unwrap_or(0.0);

        // Find the threshold with the largest min_risk_score that is ≤ score.
        let best = policy
            .risk_thresholds
            .iter()
            .filter(|t| t.min_risk_score <= score)
            .max_by(|a, b| {
                a.min_risk_score
                    .partial_cmp(&b.min_risk_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

        match best {
            Some(threshold) => &threshold.chain,
            None => &policy.fallback_chain,
        }
    }

    /// Public version of risk_level_to_score for external callers.
    pub fn risk_level_to_score_public(level: RiskLevel) -> f64 {
        Self::risk_level_to_score(level)
    }

    /// Convert a `RiskLevel` to a representative score for threshold matching.
    fn risk_level_to_score(level: RiskLevel) -> f64 {
        match level {
            RiskLevel::Read => 0.1,
            RiskLevel::Write => 0.4,
            RiskLevel::Execute => 0.7,
            RiskLevel::Admin => 1.0,
        }
    }
}
