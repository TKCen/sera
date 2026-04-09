//! Error types for the sera-hitl crate.

use crate::ticket::TicketStatus;
use thiserror::Error;

/// Errors that can occur in the HITL approval system.
#[derive(Debug, Error)]
pub enum HitlError {
    /// The requested approval ticket does not exist.
    #[error("ticket not found: {id}")]
    TicketNotFound { id: String },

    /// The ticket has already expired.
    #[error("ticket expired: {id}")]
    TicketExpired { id: String },

    /// The requested state transition is not valid for this ticket.
    #[error("invalid transition from {from:?}: {action}")]
    InvalidTransition { from: TicketStatus, action: String },

    /// All escalation targets have been exhausted with no resolution.
    #[error("escalation chain exhausted for ticket: {ticket_id}")]
    EscalationExhausted { ticket_id: String },

    /// Not enough approvals have been collected to satisfy the requirement.
    #[error("insufficient approvals: have {have}, need {need}")]
    InsufficientApprovals { have: u32, need: u32 },
}
