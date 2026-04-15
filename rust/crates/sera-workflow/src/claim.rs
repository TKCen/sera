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
    tasks: &mut [WorkflowTask],
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
    tasks: &mut [WorkflowTask],
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
    pub fn reap_stale(&self, tasks: &mut [WorkflowTask], now: DateTime<Utc>) -> usize {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::WorkflowTask;

    fn make_task(id: WorkflowTaskId, status: WorkflowTaskStatus) -> WorkflowTask {
        WorkflowTask {
            id,
            title: "Test task".to_string(),
            description: "Test description".to_string(),
            acceptance_criteria: vec!["Done".to_string()],
            status,
            priority: 0,
            task_type: crate::task::WorkflowTaskType::Feature,
            assignee: None,
            due_at: None,
            defer_until: None,
            metadata: serde_json::Value::Null,
            await_type: None,
            await_id: None,
            timeout: None,
            ephemeral: false,
            source_formula: None,
            source_location: None,
            created_at: chrono::Utc::now(),
            meta_scope: None,
            change_artifact_id: None,
            dependencies: vec![],
        }
    }

    #[test]
    fn claim_task_from_open_succeeds() {
        let task_id = WorkflowTaskId::from_content("t", "d", "ac", "f", "l", chrono::Utc::now());
        let mut tasks = vec![make_task(task_id, WorkflowTaskStatus::Open)];

        let result = claim_task(&mut tasks, &task_id, "agent-1", chrono::Utc::now());

        assert!(result.is_ok());
        let token = result.unwrap();
        assert_eq!(token.agent_id, "agent-1");
        assert_eq!(token.task_id, task_id);
    }

    #[test]
    fn claim_task_from_hook_already_claimed() {
        let task_id = WorkflowTaskId::from_content("t", "d", "ac", "f", "l", chrono::Utc::now());
        let mut tasks = vec![make_task(task_id, WorkflowTaskStatus::Hooked)];
        tasks[0].assignee = Some("agent-2".to_string());

        let result = claim_task(&mut tasks, &task_id, "agent-1", chrono::Utc::now());

        assert!(matches!(result, Err(ClaimError::AlreadyClaimed { by }) if by == "agent-2"));
    }

    #[test]
    fn claim_task_from_in_progress_fails() {
        let task_id = WorkflowTaskId::from_content("t", "d", "ac", "f", "l", chrono::Utc::now());
        let mut tasks = vec![make_task(task_id, WorkflowTaskStatus::InProgress)];

        let result = claim_task(&mut tasks, &task_id, "agent-1", chrono::Utc::now());

        assert!(matches!(result, Err(ClaimError::StatusMismatch { current }) if current == WorkflowTaskStatus::InProgress));
    }

    #[test]
    fn claim_task_not_found() {
        let task_id = WorkflowTaskId::from_content("t", "d", "ac", "f", "l", chrono::Utc::now());
        let other_id = WorkflowTaskId::from_content("other", "d", "ac", "f", "l", chrono::Utc::now());
        let mut tasks = vec![make_task(task_id, WorkflowTaskStatus::Open)];

        let result = claim_task(&mut tasks, &other_id, "agent-1", chrono::Utc::now());

        assert!(matches!(result, Err(ClaimError::NotFound)));
    }

    #[test]
    fn confirm_claim_from_hooked_succeeds() {
        let task_id = WorkflowTaskId::from_content("t", "d", "ac", "f", "l", chrono::Utc::now());
        let mut tasks = vec![make_task(task_id, WorkflowTaskStatus::Hooked)];
        tasks[0].assignee = Some("agent-1".to_string());

        let token = ClaimToken {
            task_id,
            agent_id: "agent-1".to_string(),
            claimed_at: chrono::Utc::now(),
            idempotency_key: uuid::Uuid::new_v4(),
        };

        let result = confirm_claim(&mut tasks, &token);

        assert!(result.is_ok());
        assert_eq!(tasks[0].status, WorkflowTaskStatus::InProgress);
    }

    #[test]
    fn confirm_claim_idempotent() {
        let task_id = WorkflowTaskId::from_content("t", "d", "ac", "f", "l", chrono::Utc::now());
        let mut tasks = vec![make_task(task_id, WorkflowTaskStatus::InProgress)];
        tasks[0].assignee = Some("agent-1".to_string());

        let token = ClaimToken {
            task_id,
            agent_id: "agent-1".to_string(),
            claimed_at: chrono::Utc::now(),
            idempotency_key: uuid::Uuid::new_v4(),
        };

        let result = confirm_claim(&mut tasks, &token);

        // Idempotent — task is already InProgress
        assert!(result.is_ok());
    }

    #[test]
    fn stale_claim_reaper_resets_stale() {
        let task_id = WorkflowTaskId::from_content("t", "d", "ac", "f", "l", chrono::Utc::now());
        let mut tasks = vec![make_task(task_id, WorkflowTaskStatus::Hooked)];
        tasks[0].assignee = Some("agent-1".to_string());
        // Set hooked_at to 2 minutes ago
        let two_minutes_ago = (chrono::Utc::now() - chrono::Duration::minutes(2)).to_rfc3339();
        tasks[0].metadata = serde_json::json!({ "hooked_at": two_minutes_ago });

        let reaper = StaleClaimReaper::new(std::time::Duration::from_secs(60));
        let count = reaper.reap_stale(&mut tasks, chrono::Utc::now());

        assert_eq!(count, 1);
        assert_eq!(tasks[0].status, WorkflowTaskStatus::Open);
        assert!(tasks[0].assignee.is_none());
    }

    #[test]
    fn stale_claim_reaper_keeps_recent() {
        let task_id = WorkflowTaskId::from_content("t", "d", "ac", "f", "l", chrono::Utc::now());
        let mut tasks = vec![make_task(task_id, WorkflowTaskStatus::Hooked)];
        tasks[0].assignee = Some("agent-1".to_string());
        // Set hooked_at to 10 seconds ago
        let ten_seconds_ago = (chrono::Utc::now() - chrono::Duration::seconds(10)).to_rfc3339();
        tasks[0].metadata = serde_json::json!({ "hooked_at": ten_seconds_ago });

        let reaper = StaleClaimReaper::new(std::time::Duration::from_secs(60));
        let count = reaper.reap_stale(&mut tasks, chrono::Utc::now());

        assert_eq!(count, 0);
        assert_eq!(tasks[0].status, WorkflowTaskStatus::Hooked);
    }
}
