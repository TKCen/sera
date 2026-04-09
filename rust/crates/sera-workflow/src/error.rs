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
}
