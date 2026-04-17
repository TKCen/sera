//! ApprovalTicket and its state machine.
//!
//! A ticket is created Pending, then transitions through Approved/Rejected/Escalated/Expired
//! as approvers act on it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sera_types::principal::PrincipalRef;
use uuid::Uuid;

use crate::error::HitlError;
use crate::types::{ApprovalSpec, ApprovalTarget, ApprovalRouting};

// ── Status ────────────────────────────────────────────────────────────────────

/// The current lifecycle state of an approval ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketStatus {
    /// Awaiting a decision from the current target(s).
    Pending,
    /// All required approvals have been collected.
    Approved,
    /// At least one approver has rejected the request.
    Rejected,
    /// Escalated to the next target in the chain.
    Escalated,
    /// No decision was reached before the deadline.
    Expired,
}

impl TicketStatus {
    /// Returns `true` when the status represents a terminal state — one from
    /// which no further transitions are allowed. A ticket in a terminal state
    /// rejects further `approve`/`reject` calls.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Approved | Self::Rejected | Self::Expired)
    }
}

// ── Decision ──────────────────────────────────────────────────────────────────

/// A single approve-or-reject decision from one approver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalDecision {
    /// The principal who made the decision.
    pub approver: PrincipalRef,
    /// `Approved` or `Rejected` — the only valid values here.
    pub status: TicketStatus,
    /// Optional explanation provided by the approver.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// When the decision was recorded.
    pub decided_at: DateTime<Utc>,
}

// ── Ticket ────────────────────────────────────────────────────────────────────

/// An approval request in flight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalTicket {
    /// Unique ticket identifier (UUID v4).
    pub id: String,
    /// The full specification for this approval request.
    pub spec: ApprovalSpec,
    /// The session that triggered the approval request.
    pub session_id: String,
    /// Current lifecycle state.
    pub status: TicketStatus,
    /// Index into the escalation chain — which target is currently active.
    pub current_target_index: usize,
    /// All decisions recorded so far.
    pub decisions: Vec<ApprovalDecision>,
    /// When the ticket was created.
    pub created_at: DateTime<Utc>,
    /// When the ticket expires.
    pub expires_at: DateTime<Utc>,
}

impl ApprovalTicket {
    /// Create a new Pending ticket. Expiry is calculated from `spec.timeout`.
    pub fn new(spec: ApprovalSpec, session_id: impl Into<String>) -> Self {
        let now = Utc::now();
        let expires_at = now
            + chrono::Duration::from_std(spec.timeout)
                .unwrap_or(chrono::Duration::seconds(300));
        Self {
            id: Uuid::new_v4().to_string(),
            spec,
            session_id: session_id.into(),
            status: TicketStatus::Pending,
            current_target_index: 0,
            decisions: Vec::new(),
            created_at: now,
            expires_at,
        }
    }

    /// Record an approval decision from `approver`.
    ///
    /// Returns [`HitlError::InvalidTransition`] if the ticket is already in a
    /// terminal state (Approved or Rejected). If the ticket's deadline has
    /// passed, the status is flipped to `Expired` and
    /// [`HitlError::TicketExpired`] is returned — no decision is recorded.
    ///
    /// On success the new status is returned. The ticket moves to `Approved`
    /// once `required_approvals` have been collected.
    ///
    /// Duplicate-approver policy: the same principal can approve multiple
    /// times and every decision counts toward `required_approvals`. This is
    /// intentional — the state machine treats each call as a distinct vote
    /// to avoid implicitly dropping decisions that audit logs should capture.
    /// Callers that need dedupe must enforce it at the router layer.
    pub fn approve(
        &mut self,
        approver: PrincipalRef,
        reason: Option<String>,
    ) -> Result<TicketStatus, HitlError> {
        self.guard_transition("approve")?;
        self.decisions.push(ApprovalDecision {
            approver,
            status: TicketStatus::Approved,
            reason,
            decided_at: Utc::now(),
        });
        if self.is_fully_approved() {
            self.status = TicketStatus::Approved;
        }
        Ok(self.status)
    }

    /// Record a rejection decision from `approver`.
    ///
    /// Returns [`HitlError::InvalidTransition`] if the ticket is already in a
    /// terminal state (Approved, Rejected, Expired). If the ticket's deadline
    /// has passed, the status is flipped to `Expired` and
    /// [`HitlError::TicketExpired`] is returned — no decision is recorded.
    ///
    /// On success, a single rejection immediately moves the ticket to
    /// `Rejected`.
    pub fn reject(
        &mut self,
        approver: PrincipalRef,
        reason: Option<String>,
    ) -> Result<TicketStatus, HitlError> {
        self.guard_transition("reject")?;
        self.decisions.push(ApprovalDecision {
            approver,
            status: TicketStatus::Rejected,
            reason,
            decided_at: Utc::now(),
        });
        self.status = TicketStatus::Rejected;
        Ok(self.status)
    }

    /// Advance to the next target in the escalation chain.
    ///
    /// Returns [`HitlError::EscalationExhausted`] when the current position
    /// already sits at (or past) the end of the resolved chain — i.e. there
    /// is no further target to advance to. On success sets status to
    /// `Escalated` and increments `current_target_index`.
    pub fn escalate(&mut self) -> Result<(), HitlError> {
        let chain_len = self.resolved_chain_len();
        // `current_target_index` points at the currently-active target.
        // After `escalate` it must still point at a valid target, so the
        // caller can only escalate while `index + 1 < chain_len`.
        if self.current_target_index + 1 >= chain_len {
            return Err(HitlError::EscalationExhausted {
                ticket_id: self.id.clone(),
            });
        }
        self.current_target_index += 1;
        self.status = TicketStatus::Escalated;
        Ok(())
    }

    /// Returns `true` when the number of approval decisions meets or exceeds
    /// the `required_approvals` threshold in the spec.
    pub fn is_fully_approved(&self) -> bool {
        let approval_count = self
            .decisions
            .iter()
            .filter(|d| d.status == TicketStatus::Approved)
            .count() as u32;
        approval_count >= self.spec.required_approvals
    }

    /// Returns `true` when the current wall-clock time is past `expires_at`.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Resolve the currently active `ApprovalTarget`(s) based on the routing
    /// configuration and the current escalation position.
    pub fn current_targets(&self) -> Vec<&ApprovalTarget> {
        match &self.spec.routing {
            ApprovalRouting::Autonomous => vec![],
            ApprovalRouting::Static { targets } => {
                if let Some(target) = targets.get(self.current_target_index) {
                    vec![target]
                } else {
                    vec![]
                }
            }
            ApprovalRouting::Dynamic(policy) => {
                let risk_score = self.spec.evidence.risk_score;
                // Find the highest threshold that applies.
                let chain = crate::router::ApprovalRouter::best_chain(policy, risk_score);
                if let Some(target) = chain.get(self.current_target_index) {
                    vec![target]
                } else {
                    vec![]
                }
            }
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────

    /// Shared guard for `approve`/`reject`. Flips the ticket to `Expired`
    /// if the deadline has passed, then rejects any transition originating
    /// from a terminal state.
    fn guard_transition(&mut self, action: &'static str) -> Result<(), HitlError> {
        // Expiry check first: a pending ticket past its deadline is considered
        // Expired regardless of what the caller is trying to do.
        if !self.status.is_terminal() && self.is_expired() {
            self.status = TicketStatus::Expired;
            return Err(HitlError::TicketExpired { id: self.id.clone() });
        }
        if self.status.is_terminal() {
            return Err(HitlError::InvalidTransition {
                from: self.status,
                action: action.to_string(),
            });
        }
        Ok(())
    }

    /// Length of the escalation chain resolved from the current routing mode.
    /// Used by `escalate` to bound-check the advance.
    fn resolved_chain_len(&self) -> usize {
        match &self.spec.routing {
            ApprovalRouting::Autonomous => 0,
            ApprovalRouting::Static { targets } => targets.len(),
            ApprovalRouting::Dynamic(policy) => {
                let risk_score = self.spec.evidence.risk_score;
                crate::router::ApprovalRouter::best_chain(policy, risk_score).len()
            }
        }
    }
}
