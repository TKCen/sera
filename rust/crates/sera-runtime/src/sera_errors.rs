//! `From` impls bridging sera-runtime error types into [`SeraError`].
//!
//! Covers:
//! - [`RuntimeError`] — top-level runtime errors
//! - [`ThinkError`] — LLM think-step errors
//! - [`ToolError`] — tool dispatch errors
//! - [`DelegationError`] — handoff/delegation errors
//! - [`SubagentError`] — subagent lifecycle errors
//! - [`LlmError`] — LLM client errors
//! - [`HarnessError`] — test harness errors
//! - [`ContextError`] — context engine errors

use sera_errors::{SeraError, SeraErrorCode};

use crate::context_engine::ContextError;
use crate::error::RuntimeError;
use crate::handoff::DelegationError;
use crate::harness::HarnessError;
use crate::llm_client::LlmError;
use crate::subagent::SubagentError;
use crate::turn::{ThinkError, ToolError};

impl From<RuntimeError> for SeraError {
    fn from(err: RuntimeError) -> Self {
        let code = match &err {
            RuntimeError::Llm(_) => SeraErrorCode::Internal,
            RuntimeError::Tool(_) => SeraErrorCode::Internal,
            RuntimeError::ContextOverflow(_) => SeraErrorCode::ResourceExhausted,
            RuntimeError::Io(_) => SeraErrorCode::Internal,
            RuntimeError::Json(_) => SeraErrorCode::Serialization,
            RuntimeError::Http(_) => SeraErrorCode::Unavailable,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<ThinkError> for SeraError {
    fn from(err: ThinkError) -> Self {
        SeraError::with_source(SeraErrorCode::Internal, err.to_string(), err)
    }
}

impl From<ToolError> for SeraError {
    fn from(err: ToolError) -> Self {
        let code = match &err {
            ToolError::NotFound(_) => SeraErrorCode::NotFound,
            ToolError::ExecutionFailed(_) => SeraErrorCode::Internal,
            ToolError::InvalidArguments(_) => SeraErrorCode::InvalidInput,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<DelegationError> for SeraError {
    fn from(err: DelegationError) -> Self {
        let code = match &err {
            DelegationError::MaxDepthExceeded { .. } => SeraErrorCode::ResourceExhausted,
            DelegationError::TargetNotAllowed { .. } => SeraErrorCode::Forbidden,
            DelegationError::TargetNotFound { .. } => SeraErrorCode::NotFound,
            DelegationError::Timeout { .. } => SeraErrorCode::Timeout,
            DelegationError::Transport(_) => SeraErrorCode::Unavailable,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<SubagentError> for SeraError {
    fn from(err: SubagentError) -> Self {
        let code = match &err {
            SubagentError::NotFound { .. } => SeraErrorCode::NotFound,
            SubagentError::SpawnFailed { .. } => SeraErrorCode::Internal,
            SubagentError::NotActive { .. } => SeraErrorCode::PreconditionFailed,
            SubagentError::Internal(_) => SeraErrorCode::Internal,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<LlmError> for SeraError {
    fn from(err: LlmError) -> Self {
        let code = match &err {
            LlmError::ContextOverflow(_) => SeraErrorCode::ResourceExhausted,
            LlmError::RateLimited(_) => SeraErrorCode::RateLimited,
            LlmError::ProviderUnavailable(_) => SeraErrorCode::Unavailable,
            LlmError::Timeout(_) => SeraErrorCode::Timeout,
            LlmError::RequestError(_) => SeraErrorCode::Internal,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<HarnessError> for SeraError {
    fn from(err: HarnessError) -> Self {
        let code = match &err {
            HarnessError::Internal(_) => SeraErrorCode::Internal,
            HarnessError::NotSupported(_) => SeraErrorCode::NotImplemented,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<ContextError> for SeraError {
    fn from(err: ContextError) -> Self {
        let code = match &err {
            ContextError::Internal(_) => SeraErrorCode::Internal,
            ContextError::BudgetExceeded { .. } => SeraErrorCode::ResourceExhausted,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- RuntimeError ---

    #[test]
    fn runtime_llm_maps_to_internal() {
        let e: SeraError = RuntimeError::Llm("provider error".into()).into();
        assert_eq!(e.code, SeraErrorCode::Internal);
        assert!(e.message.contains("provider error"));
    }

    #[test]
    fn runtime_context_overflow_maps_to_resource_exhausted() {
        let e: SeraError = RuntimeError::ContextOverflow(512).into();
        assert_eq!(e.code, SeraErrorCode::ResourceExhausted);
    }

    #[test]
    fn runtime_json_maps_to_serialization() {
        let json_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
        let e: SeraError = RuntimeError::Json(json_err).into();
        assert_eq!(e.code, SeraErrorCode::Serialization);
    }

    // --- ThinkError ---

    #[test]
    fn think_llm_maps_to_internal() {
        let e: SeraError = ThinkError::Llm("LLM timeout".into()).into();
        assert_eq!(e.code, SeraErrorCode::Internal);
        assert!(e.message.contains("LLM timeout"));
    }

    #[test]
    fn think_conversion_maps_to_internal() {
        let e: SeraError = ThinkError::Conversion("bad type".into()).into();
        assert_eq!(e.code, SeraErrorCode::Internal);
    }

    // --- ToolError ---

    #[test]
    fn tool_not_found_maps_to_not_found() {
        let e: SeraError = ToolError::NotFound("bash".into()).into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
        assert!(e.message.contains("bash"));
    }

    #[test]
    fn tool_execution_failed_maps_to_internal() {
        let e: SeraError = ToolError::ExecutionFailed("exit 1".into()).into();
        assert_eq!(e.code, SeraErrorCode::Internal);
    }

    #[test]
    fn tool_invalid_arguments_maps_to_invalid_input() {
        let e: SeraError = ToolError::InvalidArguments("missing field".into()).into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
    }

    // --- DelegationError ---

    #[test]
    fn delegation_max_depth_maps_to_resource_exhausted() {
        let e: SeraError =
            DelegationError::MaxDepthExceeded { depth: 5, max_depth: 3 }.into();
        assert_eq!(e.code, SeraErrorCode::ResourceExhausted);
        assert!(e.message.contains("5"));
    }

    #[test]
    fn delegation_target_not_allowed_maps_to_forbidden() {
        let e: SeraError =
            DelegationError::TargetNotAllowed { target: "evil-agent".into() }.into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
        assert!(e.message.contains("evil-agent"));
    }

    #[test]
    fn delegation_target_not_found_maps_to_not_found() {
        let e: SeraError =
            DelegationError::TargetNotFound { target: "missing-agent".into() }.into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
    }

    #[test]
    fn delegation_timeout_maps_to_timeout() {
        let e: SeraError = DelegationError::Timeout {
            target: "slow-agent".into(),
            elapsed_secs: 30.0,
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::Timeout);
    }

    #[test]
    fn delegation_transport_maps_to_unavailable() {
        let e: SeraError =
            DelegationError::Transport(anyhow::anyhow!("channel closed")).into();
        assert_eq!(e.code, SeraErrorCode::Unavailable);
    }

    // --- LlmError ---

    #[test]
    fn llm_rate_limited_maps_to_rate_limited() {
        let e: SeraError = LlmError::RateLimited("429".into()).into();
        assert_eq!(e.code, SeraErrorCode::RateLimited);
    }

    #[test]
    fn llm_provider_unavailable_maps_to_unavailable() {
        let e: SeraError = LlmError::ProviderUnavailable("503".into()).into();
        assert_eq!(e.code, SeraErrorCode::Unavailable);
    }

    #[test]
    fn llm_context_overflow_maps_to_resource_exhausted() {
        let e: SeraError = LlmError::ContextOverflow("128k exceeded".into()).into();
        assert_eq!(e.code, SeraErrorCode::ResourceExhausted);
    }

    #[test]
    fn llm_timeout_maps_to_timeout() {
        let e: SeraError = LlmError::Timeout("30s".into()).into();
        assert_eq!(e.code, SeraErrorCode::Timeout);
    }

    // --- ContextError ---

    #[test]
    fn context_budget_exceeded_maps_to_resource_exhausted() {
        let e: SeraError = ContextError::BudgetExceeded { limit: 4096, actual: 5000 }.into();
        assert_eq!(e.code, SeraErrorCode::ResourceExhausted);
        assert!(e.message.contains("4096"));
    }

    #[test]
    fn context_internal_maps_to_internal() {
        let e: SeraError = ContextError::Internal("cache miss".into()).into();
        assert_eq!(e.code, SeraErrorCode::Internal);
    }
}
