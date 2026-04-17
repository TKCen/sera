//! `From` impls bridging sera-session error types into [`SeraError`].
//!
//! Covers:
//! - [`SessionStateError`] — FSM transition errors
//! - [`PersistenceError`] — transcript persistence errors

use sera_errors::{SeraError, SeraErrorCode};

use crate::persistence::PersistenceError;
use crate::state::SessionStateError;

impl From<SessionStateError> for SeraError {
    fn from(err: SessionStateError) -> Self {
        // The only variant is InvalidTransition — always an invalid caller action.
        SeraError::with_source(SeraErrorCode::InvalidInput, err.to_string(), err)
    }
}

impl From<PersistenceError> for SeraError {
    fn from(err: PersistenceError) -> Self {
        let code = match &err {
            PersistenceError::NotFound(_) => SeraErrorCode::NotFound,
            PersistenceError::Io(_) => SeraErrorCode::Internal,
            PersistenceError::Serialization(_) => SeraErrorCode::Serialization,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SessionState;

    // --- SessionStateError ---

    #[test]
    fn invalid_transition_maps_to_invalid_input() {
        let e: SeraError = SessionStateError::InvalidTransition {
            from: SessionState::Created,
            to: SessionState::Idle,
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
        assert!(e.message.contains("Created") || e.message.contains("created"));
    }

    #[test]
    fn invalid_transition_message_contains_target_state() {
        let e: SeraError = SessionStateError::InvalidTransition {
            from: SessionState::Closed,
            to: SessionState::Active,
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
        assert!(e.message.contains("Closed") || e.message.contains("closed"));
    }

    // --- PersistenceError ---

    #[test]
    fn persistence_not_found_maps_to_not_found() {
        let e: SeraError = PersistenceError::NotFound("session-xyz.json".into()).into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
        assert!(e.message.contains("session-xyz.json"));
    }

    #[test]
    fn persistence_io_maps_to_internal() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no write");
        let e: SeraError = PersistenceError::Io(io_err).into();
        assert_eq!(e.code, SeraErrorCode::Internal);
    }

    #[test]
    fn persistence_serialization_maps_to_serialization() {
        let json_err = serde_json::from_str::<serde_json::Value>("not-json").unwrap_err();
        let e: SeraError = PersistenceError::Serialization(json_err).into();
        assert_eq!(e.code, SeraErrorCode::Serialization);
    }
}
