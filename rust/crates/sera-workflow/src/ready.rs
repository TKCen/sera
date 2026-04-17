use chrono::{DateTime, Utc};

use sera_hitl::{ApprovalId, TicketStatus};

use crate::task::{AwaitType, DependencyType, WorkflowTask, WorkflowTaskId, WorkflowTaskStatus};

/// Pure-function gate for [`AwaitType::Timer`].
///
/// Returns `true` iff `await_type` is `Some(AwaitType::Timer { not_before })`
/// and `now >= not_before`. Boundary is inclusive (`>=`, not `>`).
///
/// Returns `false` for `None` — callers should use this only when the task
/// has an active await gate. Also returns `false` for non-Timer await types,
/// which remain blocking pending their respective integrations
/// (GhRun/GhPr/Mail/Change).
pub fn is_timer_ready(await_type: &AwaitType, now: DateTime<Utc>) -> bool {
    await_type.is_timer_ready(now)
}

/// Pull-based lookup into sera-hitl for [`AwaitType::Human`].
///
/// The ready-queue polls this trait to decide whether a human-approval gate
/// has resolved. Implementors look up the ticket by [`ApprovalId`] and return
/// its current [`TicketStatus`], or `None` if the ticket does not exist in
/// the backing store (e.g. it was never created, or was evicted).
///
/// The trait is synchronous on purpose: the ready-queue itself is a pure
/// scheduling function called under a lock — any async I/O lives one layer
/// up (the caller snapshots the hitl state into an in-memory [`HashMap`] or
/// equivalent and hands it to [`ready_tasks_with_hitl`]). A synchronous
/// signature keeps the gate logic testable without a Tokio runtime and
/// matches the shape of [`is_timer_ready`].
///
/// [`HashMap`]: std::collections::HashMap
pub trait HitlLookup: Send + Sync {
    /// Return the current status of the ticket identified by `id`, or
    /// `None` when the ticket is not known to the lookup source.
    fn ticket_status(&self, id: &ApprovalId) -> Option<TicketStatus>;
}

/// A trivial [`HitlLookup`] that reports every ticket as unknown.
///
/// Useful as a default for callers that are not yet wired into sera-hitl —
/// it preserves the pre-Human behaviour where human-await tasks always block.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopHitlLookup;

impl HitlLookup for NoopHitlLookup {
    fn ticket_status(&self, _id: &ApprovalId) -> Option<TicketStatus> {
        None
    }
}

/// Pure-function gate for [`AwaitType::Human`].
///
/// Returns `true` iff `await_type` is `Some(AwaitType::Human { approval_id })`
/// and the referenced ticket resolves to a terminal [`TicketStatus`]
/// (Approved / Rejected / Expired) via `lookup`. A ticket reported as `None`
/// (unknown) or in a non-terminal state (Pending / Escalated) is treated as
/// not ready.
///
/// Workflows deliberately proceed on Rejected and Expired too — the task
/// itself needs to wake up so its handler can branch on the outcome. Gating
/// on Approved-only would strand rejected tickets indefinitely.
///
/// Returns `false` for non-Human await types.
pub fn is_human_ready(await_type: &AwaitType, lookup: &dyn HitlLookup) -> bool {
    match await_type {
        AwaitType::Human { approval_id } => lookup
            .ticket_status(approval_id)
            .map(TicketStatus::is_terminal)
            .unwrap_or(false),
        _ => false,
    }
}

/// Return all tasks that are ready to be claimed right now.
///
/// Equivalent to [`ready_tasks_with_hitl`] with a [`NoopHitlLookup`] — every
/// [`AwaitType::Human`] gate is treated as still pending. Callers that want
/// to surface approval-gated tasks must use [`ready_tasks_with_hitl`].
///
/// Five gates must all pass — see [`ready_tasks_with_hitl`] for the full list.
///
/// Results are sorted by `(priority ASC, id bytes)` for determinism.
pub fn ready_tasks(tasks: &[WorkflowTask], now: DateTime<Utc>) -> Vec<&WorkflowTask> {
    ready_tasks_with_hitl(tasks, now, &NoopHitlLookup)
}

/// Return all tasks that are ready to be claimed right now, consulting
/// `hitl` for [`AwaitType::Human`] gates.
///
/// Five gates must all pass:
/// 1. `status == Open`
/// 2. No `Blocks` / `ConditionalBlocks` dependency where the blocker task has
///    status in `{Open, InProgress, Hooked, Blocked}`.
///    Exception: a `ConditionalBlocks` edge is satisfied (does NOT block) when
///    the blocker is `Closed`.
/// 3. `defer_until <= now` or `None`.
/// 4. `await_type.is_none()` — OR — one of:
///      - `Timer { not_before }` and `now >= not_before`;
///      - `Human { approval_id }` and `hitl.ticket_status(approval_id)`
///        reports a terminal status.
///
///    All other `AwaitType` variants (GhRun/GhPr/Mail/Change) still block —
///    their integrations are tracked in follow-up beads.
/// 5. Not `(ephemeral && status == Closed)` — ephemeral tasks are never surfaced
///    once closed (redundant given gate 1, but kept for clarity).
///
/// Results are sorted by `(priority ASC, id bytes)` for determinism.
pub fn ready_tasks_with_hitl<'a>(
    tasks: &'a [WorkflowTask],
    now: DateTime<Utc>,
    hitl: &dyn HitlLookup,
) -> Vec<&'a WorkflowTask> {
    let mut ready: Vec<&WorkflowTask> = tasks
        .iter()
        .filter(|t| is_ready(t, tasks, now, hitl))
        .collect();

    ready.sort_by_key(|t| (t.priority, t.id.hash));
    ready
}

fn is_ready(
    task: &WorkflowTask,
    all: &[WorkflowTask],
    now: DateTime<Utc>,
    hitl: &dyn HitlLookup,
) -> bool {
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

    // Gate 4 — not awaiting an external signal (Timer and Human gates may
    // self-satisfy).
    if let Some(await_type) = &task.await_type {
        match await_type {
            AwaitType::Timer { .. } => {
                if !is_timer_ready(await_type, now) {
                    return false;
                }
                // Timer elapsed — gate passes.
            }
            AwaitType::Human { .. } => {
                if !is_human_ready(await_type, hitl) {
                    return false;
                }
                // Ticket reached a terminal state — gate passes.
            }
            // Other await variants remain pending until their integrations land.
            _ => return false,
        }
    }

    // Gate 5 — ephemeral + Closed never surfaces (redundant with gate 1).
    if task.ephemeral && task.status == WorkflowTaskStatus::Closed {
        return false;
    }

    true
}

/// Compute the topological ordering of tasks based on `Blocks` dependencies.
///
/// Uses Kahn's algorithm with cycle detection. Returns tasks in dependency order:
/// all tasks appear AFTER any tasks they depend on via `Blocks` edges.
/// Only considers `Blocks` dependency edges (not `Related`, `ConditionalBlocks`,
/// `ParentChild`, or `DiscoveredFrom`).
///
/// Returns `Ok(sorted_ids)` on success, or `Err(CyclicDependency)` if the
/// graph contains a cycle.
pub fn topological_sort(tasks: &[WorkflowTask]) -> Result<Vec<WorkflowTaskId>, CyclicDependency> {
    // Build adjacency map: blocker -> blocked tasks.
    let mut out_degree: std::collections::HashMap<WorkflowTaskId, usize> =
        std::collections::HashMap::new();
    let mut adj: std::collections::HashMap<WorkflowTaskId, Vec<WorkflowTaskId>> =
        std::collections::HashMap::new();

    // Initialize all tasks with 0 out-degree.
    for task in tasks {
        out_degree.insert(task.id, 0);
    }

    // Build graph based on Blocks dependencies only.
    for task in tasks {
        for dep in &task.dependencies {
            // Only consider Blocks edges (dep.from -> dep.to means from blocks to).
            if dep.kind != DependencyType::Blocks {
                continue;
            }
            // Ensure both tasks exist.
            if !out_degree.contains_key(&dep.from) || !out_degree.contains_key(&dep.to) {
                continue;
            }
            // Add edge: dep.from blocks dep.to -> dep.to depends on dep.from.
            *out_degree.entry(dep.to).or_insert(0) += 1;
            adj.entry(dep.from).or_default().push(dep.to);
        }
    }

    // Kahn's algorithm: start with nodes that have in-degree 0.
    let mut queue: std::collections::VecDeque<WorkflowTaskId> = out_degree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(&id, _)| id)
        .collect();

    let mut result: Vec<WorkflowTaskId> = Vec::with_capacity(tasks.len());

    while let Some(current) = queue.pop_front() {
        result.push(current);
        if let Some(neighbors) = adj.get(&current) {
            for &nbr in neighbors {
                if let Some(deg) = out_degree.get_mut(&nbr) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(nbr);
                    }
                }
            }
        }
    }

    // If we didn't process all nodes, there's a cycle.
    if result.len() != tasks.len() {
        Err(CyclicDependency)
    } else {
        Ok(result)
    }
}

/// Error returned when a dependency graph contains a cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CyclicDependency;

impl std::fmt::Display for CyclicDependency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cyclic dependency detected in task graph")
    }
}

impl std::error::Error for CyclicDependency {}

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
