//! `From` impls bridging sera-models error types into [`SeraError`].

use sera_errors::{SeraError, SeraErrorCode};

use crate::error::ModelError;
use crate::routing::{CatalogError, RoutingError};

impl From<ModelError> for SeraError {
    fn from(err: ModelError) -> Self {
        let code = match &err {
            ModelError::Provider(_) => SeraErrorCode::Internal,
            ModelError::Serialization(_) => SeraErrorCode::Serialization,
            ModelError::Http(_) => SeraErrorCode::Unavailable,
            ModelError::InvalidResponse(_) => SeraErrorCode::Internal,
            ModelError::Authentication(_) => SeraErrorCode::Unauthorized,
            ModelError::RateLimit => SeraErrorCode::RateLimited,
            ModelError::ContextLengthExceeded => SeraErrorCode::InvalidInput,
            ModelError::NotAvailable(_) => SeraErrorCode::Unavailable,
            ModelError::Timeout => SeraErrorCode::Timeout,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<RoutingError> for SeraError {
    fn from(err: RoutingError) -> Self {
        let code = match &err {
            RoutingError::InvalidWeights { .. } => SeraErrorCode::InvalidInput,
            RoutingError::WeightOutOfRange { .. } => SeraErrorCode::InvalidInput,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<CatalogError> for SeraError {
    fn from(err: CatalogError) -> Self {
        let code = match &err {
            CatalogError::Upstream(_) => SeraErrorCode::Unavailable,
            CatalogError::Invalid(_) => SeraErrorCode::Internal,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_authentication_maps_to_unauthorized() {
        let e: SeraError = ModelError::Authentication("bad key".into()).into();
        assert_eq!(e.code, SeraErrorCode::Unauthorized);
        assert!(e.message.contains("authentication failed"));
    }

    #[test]
    fn model_rate_limit_maps_to_rate_limited() {
        let e: SeraError = ModelError::RateLimit.into();
        assert_eq!(e.code, SeraErrorCode::RateLimited);
    }

    #[test]
    fn model_timeout_maps_to_timeout() {
        let e: SeraError = ModelError::Timeout.into();
        assert_eq!(e.code, SeraErrorCode::Timeout);
    }

    #[test]
    fn model_not_available_maps_to_unavailable() {
        let e: SeraError = ModelError::NotAvailable("openai".into()).into();
        assert_eq!(e.code, SeraErrorCode::Unavailable);
        assert!(e.message.contains("openai"));
    }

    #[test]
    fn routing_invalid_weights_maps_to_invalid_input() {
        let e: SeraError = RoutingError::InvalidWeights { sum: 1.5, epsilon: 1e-6 }.into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
        assert!(e.message.contains("weights"));
    }

    #[test]
    fn routing_weight_out_of_range_maps_to_invalid_input() {
        let e: SeraError = RoutingError::WeightOutOfRange { name: "w_latency", value: -0.1 }.into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
    }

    #[test]
    fn catalog_upstream_maps_to_unavailable() {
        let e: SeraError = CatalogError::Upstream("poll failed".into()).into();
        assert_eq!(e.code, SeraErrorCode::Unavailable);
        assert!(e.message.contains("poll failed"));
    }

    #[test]
    fn catalog_invalid_maps_to_internal() {
        let e: SeraError = CatalogError::Invalid("bad json".into()).into();
        assert_eq!(e.code, SeraErrorCode::Internal);
    }
}
