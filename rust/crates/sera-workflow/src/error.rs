use thiserror::Error;

/// Errors produced by the workflow engine.
#[derive(Debug, Error)]
pub enum WorkflowError {
    /// No workflow with the given name is registered.
    #[error("workflow not found: {name}")]
    WorkflowNotFound { name: String },

    /// The cron expression is syntactically invalid.
    #[error("invalid cron expression '{expression}': {reason}")]
    InvalidCronExpression { expression: String, reason: String },

    /// A workflow with this name is already registered.
    #[error("duplicate workflow: {name}")]
    DuplicateWorkflow { name: String },

    /// The workflow exists but is currently disabled.
    #[error("workflow is disabled: {name}")]
    WorkflowDisabled { name: String },

    /// An atomic claim operation failed.
    #[error("claim failed: {reason}")]
    ClaimFailed { reason: String },

    /// No task with the given identifier was found.
    #[error("task not found: {id}")]
    TaskNotFound { id: String },

    /// The workflow run was stopped because accumulated cost exceeded the budget.
    #[error("budget exhausted after ${cost_usd:.4} USD")]
    BudgetExhausted { cost_usd: f64 },

    /// The workflow run was stopped because the maximum round count was reached.
    #[error("n-round limit of {max_rounds} exceeded")]
    NRoundExceeded { max_rounds: u32 },
}
