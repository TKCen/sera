//! `From` impl bridging [`HitlError`] into [`SeraError`].

use sera_errors::{SeraError, SeraErrorCode};

use crate::error::HitlError;

impl From<HitlError> for SeraError {
    fn from(err: HitlError) -> Self {
        let code = match &err {
            HitlError::TicketNotFound { .. } => SeraErrorCode::NotFound,
            HitlError::TicketExpired { .. } => SeraErrorCode::PreconditionFailed,
            HitlError::InvalidTransition { .. } => SeraErrorCode::InvalidInput,
            HitlError::EscalationExhausted { .. } => SeraErrorCode::ResourceExhausted,
            HitlError::InsufficientApprovals { .. } => SeraErrorCode::PreconditionFailed,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticket::TicketStatus;

    #[test]
    fn ticket_not_found_maps_to_not_found() {
        let e: SeraError = HitlError::TicketNotFound { id: "t-1".into() }.into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
        assert!(e.message.contains("t-1"));
    }

    #[test]
    fn ticket_expired_maps_to_precondition_failed() {
        let e: SeraError = HitlError::TicketExpired { id: "t-2".into() }.into();
        assert_eq!(e.code, SeraErrorCode::PreconditionFailed);
    }

    #[test]
    fn invalid_transition_maps_to_invalid_input() {
        let e: SeraError = HitlError::InvalidTransition {
            from: TicketStatus::Pending,
            action: "approve".into(),
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
        assert!(e.message.contains("approve"));
    }

    #[test]
    fn escalation_exhausted_maps_to_resource_exhausted() {
        let e: SeraError = HitlError::EscalationExhausted {
            ticket_id: "t-3".into(),
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::ResourceExhausted);
    }

    #[test]
    fn insufficient_approvals_maps_to_precondition_failed() {
        let e: SeraError = HitlError::InsufficientApprovals { have: 1, need: 2 }.into();
        assert_eq!(e.code, SeraErrorCode::PreconditionFailed);
        assert!(e.message.contains("have 1"));
    }
}
