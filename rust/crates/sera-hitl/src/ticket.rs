//! ApprovalTicket and its state machine.
//!
//! A ticket is created Pending, then transitions through Approved/Rejected/Escalated/Expired
//! as approvers act on it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sera_domain::principal::PrincipalRef;
use uuid::Uuid;

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
    /// The ticket moves to `Approved` once `required_approvals` have been collected.
    /// Returns the new status.
    pub fn approve(&mut self, approver: PrincipalRef, reason: Option<String>) -> TicketStatus {
        self.decisions.push(ApprovalDecision {
            approver,
            status: TicketStatus::Approved,
            reason,
            decided_at: Utc::now(),
        });
        if self.is_fully_approved() {
            self.status = TicketStatus::Approved;
        }
        self.status
    }

    /// Record a rejection decision from `approver`.
    ///
    /// A single rejection immediately moves the ticket to `Rejected`.
    pub fn reject(&mut self, approver: PrincipalRef, reason: Option<String>) -> TicketStatus {
        self.decisions.push(ApprovalDecision {
            approver,
            status: TicketStatus::Rejected,
            reason,
            decided_at: Utc::now(),
        });
        self.status = TicketStatus::Rejected;
        self.status
    }

    /// Advance to the next target in the escalation chain.
    ///
    /// Sets status to `Escalated` and increments `current_target_index`.
    pub fn escalate(&mut self) {
        self.current_target_index += 1;
        self.status = TicketStatus::Escalated;
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
}
