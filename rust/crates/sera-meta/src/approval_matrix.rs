//! Blast-radius approval matrix from SPEC-self-evolution §9.1.
//!
//! Every `BlastRadius` maps to a row describing:
//! - the review window (time between approval and taking effect)
//! - the number of `MetaApprover` signatures required
//! - whether a shadow-session dry-run is mandatory
//! - whether a canary workload is mandatory
//! - the rate limit (per caller, per period)
//!
//! This matrix is the single source of truth for how a proposed change is gated.
//! Tier 1 rows have zero approvers — they are bounded by the agent's sandbox.
//! Tier 2 rows require 1–2 `MetaApprover` signatures with a shadow replay.
//! Tier 3 rows require a meta-quorum plus canary; the four most dangerous scopes
//! additionally require an operator-signed offline key (tracked by caller, not encoded here).

use std::time::Duration;

use crate::BlastRadius;

/// A row from the SPEC-self-evolution §9.1 approval matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApprovalRequirements {
    /// Review window between approval and the change taking effect.
    pub review_window: Duration,
    /// Number of `MetaApprover` signatures required (0 = none, Tier 1).
    pub approvers_required: u8,
    /// Whether a shadow-session dry-run must pass before approval.
    pub requires_shadow_replay: bool,
    /// Whether a canary workload must run before promotion.
    pub requires_canary: bool,
    /// Whether an operator-signed offline key is additionally required
    /// (the four most dangerous Tier-3 scopes).
    pub requires_operator_offline_key: bool,
    /// Max proposals per caller per `rate_limit_period`.
    pub rate_limit_count: u32,
    /// Period for the rate limit.
    pub rate_limit_period: Duration,
}

impl ApprovalRequirements {
    /// Lookup the requirements for a given `BlastRadius`.
    ///
    /// Panics only if `BlastRadius` grows a new variant and this table is not
    /// updated; the `#[non_exhaustive]` marker on `BlastRadius` guards against
    /// silent drift.
    pub fn for_blast_radius(br: BlastRadius) -> Self {
        const HOUR: Duration = Duration::from_secs(3600);
        const DAY: Duration = Duration::from_secs(86_400);
        const WEEK: Duration = Duration::from_secs(604_800);

        match br {
            // Tier 1 — no approval required, bounded by sandbox + token budget.
            BlastRadius::AgentMemory
            | BlastRadius::AgentPersonaMutable
            | BlastRadius::AgentSkill
            | BlastRadius::AgentExperiencePool => Self {
                review_window: Duration::ZERO,
                approvers_required: 0,
                requires_shadow_replay: false,
                requires_canary: false,
                requires_operator_offline_key: false,
                rate_limit_count: u32::MAX,
                rate_limit_period: HOUR,
            },

            // Tier 2 — single-approver scopes.
            BlastRadius::SingleHookConfig | BlastRadius::SingleToolPolicy => Self {
                review_window: Duration::from_secs(5 * 60),
                approvers_required: 1,
                requires_shadow_replay: true,
                requires_canary: false,
                requires_operator_offline_key: false,
                rate_limit_count: 10,
                rate_limit_period: HOUR,
            },
            BlastRadius::SingleConnector => Self {
                review_window: Duration::from_secs(15 * 60),
                approvers_required: 1,
                requires_shadow_replay: true,
                requires_canary: false,
                requires_operator_offline_key: false,
                rate_limit_count: 5,
                rate_limit_period: HOUR,
            },
            BlastRadius::SingleCircleConfig => Self {
                review_window: Duration::from_secs(10 * 60),
                approvers_required: 1,
                requires_shadow_replay: true,
                requires_canary: false,
                requires_operator_offline_key: false,
                rate_limit_count: 10,
                rate_limit_period: HOUR,
            },
            BlastRadius::AgentManifest => Self {
                review_window: Duration::from_secs(15 * 60),
                approvers_required: 1,
                requires_shadow_replay: true,
                requires_canary: false,
                requires_operator_offline_key: false,
                rate_limit_count: 5,
                rate_limit_period: HOUR,
            },

            // Tier 2 — meta-quorum scopes (2 approvers).
            BlastRadius::TierPolicy => Self {
                review_window: HOUR,
                approvers_required: 2,
                requires_shadow_replay: true,
                requires_canary: false,
                requires_operator_offline_key: false,
                rate_limit_count: 2,
                rate_limit_period: HOUR,
            },
            BlastRadius::HookChainStructure => Self {
                review_window: Duration::from_secs(30 * 60),
                approvers_required: 2,
                requires_shadow_replay: true,
                requires_canary: false,
                requires_operator_offline_key: false,
                rate_limit_count: 5,
                rate_limit_period: HOUR,
            },
            BlastRadius::ApprovalPolicy => Self {
                review_window: HOUR,
                approvers_required: 2,
                requires_shadow_replay: true,
                requires_canary: false,
                requires_operator_offline_key: false,
                rate_limit_count: 2,
                rate_limit_period: HOUR,
            },
            BlastRadius::SecretProvider | BlastRadius::GlobalConfig => Self {
                review_window: HOUR,
                approvers_required: 2,
                requires_shadow_replay: true,
                requires_canary: false,
                requires_operator_offline_key: false,
                rate_limit_count: 2,
                rate_limit_period: HOUR,
            },

            // Tier 3 — code evolution scopes.
            BlastRadius::RuntimeCrate => Self {
                review_window: 4 * HOUR,
                approvers_required: 2,
                requires_shadow_replay: true,
                requires_canary: true,
                requires_operator_offline_key: false,
                rate_limit_count: 3,
                rate_limit_period: DAY,
            },
            BlastRadius::GatewayCore => Self {
                review_window: DAY,
                approvers_required: 3,
                requires_shadow_replay: true,
                requires_canary: true,
                requires_operator_offline_key: false,
                rate_limit_count: 1,
                rate_limit_period: DAY,
            },
            BlastRadius::ProtocolSchema => Self {
                review_window: DAY,
                approvers_required: 3,
                requires_shadow_replay: true,
                requires_canary: true,
                requires_operator_offline_key: false,
                rate_limit_count: 1,
                rate_limit_period: WEEK,
            },
            BlastRadius::DbMigration => Self {
                review_window: 12 * HOUR,
                approvers_required: 3,
                requires_shadow_replay: true,
                requires_canary: true,
                requires_operator_offline_key: false,
                rate_limit_count: 1,
                rate_limit_period: DAY,
            },

            // Tier 3 — meta-change scopes; offline key required.
            BlastRadius::ConstitutionalRuleSet => Self {
                review_window: 72 * HOUR,
                approvers_required: 3,
                requires_shadow_replay: true,
                requires_canary: false,
                requires_operator_offline_key: true,
                rate_limit_count: 1,
                rate_limit_period: WEEK,
            },
            BlastRadius::KillSwitchProtocol => Self {
                review_window: 72 * HOUR,
                approvers_required: 3,
                requires_shadow_replay: true,
                requires_canary: false,
                requires_operator_offline_key: true,
                rate_limit_count: 1,
                rate_limit_period: 30 * DAY,
            },
            BlastRadius::AuditLogBackend => Self {
                review_window: 72 * HOUR,
                approvers_required: 3,
                requires_shadow_replay: true,
                requires_canary: false,
                requires_operator_offline_key: true,
                rate_limit_count: 1,
                rate_limit_period: 365 * DAY,
            },
            BlastRadius::SelfEvolutionPipeline => Self {
                review_window: 72 * HOUR,
                approvers_required: 3,
                requires_shadow_replay: true,
                requires_canary: false,
                requires_operator_offline_key: true,
                rate_limit_count: 1,
                rate_limit_period: 365 * DAY,
            },

            // Forward-compat: any future variant falls through to maximum gates.
            _ => Self {
                review_window: 72 * HOUR,
                approvers_required: 3,
                requires_shadow_replay: true,
                requires_canary: true,
                requires_operator_offline_key: true,
                rate_limit_count: 1,
                rate_limit_period: 365 * DAY,
            },
        }
    }

    /// Whether this row is Tier 1 (no approval required).
    pub fn is_tier1(&self) -> bool {
        self.approvers_required == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier1_has_no_approvers() {
        let row = ApprovalRequirements::for_blast_radius(BlastRadius::AgentMemory);
        assert!(row.is_tier1());
        assert_eq!(row.approvers_required, 0);
        assert!(!row.requires_shadow_replay);
        assert!(!row.requires_canary);
    }

    #[test]
    fn agent_manifest_needs_one_approver_and_shadow_replay() {
        let row = ApprovalRequirements::for_blast_radius(BlastRadius::AgentManifest);
        assert_eq!(row.approvers_required, 1);
        assert!(row.requires_shadow_replay);
        assert!(!row.requires_canary);
    }

    #[test]
    fn global_config_needs_two_approvers() {
        let row = ApprovalRequirements::for_blast_radius(BlastRadius::GlobalConfig);
        assert_eq!(row.approvers_required, 2);
        assert!(row.requires_shadow_replay);
    }

    #[test]
    fn runtime_crate_requires_canary() {
        let row = ApprovalRequirements::for_blast_radius(BlastRadius::RuntimeCrate);
        assert_eq!(row.approvers_required, 2);
        assert!(row.requires_canary);
        assert!(!row.requires_operator_offline_key);
    }

    #[test]
    fn constitutional_rule_set_requires_operator_offline_key() {
        let row = ApprovalRequirements::for_blast_radius(BlastRadius::ConstitutionalRuleSet);
        assert_eq!(row.approvers_required, 3);
        assert!(row.requires_operator_offline_key);
    }

    #[test]
    fn gateway_core_requires_three_approvers_and_canary() {
        let row = ApprovalRequirements::for_blast_radius(BlastRadius::GatewayCore);
        assert_eq!(row.approvers_required, 3);
        assert!(row.requires_canary);
        assert!(!row.requires_operator_offline_key);
    }
}
