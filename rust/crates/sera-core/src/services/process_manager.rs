//! Process manager service — sequential, parallel, and hierarchical workflow execution.

use std::collections::HashMap;
use uuid::Uuid;
use time::OffsetDateTime;
use serde::{Deserialize, Serialize};

use sera_db::DbPool;

/// Error type for process management operations.
#[derive(Debug, thiserror::Error)]
pub enum ProcessManagerError {
    #[error("database error: {0}")]
    Db(#[from] sera_db::DbError),
    #[error("step execution failed: {0}")]
    StepFailed(String),
    #[error("step execution timed out: {0}")]
    Timeout(String),
    #[error("invalid workflow: {0}")]
    InvalidWorkflow(String),
    #[error("workflow not found: {0}")]
    NotFound(String),
}

/// Represents the execution mode for workflow steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    /// Sequential: step N waits for step N-1 to complete
    Sequential,
    /// Parallel: all steps run concurrently
    Parallel,
}

/// Represents a single step in a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    /// Unique name for this step
    pub name: String,
    /// Action type (e.g., "invoke_agent", "call_webhook", "process_data")
    pub action: String,
    /// Optional IDs of steps that must complete before this one
    pub dependencies: Option<Vec<String>>,
    /// Timeout in milliseconds; None means no timeout
    pub timeout_ms: Option<u64>,
    /// Additional action-specific configuration
    pub config: serde_json::Value,
}

/// Represents a complete workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    /// Workflow name
    pub name: String,
    /// Execution mode: sequential or parallel
    pub mode: ExecutionMode,
    /// List of steps in the workflow
    pub steps: Vec<WorkflowStep>,
    /// Optional description
    pub description: Option<String>,
}

/// Status of a workflow execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionStatus {
    /// Workflow is pending execution
    Pending,
    /// Workflow is currently running
    Running,
    /// Workflow completed successfully
    Completed,
    /// Workflow failed
    Failed,
}

/// Status of a single step in a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepStatus {
    /// Step name
    pub name: String,
    /// Current status
    pub status: ExecutionStatus,
    /// When the step started
    pub started_at: Option<OffsetDateTime>,
    /// When the step completed
    pub completed_at: Option<OffsetDateTime>,
    /// Result or error from step execution
    pub result: Option<serde_json::Value>,
    /// Error message if step failed
    pub error: Option<String>,
}

/// Represents a workflow execution with progress tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowExecution {
    /// Unique execution ID
    pub id: Uuid,
    /// Agent ID that owns this execution
    pub agent_id: Uuid,
    /// Workflow definition
    pub workflow: WorkflowDefinition,
    /// Overall execution status
    pub status: ExecutionStatus,
    /// Per-step status tracking
    pub steps: HashMap<String, StepStatus>,
    /// When execution started
    pub started_at: Option<OffsetDateTime>,
    /// When execution completed
    pub completed_at: Option<OffsetDateTime>,
    /// Error message if execution failed
    pub error: Option<String>,
}

/// Process manager service for orchestrating multi-step workflows.
pub struct ProcessManager {
    pool: DbPool,
}

impl ProcessManager {
    /// Create a new process manager.
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    /// Validate a workflow definition.
    fn validate_workflow(definition: &WorkflowDefinition) -> Result<(), ProcessManagerError> {
        if definition.steps.is_empty() {
            return Err(ProcessManagerError::InvalidWorkflow(
                "workflow must contain at least one step".to_string(),
            ));
        }

        // Validate that all dependencies reference existing steps
        let step_names: std::collections::HashSet<_> =
            definition.steps.iter().map(|s| &s.name).collect();

        for step in &definition.steps {
            if let Some(deps) = &step.dependencies {
                for dep in deps {
                    if !step_names.contains(dep) {
                        return Err(ProcessManagerError::InvalidWorkflow(format!(
                            "step '{}' depends on non-existent step '{}'",
                            step.name, dep
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    /// Execute a workflow.
    ///
    /// # Arguments
    /// * `agent_id` — UUID of the agent executing the workflow
    /// * `definition` — workflow definition
    ///
    /// # Returns
    /// A WorkflowExecution tracking the execution progress.
    pub async fn execute_workflow(
        &self,
        agent_id: Uuid,
        definition: WorkflowDefinition,
    ) -> Result<WorkflowExecution, ProcessManagerError> {
        Self::validate_workflow(&definition)?;

        let execution_id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();

        // Initialize step statuses
        let mut steps = HashMap::new();
        for step in &definition.steps {
            steps.insert(
                step.name.clone(),
                StepStatus {
                    name: step.name.clone(),
                    status: ExecutionStatus::Pending,
                    started_at: None,
                    completed_at: None,
                    result: None,
                    error: None,
                },
            );
        }

        let execution = WorkflowExecution {
            id: execution_id,
            agent_id,
            workflow: definition,
            status: ExecutionStatus::Running,
            steps,
            started_at: Some(now),
            completed_at: None,
            error: None,
        };

        // Phase 5: Implement workflow execution orchestration. Required:
        // - Sequential mode: execute steps in order, respecting dependencies
        // - Parallel mode: execute independent steps concurrently (tokio::spawn)
        // - Step-level timeout handling (tokio::time::timeout)
        // - Persist execution state to database after each step
        // - Handle failures: mark step status, propagate errors

        Ok(execution)
    }

    /// Get the current status of a workflow execution.
    ///
    /// # Arguments
    /// * `execution_id` — ID of the execution to check
    ///
    /// # Returns
    /// The current execution status with step-level progress.
    pub async fn get_execution_status(
        &self,
        _execution_id: Uuid,
    ) -> Result<WorkflowExecution, ProcessManagerError> {
        // TODO: Implement status retrieval from persistent storage
        Err(ProcessManagerError::NotFound(
            "workflow execution not found".to_string(),
        ))
    }

    /// Cancel a running workflow execution.
    pub async fn cancel_execution(&self, _execution_id: Uuid) -> Result<(), ProcessManagerError> {
        // TODO: Implement execution cancellation
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_workflow_valid_sequential() {
        let definition = WorkflowDefinition {
            name: "test".to_string(),
            mode: ExecutionMode::Sequential,
            description: None,
            steps: vec![
                WorkflowStep {
                    name: "step1".to_string(),
                    action: "invoke".to_string(),
                    dependencies: None,
                    timeout_ms: Some(5000),
                    config: serde_json::json!({}),
                },
                WorkflowStep {
                    name: "step2".to_string(),
                    action: "invoke".to_string(),
                    dependencies: Some(vec!["step1".to_string()]),
                    timeout_ms: None,
                    config: serde_json::json!({}),
                },
            ],
        };

        assert!(ProcessManager::validate_workflow(&definition).is_ok());
    }

    #[test]
    fn test_validate_workflow_invalid_empty() {
        let definition = WorkflowDefinition {
            name: "test".to_string(),
            mode: ExecutionMode::Parallel,
            description: None,
            steps: vec![],
        };

        assert!(ProcessManager::validate_workflow(&definition).is_err());
    }

    #[test]
    fn test_validate_workflow_invalid_dependency() {
        let definition = WorkflowDefinition {
            name: "test".to_string(),
            mode: ExecutionMode::Sequential,
            description: None,
            steps: vec![WorkflowStep {
                name: "step1".to_string(),
                action: "invoke".to_string(),
                dependencies: Some(vec!["nonexistent".to_string()]),
                timeout_ms: None,
                config: serde_json::json!({}),
            }],
        };

        assert!(ProcessManager::validate_workflow(&definition).is_err());
    }

    #[test]
    fn test_step_status_initialization() {
        let step = StepStatus {
            name: "test_step".to_string(),
            status: ExecutionStatus::Pending,
            started_at: None,
            completed_at: None,
            result: None,
            error: None,
        };

        assert_eq!(step.status, ExecutionStatus::Pending);
        assert!(step.started_at.is_none());
    }
}
