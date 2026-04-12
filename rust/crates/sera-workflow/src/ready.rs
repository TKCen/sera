use chrono::{DateTime, Utc};

use crate::task::{DependencyType, WorkflowTask, WorkflowTaskId, WorkflowTaskStatus};

/// Return all tasks that are ready to be claimed right now.
///
/// Five gates must all pass:
/// 1. `status == Open`
/// 2. No `Blocks` / `ConditionalBlocks` dependency where the blocker task has
///    status in `{Open, InProgress, Hooked, Blocked}`.
///    Exception: a `ConditionalBlocks` edge is satisfied (does NOT block) when
///    the blocker is `Closed`.
/// 3. `defer_until <= now` or `None`.
/// 4. `await_type.is_none()`.
/// 5. Not `(ephemeral && status == Closed)` — ephemeral tasks are never surfaced
///    once closed (redundant given gate 1, but kept for clarity).
///
/// Results are sorted by `(priority ASC, id bytes)` for determinism.
pub fn ready_tasks(tasks: &[WorkflowTask], now: DateTime<Utc>) -> Vec<&WorkflowTask> {
    let mut ready: Vec<&WorkflowTask> = tasks
        .iter()
        .filter(|t| is_ready(t, tasks, now))
        .collect();

    ready.sort_by_key(|t| (t.priority, t.id.hash));
    ready
}

fn is_ready(task: &WorkflowTask, all: &[WorkflowTask], now: DateTime<Utc>) -> bool {
    // Gate 1 — must be Open.
    if task.status != WorkflowTaskStatus::Open {
        return false;
    }

    // Gate 2 — no unsatisfied blocking dependencies.
    for dep in &task.dependencies {
        // We care about edges where this task is the one being blocked.
        // Convention: dep.to == task.id means "dep.from blocks dep.to".
        if dep.to != task.id {
            continue;
        }
        let is_blocking = matches!(
            dep.kind,
            DependencyType::Blocks | DependencyType::ConditionalBlocks
        );
        if !is_blocking {
            continue;
        }

        // Find the blocker task.
        if let Some(blocker) = all.iter().find(|t| t.id == dep.from) {
            let blocker_active = matches!(
                blocker.status,
                WorkflowTaskStatus::Open
                    | WorkflowTaskStatus::InProgress
                    | WorkflowTaskStatus::Hooked
                    | WorkflowTaskStatus::Blocked
            );

            if blocker_active {
                // ConditionalBlocks is only satisfied when the blocker is Closed.
                // Since it is NOT Closed here, the edge still blocks.
                return false;
            }
            // blocker is Closed / Deferred / Pinned — edge is satisfied.
        }
        // If blocker task not found in slice, treat as satisfied (defensive).
    }

    // Gate 3 — not deferred.
    if let Some(defer) = task.defer_until
        && defer > now
    {
        return false;
    }

    // Gate 4 — not awaiting an external signal.
    if task.await_type.is_some() {
        return false;
    }

    // Gate 5 — ephemeral + Closed never surfaces (redundant with gate 1).
    if task.ephemeral && task.status == WorkflowTaskStatus::Closed {
        return false;
    }

    true
}

/// Compute the transitive closure of task dependencies starting from `roots`.
///
/// Returns all [`WorkflowTaskId`]s reachable via any dependency edge
/// (regardless of direction or kind) from any root.  The roots themselves are
/// included in the result.
pub fn dependency_closure(
    tasks: &[WorkflowTask],
    roots: &[WorkflowTaskId],
) -> Vec<WorkflowTaskId> {
    let mut visited: std::collections::HashSet<WorkflowTaskId> =
        roots.iter().copied().collect();
    let mut queue: std::collections::VecDeque<WorkflowTaskId> =
        roots.iter().copied().collect();

    while let Some(current) = queue.pop_front() {
        if let Some(task) = tasks.iter().find(|t| t.id == current) {
            for dep in &task.dependencies {
                let neighbour = if dep.from == current { dep.to } else { dep.from };
                if visited.insert(neighbour) {
                    queue.push_back(neighbour);
                }
            }
        }
    }

    let mut result: Vec<WorkflowTaskId> = visited.into_iter().collect();
    result.sort();
    result
}
