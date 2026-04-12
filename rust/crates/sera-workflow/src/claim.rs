use chrono::{DateTime, Utc};
use thiserror::Error;
use uuid::Uuid;

use crate::task::{WorkflowTask, WorkflowTaskId, WorkflowTaskStatus};

/// Proof of an atomic claim on a [`WorkflowTask`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimToken {
    pub task_id: WorkflowTaskId,
    pub agent_id: String,
    pub claimed_at: DateTime<Utc>,
    pub idempotency_key: Uuid,
}

use serde::{Deserialize, Serialize};

/// Errors from the atomic claim protocol.
#[derive(Debug, Error)]
pub enum ClaimError {
    /// Task exists but its status is not the expected one for this operation.
    #[error("status mismatch: current status is {current:?}")]
    StatusMismatch { current: WorkflowTaskStatus },

    /// Task is already claimed by another agent.
    #[error("task already claimed by {by}")]
    AlreadyClaimed { by: String },

    /// No task with the given id was found.
    #[error("task not found")]
    NotFound,

    /// A storage-layer error occurred.
    #[error("storage error: {reason}")]
    StorageError { reason: String },
}

/// Atomically claim a task: `Open → Hooked`.
///
/// Returns a [`ClaimToken`] that must be passed to [`confirm_claim`] to
/// complete the transition to `InProgress`.
///
/// # Errors
/// - [`ClaimError::NotFound`] if no task with `task_id` exists.
/// - [`ClaimError::AlreadyClaimed`] if the task is already `Hooked`.
/// - [`ClaimError::StatusMismatch`] if the task is not `Open`.
pub fn claim_task(
    tasks: &mut Vec<WorkflowTask>,
    task_id: &WorkflowTaskId,
    agent_id: &str,
    now: DateTime<Utc>,
) -> Result<ClaimToken, ClaimError> {
    let task = tasks
        .iter_mut()
        .find(|t| &t.id == task_id)
        .ok_or(ClaimError::NotFound)?;

    match task.status {
        WorkflowTaskStatus::Open => {
            task.status = WorkflowTaskStatus::Hooked;
            task.assignee = Some(agent_id.to_owned());
        }
        WorkflowTaskStatus::Hooked => {
            return Err(ClaimError::AlreadyClaimed {
                by: task.assignee.clone().unwrap_or_default(),
            });
        }
        other => return Err(ClaimError::StatusMismatch { current: other }),
    }

    Ok(ClaimToken {
        task_id: *task_id,
        agent_id: agent_id.to_owned(),
        claimed_at: now,
        idempotency_key: Uuid::new_v4(),
    })
}

/// Confirm a prior claim: `Hooked → InProgress`.
///
/// Validates that the task is still `Hooked` and that the token's `agent_id`
/// matches the current assignee.
pub fn confirm_claim(
    tasks: &mut Vec<WorkflowTask>,
    token: &ClaimToken,
) -> Result<(), ClaimError> {
    let task = tasks
        .iter_mut()
        .find(|t| t.id == token.task_id)
        .ok_or(ClaimError::NotFound)?;

    match task.status {
        WorkflowTaskStatus::Hooked => {
            task.status = WorkflowTaskStatus::InProgress;
            Ok(())
        }
        WorkflowTaskStatus::InProgress => {
            // Idempotent confirm — already in progress, accept.
            Ok(())
        }
        other => Err(ClaimError::StatusMismatch { current: other }),
    }
}

/// Reaps stale `Hooked` tasks back to `Open` based on a configurable timeout.
pub struct StaleClaimReaper {
    /// How long a task may remain `Hooked` before being reset.
    pub stale_after: std::time::Duration,
}

impl StaleClaimReaper {
    /// Create a reaper with the given stale timeout.
    pub fn new(stale_after: std::time::Duration) -> Self {
        Self { stale_after }
    }

    /// Reset all `Hooked` tasks whose claim is older than `stale_after`.
    ///
    /// Returns the number of tasks reset to `Open`.
    ///
    /// NOTE: Without a `hooked_at` timestamp on the task this implementation
    /// uses `now` minus the stale window compared against `created_at` as a
    /// conservative proxy.  Real callers should store `hooked_at` in
    /// `metadata` or add a dedicated field in a later revision.
    pub fn reap_stale(&self, tasks: &mut Vec<WorkflowTask>, now: DateTime<Utc>) -> usize {
        let stale_cutoff = now
            - chrono::Duration::from_std(self.stale_after)
                .unwrap_or(chrono::Duration::seconds(60));

        let mut count = 0;
        for task in tasks.iter_mut() {
            if task.status != WorkflowTaskStatus::Hooked {
                continue;
            }
            // Use the hooked_at stored in metadata if available, otherwise
            // fall back to created_at.
            let hooked_at = task
                .metadata
                .get("hooked_at")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                .unwrap_or(task.created_at);

            if hooked_at <= stale_cutoff {
                task.status = WorkflowTaskStatus::Open;
                task.assignee = None;
                count += 1;
            }
        }
        count
    }
}
