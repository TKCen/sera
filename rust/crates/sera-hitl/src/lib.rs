//! sera-hitl — Human-in-the-Loop / Agent-in-the-Loop approval system.
//!
//! Provides configurable escalation chains that can involve agents, humans, or
//! both as approvers.  See SPEC-hitl-approval for the full design.

pub mod error;
pub mod mode;
pub mod router;
pub mod ticket;
pub mod types;

// Convenience re-exports at the crate root.
pub use error::HitlError;
pub use mode::EnforcementMode;
pub use router::ApprovalRouter;
pub use ticket::{ApprovalDecision, ApprovalTicket, TicketStatus};
pub use types::{
    ApprovalEvidence, ApprovalPolicy, ApprovalRouting, ApprovalScope, ApprovalSpec,
    ApprovalTarget, ApprovalUrgency, RiskThreshold,
};

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use sera_types::principal::{Principal, PrincipalId, PrincipalKind, PrincipalRef};
    use sera_types::tool::RiskLevel;

    use super::*;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn human_ref() -> PrincipalRef {
        Principal::default_admin().as_ref()
    }

    fn agent_ref() -> PrincipalRef {
        PrincipalRef {
            id: PrincipalId::new("agent-42"),
            kind: PrincipalKind::Agent,
        }
    }

    fn basic_spec(routing: ApprovalRouting, required: u32, timeout: Duration) -> ApprovalSpec {
        ApprovalSpec {
            scope: ApprovalScope::ToolCall {
                tool_name: "shell".to_string(),
                risk_level: RiskLevel::Execute,
            },
            description: "Execute shell command".to_string(),
            urgency: ApprovalUrgency::Medium,
            routing,
            timeout,
            required_approvals: required,
            evidence: ApprovalEvidence {
                tool_args: None,
                risk_score: Some(0.75),
                principal: human_ref(),
                session_context: None,
                additional: HashMap::new(),
            },
        }
    }

    fn static_routing() -> ApprovalRouting {
        ApprovalRouting::Static {
            targets: vec![ApprovalTarget::Agent { id: "agent-42".to_string() }],
        }
    }

    // ── Ticket lifecycle ──────────────────────────────────────────────────────

    #[test]
    fn ticket_create_approve_is_fully_approved() {
        let spec = basic_spec(static_routing(), 1, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-1");

        assert_eq!(ticket.status, TicketStatus::Pending);
        assert!(!ticket.is_fully_approved());

        let status = ticket.approve(human_ref(), Some("lgtm".to_string()));
        assert_eq!(status, TicketStatus::Approved);
        assert!(ticket.is_fully_approved());
    }

    #[test]
    fn ticket_multi_approval_require_two() {
        let spec = basic_spec(static_routing(), 2, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-2");

        // First approval — not yet satisfied.
        ticket.approve(human_ref(), None);
        assert!(!ticket.is_fully_approved());
        assert_eq!(ticket.status, TicketStatus::Pending);

        // Second approval — now satisfied.
        ticket.approve(agent_ref(), None);
        assert!(ticket.is_fully_approved());
        assert_eq!(ticket.status, TicketStatus::Approved);
    }

    #[test]
    fn ticket_rejection_flow() {
        let spec = basic_spec(static_routing(), 1, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-3");

        let status = ticket.reject(human_ref(), Some("too risky".to_string()));
        assert_eq!(status, TicketStatus::Rejected);
        assert_eq!(ticket.status, TicketStatus::Rejected);
        assert!(!ticket.is_fully_approved());
    }

    #[test]
    fn ticket_escalation_advances_index() {
        let routing = ApprovalRouting::Static {
            targets: vec![
                ApprovalTarget::Agent { id: "agent-1".to_string() },
                ApprovalTarget::Role { name: "admin".to_string() },
            ],
        };
        let spec = basic_spec(routing, 1, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-4");

        assert_eq!(ticket.current_target_index, 0);
        ticket.escalate();
        assert_eq!(ticket.current_target_index, 1);
        assert_eq!(ticket.status, TicketStatus::Escalated);
    }

    #[test]
    fn ticket_expiry() {
        let spec = basic_spec(static_routing(), 1, Duration::from_nanos(1));
        let ticket = ApprovalTicket::new(spec, "session-5");
        // A 1-nanosecond timeout is guaranteed to have passed by the time we check.
        assert!(ticket.is_expired());
    }

    #[test]
    fn ticket_not_expired_with_long_timeout() {
        let spec = basic_spec(static_routing(), 1, Duration::from_secs(3600));
        let ticket = ApprovalTicket::new(spec, "session-6");
        assert!(!ticket.is_expired());
    }

    // ── ApprovalRouter ────────────────────────────────────────────────────────

    #[test]
    fn router_autonomous_returns_empty_chain() {
        let chain = ApprovalRouter::resolve_chain(&ApprovalRouting::Autonomous, None);
        assert!(chain.is_empty());
    }

    #[test]
    fn router_static_returns_chain() {
        let routing = ApprovalRouting::Static {
            targets: vec![
                ApprovalTarget::Agent { id: "a1".to_string() },
                ApprovalTarget::Role { name: "ops".to_string() },
            ],
        };
        let chain = ApprovalRouter::resolve_chain(&routing, None);
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn router_dynamic_threshold_matching() {
        let policy = ApprovalPolicy {
            risk_thresholds: vec![
                RiskThreshold {
                    min_risk_score: 0.3,
                    chain: vec![ApprovalTarget::Role { name: "reviewer".to_string() }],
                    required_approvals: 1,
                },
                RiskThreshold {
                    min_risk_score: 0.7,
                    chain: vec![
                        ApprovalTarget::Role { name: "admin".to_string() },
                        ApprovalTarget::Role { name: "security".to_string() },
                    ],
                    required_approvals: 2,
                },
            ],
            fallback_chain: vec![],
        };
        let routing = ApprovalRouting::Dynamic(policy);

        // Score 0.8 matches the 0.7 threshold (highest matching).
        let chain = ApprovalRouter::resolve_chain(&routing, Some(0.8));
        assert_eq!(chain.len(), 2);

        // Score 0.5 matches the 0.3 threshold.
        let chain = ApprovalRouter::resolve_chain(&routing, Some(0.5));
        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn router_dynamic_fallback_when_no_threshold_matches() {
        let policy = ApprovalPolicy {
            risk_thresholds: vec![RiskThreshold {
                min_risk_score: 0.8,
                chain: vec![ApprovalTarget::Role { name: "admin".to_string() }],
                required_approvals: 1,
            }],
            fallback_chain: vec![ApprovalTarget::Role { name: "fallback".to_string() }],
        };
        let routing = ApprovalRouting::Dynamic(policy);

        // Score 0.1 is below the 0.8 threshold → fallback.
        let chain = ApprovalRouter::resolve_chain(&routing, Some(0.1));
        assert_eq!(chain.len(), 1);
        if let ApprovalTarget::Role { name } = &chain[0] {
            assert_eq!(name, "fallback");
        } else {
            panic!("expected Role target");
        }
    }

    // ── EnforcementMode ───────────────────────────────────────────────────────

    #[test]
    fn enforcement_autonomous_never_needs_approval() {
        assert!(!ApprovalRouter::needs_approval(
            EnforcementMode::Autonomous,
            RiskLevel::Admin,
            &static_routing(),
        ));
    }

    #[test]
    fn enforcement_strict_always_needs_approval() {
        assert!(ApprovalRouter::needs_approval(
            EnforcementMode::Strict,
            RiskLevel::Read,
            &ApprovalRouting::Autonomous,
        ));
    }

    #[test]
    fn enforcement_standard_respects_policy() {
        // Static routing with targets → needs approval.
        assert!(ApprovalRouter::needs_approval(
            EnforcementMode::Standard,
            RiskLevel::Execute,
            &static_routing(),
        ));

        // Autonomous routing → no approval needed.
        assert!(!ApprovalRouter::needs_approval(
            EnforcementMode::Standard,
            RiskLevel::Execute,
            &ApprovalRouting::Autonomous,
        ));
    }

    // ── Serde roundtrips ──────────────────────────────────────────────────────

    #[test]
    fn approval_scope_serde_roundtrip_tool_call() {
        let scope = ApprovalScope::ToolCall {
            tool_name: "shell".to_string(),
            risk_level: RiskLevel::Execute,
        };
        let json = serde_json::to_string(&scope).unwrap();
        let parsed: ApprovalScope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, scope);
    }

    #[test]
    fn approval_scope_serde_roundtrip_session_action() {
        let scope = ApprovalScope::SessionAction { action: "terminate".to_string() };
        let json = serde_json::to_string(&scope).unwrap();
        let parsed: ApprovalScope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, scope);
    }

    #[test]
    fn approval_scope_serde_roundtrip_memory_write() {
        let scope = ApprovalScope::MemoryWrite { scope: "shared".to_string() };
        let json = serde_json::to_string(&scope).unwrap();
        let parsed: ApprovalScope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, scope);
    }

    #[test]
    fn approval_scope_serde_roundtrip_config_change() {
        let scope = ApprovalScope::ConfigChange { path: "/policy/tier".to_string() };
        let json = serde_json::to_string(&scope).unwrap();
        let parsed: ApprovalScope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, scope);
    }

    #[test]
    fn approval_urgency_ordering() {
        assert!(ApprovalUrgency::Low < ApprovalUrgency::Medium);
        assert!(ApprovalUrgency::Medium < ApprovalUrgency::High);
        assert!(ApprovalUrgency::High < ApprovalUrgency::Critical);
    }

    #[test]
    fn approval_urgency_serde() {
        let variants = [
            (ApprovalUrgency::Low, "low"),
            (ApprovalUrgency::Medium, "medium"),
            (ApprovalUrgency::High, "high"),
            (ApprovalUrgency::Critical, "critical"),
        ];
        for (urgency, expected) in variants {
            let json = serde_json::to_string(&urgency).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
            let parsed: ApprovalUrgency = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, urgency);
        }
    }

    #[test]
    fn risk_threshold_multiple_thresholds() {
        let policy = ApprovalPolicy {
            risk_thresholds: vec![
                RiskThreshold {
                    min_risk_score: 0.1,
                    chain: vec![ApprovalTarget::Role { name: "low".to_string() }],
                    required_approvals: 1,
                },
                RiskThreshold {
                    min_risk_score: 0.5,
                    chain: vec![ApprovalTarget::Role { name: "medium".to_string() }],
                    required_approvals: 1,
                },
                RiskThreshold {
                    min_risk_score: 0.9,
                    chain: vec![ApprovalTarget::Role { name: "critical".to_string() }],
                    required_approvals: 2,
                },
            ],
            fallback_chain: vec![],
        };

        // Score 0.95 → critical threshold.
        let chain = ApprovalRouter::best_chain(&policy, Some(0.95));
        assert_eq!(chain.len(), 1);
        if let ApprovalTarget::Role { name } = &chain[0] {
            assert_eq!(name, "critical");
        }

        // Score 0.6 → medium threshold.
        let chain = ApprovalRouter::best_chain(&policy, Some(0.6));
        if let ApprovalTarget::Role { name } = &chain[0] {
            assert_eq!(name, "medium");
        }

        // Score 0.2 → low threshold.
        let chain = ApprovalRouter::best_chain(&policy, Some(0.2));
        if let ApprovalTarget::Role { name } = &chain[0] {
            assert_eq!(name, "low");
        }
    }
}
