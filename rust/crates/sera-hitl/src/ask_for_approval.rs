//! Five-level `AskForApproval` + per-category `GranularApprovalConfig`.
//!
//! SPEC-hitl-approval §3 (`AskForApproval`) and §5a (`GranularApprovalConfig`,
//! `CategoryRouting`, `ExecAllowRule`). Aligned with Codex's five-level
//! approval enum and openclaw's `ExecApprovalsFileSchema`.

use serde::{Deserialize, Serialize};

use crate::types::ApprovalRouting;

/// Five-level approval request style.
///
/// `Policy` preserves the original SERA routing model for backwards compat;
/// the other variants align with Codex `AskForApproval`.
// TODO P1-INTEGRATION: gateway Op::UserTurn.approval_policy already carries an
//                      AskForApproval variant; unify the type once gateway lands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AskForApproval {
    /// Ask for everything except known-safe read-only operations.
    UnlessTrusted,
    /// Model decides when to ask. Default for Tier-2 standard mode.
    OnRequest,
    /// Per-category fine control.
    Granular(Box<GranularApprovalConfig>),
    /// Full-auto; no HITL ever. Tier-1 autonomous sandbox only.
    Never,
    /// Static, dynamic, or delegated policy resolution.
    Policy(ApprovalRouting),
}

/// Per-risk-category routing (exec, patch, file_write, network, mcp_call, …).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GranularApprovalConfig {
    pub exec: CategoryRouting,
    pub patch: CategoryRouting,
    pub file_write: CategoryRouting,
    pub network: CategoryRouting,
    pub mcp_call: CategoryRouting,
    pub memory_write: CategoryRouting,
    pub config_change: CategoryRouting,
    /// Required for Tier-2/3 self-evolution; `None` on deployments that
    /// disallow meta-changes outright.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta_change: Option<CategoryRouting>,
}

/// Routing for a single risk category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryRouting {
    pub default: ApprovalRouting,
    /// Per-agent allowlists with argument patterns (openclaw `ExecApprovals`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow_list: Vec<ExecAllowRule>,
    /// `autoAllowSkills: true` — skill-bound tools bypass approval.
    #[serde(default)]
    pub auto_allow_skills: bool,
}

/// A per-agent allow rule with optional argument pattern.
///
/// Wildcard semantics: rules evaluated in order, stricter-wins; `deny`
/// outranks `ask` outranks `allow`. Session-scoped overrides extend the
/// ruleset at the runtime layer.
// TODO P1-INTEGRATION: wildcard evaluator + session override extension live in
//                      sera-runtime; trait surface lands with that work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecAllowRule {
    /// Agent identifier this rule applies to.
    pub agent_ref: String,
    /// Command or tool name glob.
    pub pattern: String,
    /// Argument regex (separate from command pattern).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arg_pattern: Option<String>,
    /// Human-readable rationale for the audit trail.
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ApprovalPolicy, ApprovalTarget};

    fn autonomous_category() -> CategoryRouting {
        CategoryRouting {
            default: ApprovalRouting::Autonomous,
            allow_list: vec![],
            auto_allow_skills: false,
        }
    }

    fn granular() -> GranularApprovalConfig {
        GranularApprovalConfig {
            exec: autonomous_category(),
            patch: autonomous_category(),
            file_write: autonomous_category(),
            network: autonomous_category(),
            mcp_call: autonomous_category(),
            memory_write: autonomous_category(),
            config_change: autonomous_category(),
            meta_change: None,
        }
    }

    #[test]
    fn ask_for_approval_never_serde_roundtrip() {
        let a = AskForApproval::Never;
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"never\""));
        let _parsed: AskForApproval = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn ask_for_approval_policy_serde_roundtrip() {
        let a = AskForApproval::Policy(ApprovalRouting::Static {
            targets: vec![ApprovalTarget::Role { name: "ops".to_string() }],
        });
        let json = serde_json::to_string(&a).unwrap();
        let parsed: AskForApproval = serde_json::from_str(&json).unwrap();
        match parsed {
            AskForApproval::Policy(ApprovalRouting::Static { targets }) => {
                assert_eq!(targets.len(), 1);
            }
            _ => panic!("expected Policy(Static)"),
        }
    }

    #[test]
    fn ask_for_approval_granular_serde_roundtrip() {
        let a = AskForApproval::Granular(Box::new(granular()));
        let json = serde_json::to_string(&a).unwrap();
        let parsed: AskForApproval = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, AskForApproval::Granular(_)));
    }

    #[test]
    fn category_routing_allow_rule_roundtrip() {
        let cat = CategoryRouting {
            default: ApprovalRouting::Dynamic(ApprovalPolicy {
                risk_thresholds: vec![],
                fallback_chain: vec![],
            }),
            allow_list: vec![ExecAllowRule {
                agent_ref: "agent-42".to_string(),
                pattern: "git *".to_string(),
                arg_pattern: Some("^(status|log|diff)".to_string()),
                reason: "read-only git".to_string(),
            }],
            auto_allow_skills: true,
        };
        let json = serde_json::to_string(&cat).unwrap();
        let parsed: CategoryRouting = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.allow_list.len(), 1);
        assert!(parsed.auto_allow_skills);
        assert_eq!(parsed.allow_list[0].agent_ref, "agent-42");
    }
}
