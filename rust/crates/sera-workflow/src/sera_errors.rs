//! `From` impls bridging [`WorkflowError`] into [`SeraError`].

use sera_errors::{SeraError, SeraErrorCode};

use crate::error::WorkflowError;

impl From<WorkflowError> for SeraError {
    fn from(err: WorkflowError) -> Self {
        let code = match &err {
            WorkflowError::WorkflowNotFound { .. } => SeraErrorCode::NotFound,
            WorkflowError::InvalidCronExpression { .. } => SeraErrorCode::InvalidInput,
            WorkflowError::DuplicateWorkflow { .. } => SeraErrorCode::AlreadyExists,
            WorkflowError::WorkflowDisabled { .. } => SeraErrorCode::PreconditionFailed,
            WorkflowError::ClaimFailed { .. } => SeraErrorCode::PreconditionFailed,
            WorkflowError::TaskNotFound { .. } => SeraErrorCode::NotFound,
            WorkflowError::BudgetExhausted { .. } => SeraErrorCode::ResourceExhausted,
            WorkflowError::NRoundExceeded { .. } => SeraErrorCode::ResourceExhausted,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_not_found_maps_to_not_found() {
        let e: SeraError = WorkflowError::WorkflowNotFound { name: "daily-sync".into() }.into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
        assert!(e.message.contains("daily-sync"));
    }

    #[test]
    fn invalid_cron_maps_to_invalid_input() {
        let e: SeraError = WorkflowError::InvalidCronExpression {
            expression: "bad".into(),
            reason: "parse failed".into(),
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
        assert!(e.message.contains("bad"));
    }

    #[test]
    fn duplicate_workflow_maps_to_already_exists() {
        let e: SeraError = WorkflowError::DuplicateWorkflow { name: "nightly".into() }.into();
        assert_eq!(e.code, SeraErrorCode::AlreadyExists);
    }

    #[test]
    fn workflow_disabled_maps_to_precondition_failed() {
        let e: SeraError = WorkflowError::WorkflowDisabled { name: "disabled-wf".into() }.into();
        assert_eq!(e.code, SeraErrorCode::PreconditionFailed);
        assert!(e.message.contains("disabled-wf"));
    }

    #[test]
    fn claim_failed_maps_to_precondition_failed() {
        let e: SeraError = WorkflowError::ClaimFailed { reason: "lock held".into() }.into();
        assert_eq!(e.code, SeraErrorCode::PreconditionFailed);
        assert!(e.message.contains("lock held"));
    }

    #[test]
    fn task_not_found_maps_to_not_found() {
        let e: SeraError = WorkflowError::TaskNotFound { id: "task-42".into() }.into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
        assert!(e.message.contains("task-42"));
    }

    #[test]
    fn budget_exhausted_maps_to_resource_exhausted() {
        let e: SeraError = WorkflowError::BudgetExhausted { cost_usd: 1.5 }.into();
        assert_eq!(e.code, SeraErrorCode::ResourceExhausted);
    }

    #[test]
    fn nround_exceeded_maps_to_resource_exhausted() {
        let e: SeraError = WorkflowError::NRoundExceeded { max_rounds: 10 }.into();
        assert_eq!(e.code, SeraErrorCode::ResourceExhausted);
        assert!(e.message.contains("10"));
    }
}
