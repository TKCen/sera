//! `From` impls bridging sera-models error types into [`SeraError`].
//!
//! Note: `From<ModelError> for SeraError` lives in `sera-types` (orphan rule)
//! since `ModelError` is now defined there. This module provides impls for
//! sera-models-local error types only (`RoutingError`, `CatalogError`).

use sera_errors::{SeraError, SeraErrorCode};

use crate::routing::{CatalogError, RoutingError};

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
