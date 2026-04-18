//! `From` impls bridging sera-telemetry error types into [`SeraError`].

use sera_errors::{SeraError, SeraErrorCode};

use crate::audit::AuditError;
use crate::otel::OtelInitError;

impl From<AuditError> for SeraError {
    fn from(err: AuditError) -> Self {
        let code = match &err {
            AuditError::NotInitialised => SeraErrorCode::PreconditionFailed,
            AuditError::ChainBroken { .. } => SeraErrorCode::Internal,
            AuditError::Write { .. } => SeraErrorCode::Internal,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<OtelInitError> for SeraError {
    fn from(err: OtelInitError) -> Self {
        let code = match &err {
            OtelInitError::InvalidEndpoint { .. } => SeraErrorCode::InvalidInput,
            OtelInitError::ProviderSetup { .. } => SeraErrorCode::Configuration,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_not_initialised_maps_to_precondition_failed() {
        let e: SeraError = AuditError::NotInitialised.into();
        assert_eq!(e.code, SeraErrorCode::PreconditionFailed);
        assert!(e.message.contains("not initialised"));
    }

    #[test]
    fn audit_chain_broken_maps_to_internal() {
        let e: SeraError = AuditError::ChainBroken { index: 3 }.into();
        assert_eq!(e.code, SeraErrorCode::Internal);
        assert!(e.message.contains("broken"));
    }

    #[test]
    fn audit_write_maps_to_internal() {
        let e: SeraError = AuditError::Write { reason: "disk full".into() }.into();
        assert_eq!(e.code, SeraErrorCode::Internal);
        assert!(e.message.contains("disk full"));
    }

    #[test]
    fn otel_invalid_endpoint_maps_to_invalid_input() {
        let e: SeraError = OtelInitError::InvalidEndpoint { reason: "empty".into() }.into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
        assert!(e.message.contains("invalid endpoint"));
    }

    #[test]
    fn otel_provider_setup_maps_to_configuration() {
        let e: SeraError = OtelInitError::ProviderSetup { reason: "tls failure".into() }.into();
        assert_eq!(e.code, SeraErrorCode::Configuration);
        assert!(e.message.contains("tracer provider"));
    }
}
