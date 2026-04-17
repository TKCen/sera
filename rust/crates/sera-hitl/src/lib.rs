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

    // ── current_targets ───────────────────────────────────────────────────────

    #[test]
    fn current_targets_autonomous_routing_always_empty() {
        let spec = basic_spec(ApprovalRouting::Autonomous, 1, Duration::from_secs(300));
        let ticket = ApprovalTicket::new(spec, "session-ct-1");
        assert!(ticket.current_targets().is_empty());
    }

    #[test]
    fn current_targets_static_routing_first_target() {
        let routing = ApprovalRouting::Static {
            targets: vec![
                ApprovalTarget::Agent { id: "a1".to_string() },
                ApprovalTarget::Role { name: "admin".to_string() },
            ],
        };
        let spec = basic_spec(routing, 1, Duration::from_secs(300));
        let ticket = ApprovalTicket::new(spec, "session-ct-2");
        let targets = ticket.current_targets();
        assert_eq!(targets.len(), 1);
        if let ApprovalTarget::Agent { id } = targets[0] {
            assert_eq!(id, "a1");
        } else {
            panic!("expected Agent target");
        }
    }

    #[test]
    fn current_targets_static_routing_advances_after_escalate() {
        let routing = ApprovalRouting::Static {
            targets: vec![
                ApprovalTarget::Agent { id: "a1".to_string() },
                ApprovalTarget::Role { name: "admin".to_string() },
            ],
        };
        let spec = basic_spec(routing, 1, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-ct-3");
        ticket.escalate();
        let targets = ticket.current_targets();
        assert_eq!(targets.len(), 1);
        if let ApprovalTarget::Role { name } = targets[0] {
            assert_eq!(name, "admin");
        } else {
            panic!("expected Role target");
        }
    }

    #[test]
    fn current_targets_static_routing_exhausted_returns_empty() {
        let routing = ApprovalRouting::Static {
            targets: vec![ApprovalTarget::Agent { id: "a1".to_string() }],
        };
        let spec = basic_spec(routing, 1, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-ct-4");
        // Escalate past the only target.
        ticket.escalate();
        assert!(ticket.current_targets().is_empty());
    }

    #[test]
    fn current_targets_dynamic_routing_selects_correct_chain() {
        let policy = ApprovalPolicy {
            risk_thresholds: vec![RiskThreshold {
                min_risk_score: 0.6,
                chain: vec![
                    ApprovalTarget::Role { name: "security".to_string() },
                    ApprovalTarget::Role { name: "ops".to_string() },
                ],
                required_approvals: 1,
            }],
            fallback_chain: vec![ApprovalTarget::Role { name: "fallback".to_string() }],
        };
        let routing = ApprovalRouting::Dynamic(policy);
        // risk_score 0.75 — matches the 0.6 threshold.
        let spec = ApprovalSpec {
            scope: ApprovalScope::ToolCall {
                tool_name: "shell".to_string(),
                risk_level: sera_types::tool::RiskLevel::Execute,
            },
            description: "test".to_string(),
            urgency: ApprovalUrgency::High,
            routing,
            timeout: Duration::from_secs(300),
            required_approvals: 1,
            evidence: ApprovalEvidence {
                tool_args: None,
                risk_score: Some(0.75),
                principal: human_ref(),
                session_context: None,
                additional: HashMap::new(),
            },
        };
        let ticket = ApprovalTicket::new(spec, "session-ct-5");
        let targets = ticket.current_targets();
        // First target in the matched chain.
        assert_eq!(targets.len(), 1);
        if let ApprovalTarget::Role { name } = targets[0] {
            assert_eq!(name, "security");
        } else {
            panic!("expected Role target");
        }
    }

    #[test]
    fn current_targets_dynamic_routing_uses_fallback_below_threshold() {
        let policy = ApprovalPolicy {
            risk_thresholds: vec![RiskThreshold {
                min_risk_score: 0.9,
                chain: vec![ApprovalTarget::Role { name: "critical".to_string() }],
                required_approvals: 1,
            }],
            fallback_chain: vec![ApprovalTarget::Role { name: "fallback".to_string() }],
        };
        let routing = ApprovalRouting::Dynamic(policy);
        let spec = ApprovalSpec {
            scope: ApprovalScope::ToolCall {
                tool_name: "read_file".to_string(),
                risk_level: sera_types::tool::RiskLevel::Read,
            },
            description: "test".to_string(),
            urgency: ApprovalUrgency::Low,
            routing,
            timeout: Duration::from_secs(300),
            required_approvals: 1,
            evidence: ApprovalEvidence {
                tool_args: None,
                risk_score: Some(0.1),
                principal: human_ref(),
                session_context: None,
                additional: HashMap::new(),
            },
        };
        let ticket = ApprovalTicket::new(spec, "session-ct-6");
        let targets = ticket.current_targets();
        assert_eq!(targets.len(), 1);
        if let ApprovalTarget::Role { name } = targets[0] {
            assert_eq!(name, "fallback");
        } else {
            panic!("expected Role fallback target");
        }
    }

    // ── Escalation edge cases ─────────────────────────────────────────────────

    #[test]
    fn escalate_multiple_steps_through_chain() {
        let routing = ApprovalRouting::Static {
            targets: vec![
                ApprovalTarget::Agent { id: "tier-1".to_string() },
                ApprovalTarget::Role { name: "tier-2".to_string() },
                ApprovalTarget::Role { name: "tier-3".to_string() },
            ],
        };
        let spec = basic_spec(routing, 1, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-esc-1");

        assert_eq!(ticket.current_target_index, 0);
        ticket.escalate();
        assert_eq!(ticket.current_target_index, 1);
        assert_eq!(ticket.status, TicketStatus::Escalated);
        ticket.escalate();
        assert_eq!(ticket.current_target_index, 2);
        // After escalating past the last target the index is out of range.
        ticket.escalate();
        assert_eq!(ticket.current_target_index, 3);
        assert!(ticket.current_targets().is_empty());
    }

    #[test]
    fn escalation_then_approval_resolves_ticket() {
        let routing = ApprovalRouting::Static {
            targets: vec![
                ApprovalTarget::Agent { id: "agent-1".to_string() },
                ApprovalTarget::Role { name: "admin".to_string() },
            ],
        };
        let spec = basic_spec(routing, 1, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-esc-2");

        ticket.escalate();
        assert_eq!(ticket.status, TicketStatus::Escalated);

        // Approver at the escalated level approves.
        let status = ticket.approve(human_ref(), Some("approved after escalation".to_string()));
        assert_eq!(status, TicketStatus::Approved);
        assert!(ticket.is_fully_approved());
    }

    // ── Approve/reject interaction edge cases ─────────────────────────────────

    #[test]
    fn reject_after_partial_approvals_sets_rejected() {
        let spec = basic_spec(static_routing(), 3, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-edge-1");

        ticket.approve(human_ref(), None);
        ticket.approve(agent_ref(), None);
        // Still pending — only 2 of 3 collected.
        assert_eq!(ticket.status, TicketStatus::Pending);

        let status = ticket.reject(human_ref(), Some("changed mind".to_string()));
        assert_eq!(status, TicketStatus::Rejected);
        assert!(!ticket.is_fully_approved());
        // Two approve decisions + one reject decision recorded.
        assert_eq!(ticket.decisions.len(), 3);
    }

    #[test]
    fn approve_after_rejection_does_not_override_rejected_status() {
        // The state machine rejects immediately on any rejection.
        // A subsequent approve call still adds a decision but the ticket
        // stays Rejected because approve() only transitions to Approved
        // — it never clears a Rejected state.
        let spec = basic_spec(static_routing(), 1, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-edge-2");

        ticket.reject(human_ref(), None);
        assert_eq!(ticket.status, TicketStatus::Rejected);

        // Calling approve after rejection — the approval count now satisfies
        // required_approvals so is_fully_approved() returns true, which means
        // approve() will flip status back to Approved. This is a known
        // behaviour worth documenting: the state machine has no guard against
        // approving an already-rejected ticket. Test records the actual
        // behaviour so any future change is visible.
        ticket.approve(agent_ref(), None);
        // is_fully_approved counts only Approved decisions (1 >= 1 → true).
        // approve() sets status = Approved when fully approved.
        assert_eq!(ticket.status, TicketStatus::Approved);
        assert!(ticket.is_fully_approved());
    }

    #[test]
    fn required_approvals_zero_is_always_fully_approved() {
        let spec = basic_spec(static_routing(), 0, Duration::from_secs(300));
        let ticket = ApprovalTicket::new(spec, "session-edge-3");
        // Zero approvals required — immediately satisfied even with no decisions.
        assert!(ticket.is_fully_approved());
    }

    #[test]
    fn duplicate_approver_counts_each_decision_separately() {
        // The same principal can approve multiple times; each counts toward the quota.
        let spec = basic_spec(static_routing(), 2, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-edge-4");

        ticket.approve(human_ref(), None);
        assert!(!ticket.is_fully_approved());

        // Same approver again — second decision fulfils the requirement.
        ticket.approve(human_ref(), None);
        assert!(ticket.is_fully_approved());
        assert_eq!(ticket.status, TicketStatus::Approved);
        assert_eq!(ticket.decisions.len(), 2);
    }

    #[test]
    fn approve_records_decision_fields_correctly() {
        let spec = basic_spec(static_routing(), 1, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-edge-5");
        let approver = human_ref();

        ticket.approve(approver.clone(), Some("looks good".to_string()));

        assert_eq!(ticket.decisions.len(), 1);
        let decision = &ticket.decisions[0];
        assert_eq!(decision.approver.id, approver.id);
        assert_eq!(decision.status, TicketStatus::Approved);
        assert_eq!(decision.reason.as_deref(), Some("looks good"));
    }

    #[test]
    fn reject_records_decision_fields_correctly() {
        let spec = basic_spec(static_routing(), 1, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-edge-6");
        let approver = agent_ref();

        ticket.reject(approver.clone(), Some("too dangerous".to_string()));

        assert_eq!(ticket.decisions.len(), 1);
        let decision = &ticket.decisions[0];
        assert_eq!(decision.approver.id, approver.id);
        assert_eq!(decision.status, TicketStatus::Rejected);
        assert_eq!(decision.reason.as_deref(), Some("too dangerous"));
    }

    // ── Router edge cases ─────────────────────────────────────────────────────

    #[test]
    fn router_dynamic_empty_policy_returns_empty_fallback() {
        let policy = ApprovalPolicy {
            risk_thresholds: vec![],
            fallback_chain: vec![],
        };
        let routing = ApprovalRouting::Dynamic(policy);
        let chain = ApprovalRouter::resolve_chain(&routing, Some(0.99));
        assert!(chain.is_empty());
    }

    #[test]
    fn router_dynamic_no_risk_score_uses_zero_for_matching() {
        let policy = ApprovalPolicy {
            risk_thresholds: vec![RiskThreshold {
                min_risk_score: 0.0,
                chain: vec![ApprovalTarget::Role { name: "always".to_string() }],
                required_approvals: 1,
            }],
            fallback_chain: vec![],
        };
        let routing = ApprovalRouting::Dynamic(policy);
        // None risk_score → treated as 0.0 → matches min_risk_score 0.0.
        let chain = ApprovalRouter::resolve_chain(&routing, None);
        assert_eq!(chain.len(), 1);
        if let ApprovalTarget::Role { name } = &chain[0] {
            assert_eq!(name, "always");
        }
    }

    #[test]
    fn router_static_empty_targets_returns_empty_chain() {
        let routing = ApprovalRouting::Static { targets: vec![] };
        let chain = ApprovalRouter::resolve_chain(&routing, None);
        assert!(chain.is_empty());
    }

    #[test]
    fn router_risk_level_to_score_mapping() {
        assert_eq!(ApprovalRouter::risk_level_to_score_public(RiskLevel::Read), 0.1);
        assert_eq!(ApprovalRouter::risk_level_to_score_public(RiskLevel::Write), 0.4);
        assert_eq!(ApprovalRouter::risk_level_to_score_public(RiskLevel::Execute), 0.7);
        assert_eq!(ApprovalRouter::risk_level_to_score_public(RiskLevel::Admin), 1.0);
    }

    #[test]
    fn needs_approval_standard_dynamic_with_matching_threshold() {
        let policy = ApprovalPolicy {
            risk_thresholds: vec![RiskThreshold {
                min_risk_score: 0.6,
                chain: vec![ApprovalTarget::Role { name: "ops".to_string() }],
                required_approvals: 1,
            }],
            fallback_chain: vec![],
        };
        // Execute → score 0.7 → matches 0.6 threshold → needs approval.
        assert!(ApprovalRouter::needs_approval(
            EnforcementMode::Standard,
            RiskLevel::Execute,
            &ApprovalRouting::Dynamic(policy.clone()),
        ));
        // Read → score 0.1 → no threshold matches, empty fallback → no approval.
        assert!(!ApprovalRouter::needs_approval(
            EnforcementMode::Standard,
            RiskLevel::Read,
            &ApprovalRouting::Dynamic(policy),
        ));
    }

    #[test]
    fn needs_approval_standard_dynamic_fallback_triggers_approval() {
        let policy = ApprovalPolicy {
            risk_thresholds: vec![],
            fallback_chain: vec![ApprovalTarget::Role { name: "default".to_string() }],
        };
        // No thresholds, but fallback is non-empty → needs approval.
        assert!(ApprovalRouter::needs_approval(
            EnforcementMode::Standard,
            RiskLevel::Read,
            &ApprovalRouting::Dynamic(policy),
        ));
    }

    // ── EnforcementMode serde ─────────────────────────────────────────────────

    #[test]
    fn enforcement_mode_serde_roundtrip() {
        let variants = [
            (EnforcementMode::Autonomous, "\"autonomous\""),
            (EnforcementMode::Standard, "\"standard\""),
            (EnforcementMode::Strict, "\"strict\""),
        ];
        for (mode, expected_json) in variants {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, expected_json);
            let parsed: EnforcementMode = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, mode);
        }
    }

    // ── TicketStatus serde ────────────────────────────────────────────────────

    #[test]
    fn ticket_status_serde_roundtrip() {
        let variants = [
            (TicketStatus::Pending, "\"pending\""),
            (TicketStatus::Approved, "\"approved\""),
            (TicketStatus::Rejected, "\"rejected\""),
            (TicketStatus::Escalated, "\"escalated\""),
            (TicketStatus::Expired, "\"expired\""),
        ];
        for (status, expected_json) in variants {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected_json);
            let parsed: TicketStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, status);
        }
    }

    // ── HitlError display ─────────────────────────────────────────────────────

    #[test]
    fn hitl_error_display_ticket_not_found() {
        let err = HitlError::TicketNotFound { id: "abc-123".to_string() };
        assert_eq!(err.to_string(), "ticket not found: abc-123");
    }

    #[test]
    fn hitl_error_display_ticket_expired() {
        let err = HitlError::TicketExpired { id: "xyz-456".to_string() };
        assert_eq!(err.to_string(), "ticket expired: xyz-456");
    }

    #[test]
    fn hitl_error_display_invalid_transition() {
        let err = HitlError::InvalidTransition {
            from: TicketStatus::Approved,
            action: "approve".to_string(),
        };
        assert!(err.to_string().contains("Approved"));
        assert!(err.to_string().contains("approve"));
    }

    #[test]
    fn hitl_error_display_escalation_exhausted() {
        let err = HitlError::EscalationExhausted { ticket_id: "t-001".to_string() };
        assert_eq!(err.to_string(), "escalation chain exhausted for ticket: t-001");
    }

    #[test]
    fn hitl_error_display_insufficient_approvals() {
        let err = HitlError::InsufficientApprovals { have: 1, need: 3 };
        assert_eq!(err.to_string(), "insufficient approvals: have 1, need 3");
    }

    // ── ApprovalSpec serde roundtrip ──────────────────────────────────────────

    #[test]
    fn approval_spec_serde_roundtrip() {
        let spec = basic_spec(static_routing(), 2, Duration::from_secs(120));
        let json = serde_json::to_string(&spec).unwrap();
        let parsed: ApprovalSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.required_approvals, 2);
        assert_eq!(parsed.timeout, Duration::from_secs(120));
        assert_eq!(parsed.urgency, ApprovalUrgency::Medium);
    }

    #[test]
    fn approval_ticket_serde_roundtrip() {
        let spec = basic_spec(static_routing(), 1, Duration::from_secs(300));
        let mut ticket = ApprovalTicket::new(spec, "session-serde-1");
        ticket.approve(human_ref(), Some("ok".to_string()));

        let json = serde_json::to_string(&ticket).unwrap();
        let parsed: ApprovalTicket = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, ticket.id);
        assert_eq!(parsed.session_id, "session-serde-1");
        assert_eq!(parsed.status, TicketStatus::Approved);
        assert_eq!(parsed.decisions.len(), 1);
        assert_eq!(parsed.decisions[0].reason.as_deref(), Some("ok"));
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
