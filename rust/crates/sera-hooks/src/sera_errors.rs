//! `From` impl bridging [`HookError`] into [`SeraError`].

use sera_errors::{SeraError, SeraErrorCode};

use crate::error::HookError;

impl From<HookError> for SeraError {
    fn from(err: HookError) -> Self {
        let code = match &err {
            HookError::HookNotFound { .. } => SeraErrorCode::NotFound,
            HookError::InitFailed { .. } => SeraErrorCode::Internal,
            HookError::ExecutionFailed { .. } => SeraErrorCode::Internal,
            HookError::ChainTimeout { .. } => SeraErrorCode::Timeout,
            HookError::HookTimeout { .. } => SeraErrorCode::Timeout,
            HookError::InvalidHookPoint { .. } => SeraErrorCode::InvalidInput,
            HookError::Aborted { .. } => SeraErrorCode::Forbidden,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HookAbortSignal;
    use sera_types::hook::HookPoint;

    #[test]
    fn hook_not_found_maps_to_not_found() {
        let e: SeraError = HookError::HookNotFound {
            name: "my-hook".into(),
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
        assert!(e.message.contains("my-hook"));
    }

    #[test]
    fn init_failed_maps_to_internal() {
        let e: SeraError = HookError::InitFailed {
            hook: "h".into(),
            reason: "missing config".into(),
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::Internal);
    }

    #[test]
    fn chain_timeout_maps_to_timeout() {
        let e: SeraError = HookError::ChainTimeout {
            chain: "pre-exec".into(),
            elapsed_ms: 5000,
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::Timeout);
        assert!(e.message.contains("5000ms"));
    }

    #[test]
    fn hook_timeout_maps_to_timeout() {
        let e: SeraError = HookError::HookTimeout {
            hook: "slow-hook".into(),
            elapsed_ms: 1000,
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::Timeout);
    }

    #[test]
    fn invalid_hook_point_maps_to_invalid_input() {
        let e: SeraError = HookError::InvalidHookPoint {
            hook: "h".into(),
            point: HookPoint::PreTool,
            supported: vec![HookPoint::PostTool],
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
    }

    #[test]
    fn aborted_maps_to_forbidden() {
        let signal = HookAbortSignal::with_code("policy violation", "policy_violation");
        let e: SeraError = HookError::Aborted {
            hook: "policy-hook".into(),
            reason: "policy violation".into(),
            signal,
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
        assert!(e.message.contains("policy violation"));
    }
}
