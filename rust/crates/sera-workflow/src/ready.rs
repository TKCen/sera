use chrono::{DateTime, Utc};

use sera_hitl::{ApprovalId, TicketStatus};
use sera_types::evolution::ChangeArtifactId;

use crate::task::{
    AwaitType, ChangeState, DependencyType, GhPrId, GhPrState, GhRunId, GhRunStatus, MailEvent,
    MailThreadId, WorkflowTask, WorkflowTaskId, WorkflowTaskStatus,
};

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

/// Pull-based lookup into a GitHub Actions status source for
/// [`AwaitType::GhRun`].
///
/// Mirrors the shape of [`HitlLookup`]: the ready-queue is synchronous, so the
/// implementor must snapshot the current state of relevant runs into an
/// in-memory map (or equivalent) and answer synchronously. Any network I/O
/// lives one layer up.
///
/// Returning `None` means the run is not known to the lookup (never seen, or
/// evicted). The ready-queue treats unknown runs as not-ready — we never
/// self-satisfy on unknown state, matching the [`HitlLookup`] contract.
pub trait GhRunLookup: Send + Sync {
    /// Return the current status of the run identified by `id`, or `None`
    /// when the run is not known to the lookup source.
    fn run_status(&self, run_id: &GhRunId) -> Option<GhRunStatus>;
}

/// A trivial [`GhRunLookup`] that reports every run as unknown.
///
/// Useful as a default for callers that are not yet wired into a GitHub
/// polling source — preserves the pre-GhRun behaviour where GhRun-await
/// tasks always block.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopGhRunLookup;

impl GhRunLookup for NoopGhRunLookup {
    fn run_status(&self, _id: &GhRunId) -> Option<GhRunStatus> {
        None
    }
}

/// Pure-function gate for [`AwaitType::GhRun`].
///
/// Returns `true` iff `await_type` is `Some(AwaitType::GhRun { run_id, .. })`
/// and the referenced run resolves to a terminal [`GhRunStatus`]
/// (Completed / Failed / Cancelled / Skipped / Neutral) via `lookup`. A run
/// reported as `None` (unknown) or in a non-terminal state (Queued /
/// InProgress / Unknown) is treated as not ready.
///
/// Workflows deliberately proceed on Failed / Cancelled / Neutral too — the
/// task itself needs to wake up so its handler can branch on the outcome.
/// Gating on Completed-only would strand failed runs indefinitely.
///
/// Returns `false` for non-GhRun await types.
pub fn is_gh_run_ready(await_type: &AwaitType, lookup: &dyn GhRunLookup) -> bool {
    match await_type {
        AwaitType::GhRun { run_id, .. } => lookup
            .run_status(run_id)
            .map(|s| s.is_terminal())
            .unwrap_or(false),
        _ => false,
    }
}

/// Pull-based lookup into a GitHub pull-request state source for
/// [`AwaitType::GhPr`].
///
/// Mirrors the shape of [`GhRunLookup`]: the ready-queue is synchronous, so
/// the implementor must snapshot the current state of relevant PRs into an
/// in-memory map (or equivalent) and answer synchronously. Any network I/O
/// lives one layer up.
///
/// Returning `None` means the PR is not known to the lookup (never seen, or
/// evicted). The ready-queue treats unknown PRs as not-ready — we never
/// self-satisfy on unknown state, matching the [`HitlLookup`] and
/// [`GhRunLookup`] contracts.
pub trait GhPrLookup: Send + Sync {
    /// Return the current state of the PR identified by `pr_id`, or `None`
    /// when the PR is not known to the lookup source.
    fn pr_state(&self, pr_id: &GhPrId) -> Option<GhPrState>;
}

/// A trivial [`GhPrLookup`] that reports every PR as unknown.
///
/// Useful as a default for callers that are not yet wired into a GitHub
/// polling source — preserves the pre-GhPr behaviour where GhPr-await
/// tasks always block.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopGhPrLookup;

impl GhPrLookup for NoopGhPrLookup {
    fn pr_state(&self, _pr_id: &GhPrId) -> Option<GhPrState> {
        None
    }
}

/// Pure-function gate for [`AwaitType::GhPr`].
///
/// Returns `true` iff `await_type` is `Some(AwaitType::GhPr { pr_id, .. })`
/// and the referenced PR resolves to a terminal [`GhPrState`]
/// (Closed / Merged) via `lookup`. A PR reported as `None` (unknown) or in a
/// non-terminal state (Open / Draft / Unknown) is treated as not ready.
///
/// Workflows deliberately proceed on Closed too — the task itself needs to
/// wake up so its handler can branch on the outcome (merged vs closed-without-
/// merge). Gating on Merged-only would strand closed-without-merge PRs
/// indefinitely.
///
/// Returns `false` for non-GhPr await types.
pub fn is_gh_pr_ready(await_type: &AwaitType, lookup: &dyn GhPrLookup) -> bool {
    match await_type {
        AwaitType::GhPr { pr_id, .. } => lookup
            .pr_state(pr_id)
            .map(|s| s.is_terminal())
            .unwrap_or(false),
        _ => false,
    }
}

/// Pull-based lookup into a SERA change-artifact state source for
/// [`AwaitType::Change`].
///
/// Mirrors the shape of [`GhPrLookup`]: the ready-queue is synchronous, so the
/// implementor must snapshot the current state of relevant change artifacts into
/// an in-memory map (or equivalent) and answer synchronously. Any network I/O
/// lives one layer up.
///
/// Returning `None` means the artifact is not known to the lookup (never seen,
/// or evicted). The ready-queue treats unknown artifacts as not-ready — we
/// never self-satisfy on unknown state, matching the other lookup contracts.
pub trait ChangeLookup: Send + Sync {
    /// Return the current state of the change artifact identified by `id`, or
    /// `None` when the artifact is not known to the lookup source.
    fn change_state(&self, id: &ChangeArtifactId) -> Option<ChangeState>;
}

/// A trivial [`ChangeLookup`] that reports every change artifact as unknown.
///
/// Useful as a default for callers that are not yet wired into a change-artifact
/// polling source — preserves the pre-Change behaviour where Change-await tasks
/// always block.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopChangeLookup;

impl ChangeLookup for NoopChangeLookup {
    fn change_state(&self, _id: &ChangeArtifactId) -> Option<ChangeState> {
        None
    }
}

/// Pure-function gate for [`AwaitType::Change`].
///
/// Returns `true` iff `await_type` is `Some(AwaitType::Change { artifact_id })`
/// and the referenced artifact resolves to a terminal [`ChangeState`]
/// (Applied / Rejected / Failed / Superseded) via `lookup`. An artifact
/// reported as `None` (unknown) or in a non-terminal state (Proposed /
/// UnderReview / Approved / Unknown) is treated as not ready.
///
/// Workflows deliberately proceed on Rejected / Failed / Superseded too — the
/// task itself needs to wake up so its handler can branch on the outcome.
/// Gating on Applied-only would strand artifacts that fail or get superseded.
///
/// Returns `false` for non-Change await types.
pub fn is_change_ready(await_type: &AwaitType, lookup: &dyn ChangeLookup) -> bool {
    match await_type {
        AwaitType::Change { artifact_id } => lookup
            .change_state(artifact_id)
            .map(|s| s.is_terminal())
            .unwrap_or(false),
        _ => false,
    }
}

/// Pull-based lookup into a mail backend for [`AwaitType::Mail`].
///
/// Mirrors the shape of [`ChangeLookup`]: the ready-queue is synchronous, so
/// the implementor must snapshot the current event state of relevant threads
/// into an in-memory map (or equivalent) and answer synchronously. Any network
/// I/O lives one layer up.
///
/// Returning `None` means the thread is not known to the lookup (never seen, or
/// evicted). The ready-queue treats unknown threads as not-ready — we never
/// self-satisfy on unknown state, matching the other lookup contracts.
pub trait MailLookup: Send + Sync {
    /// Return the current event state of the thread identified by `id`, or
    /// `None` when the thread is not known to the lookup source.
    fn thread_event(&self, id: &MailThreadId) -> Option<MailEvent>;
}

/// A trivial [`MailLookup`] that reports every thread as unknown.
///
/// Useful as a default for callers that are not yet wired into a mail backend —
/// preserves the pre-Mail behaviour where Mail-await tasks always block.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopMailLookup;

impl MailLookup for NoopMailLookup {
    fn thread_event(&self, _id: &MailThreadId) -> Option<MailEvent> {
        None
    }
}

/// Pure-function gate for [`AwaitType::Mail`].
///
/// Returns `true` iff `await_type` is `Some(AwaitType::Mail { thread_id })`
/// and the referenced thread resolves to a terminal [`MailEvent`]
/// (ReplyReceived / Closed) via `lookup`. A thread reported as `None`
/// (unknown) or in a non-terminal state (Pending / Unknown) is treated as not
/// ready.
///
/// Workflows deliberately proceed on Closed too — the task itself needs to
/// wake up so its handler can branch on the outcome (reply received vs closed).
/// Gating on ReplyReceived-only would strand administratively-closed threads
/// indefinitely.
///
/// Returns `false` for non-Mail await types.
pub fn is_mail_ready(await_type: &AwaitType, lookup: &dyn MailLookup) -> bool {
    match await_type {
        AwaitType::Mail { thread_id } => lookup
            .thread_event(thread_id)
            .map(|e| e.is_terminal())
            .unwrap_or(false),
        _ => false,
    }
}

/// Bundle of lookup dependencies consulted by [`ready_tasks_with_context`].
///
/// Exists to keep gate signatures small as new per-await-variant lookups are
/// added. Instead of threading a growing list of `&dyn XLookup` positional args
/// through `ready_tasks_with_…` and `is_ready`, callers build one
/// [`ReadyContext`] and pass it by reference.
///
/// Use [`ReadyContext::default_noop`] for callers that are not yet wired into
/// any real lookup source — every gate reports unknown, matching the
/// pre-integration behaviour (all non-Timer awaits block).
pub struct ReadyContext<'a> {
    /// Lookup consulted for [`AwaitType::Human`] gates.
    pub hitl: &'a dyn HitlLookup,
    /// Lookup consulted for [`AwaitType::GhRun`] gates.
    pub gh_run: &'a dyn GhRunLookup,
    /// Lookup consulted for [`AwaitType::GhPr`] gates.
    pub gh_pr: &'a dyn GhPrLookup,
    /// Lookup consulted for [`AwaitType::Change`] gates.
    pub change: &'a dyn ChangeLookup,
    /// Lookup consulted for [`AwaitType::Mail`] gates.
    pub mail: &'a dyn MailLookup,
}

impl<'a> ReadyContext<'a> {
    /// Build a [`ReadyContext`] backed entirely by no-op lookups.
    ///
    /// Every gate that requires an external signal (Human, GhRun, GhPr, …)
    /// reports "unknown" and therefore resolves to not-ready. Useful as the
    /// default for callers that have not yet wired into real lookup sources,
    /// and for unit tests exercising purely time/dependency-based gates.
    pub fn default_noop() -> ReadyContext<'static> {
        ReadyContext {
            hitl: &NoopHitlLookup,
            gh_run: &NoopGhRunLookup,
            gh_pr: &NoopGhPrLookup,
            change: &NoopChangeLookup,
            mail: &NoopMailLookup,
        }
    }
}

/// Return all tasks that are ready to be claimed right now.
///
/// Equivalent to [`ready_tasks_with_context`] with a
/// [`ReadyContext::default_noop`] — every external-signal gate
/// ([`AwaitType::Human`], [`AwaitType::GhRun`], …) is treated as still
/// pending. Callers that want to surface externally-gated tasks must use
/// [`ready_tasks_with_context`].
///
/// Five gates must all pass — see [`ready_tasks_with_context`] for the full
/// list.
///
/// Results are sorted by `(priority ASC, id bytes)` for determinism.
pub fn ready_tasks(tasks: &[WorkflowTask], now: DateTime<Utc>) -> Vec<&WorkflowTask> {
    ready_tasks_with_context(tasks, now, &ReadyContext::default_noop())
}

/// Deprecated shim kept for source compatibility with sera-gj93 callers.
///
/// Constructs a [`ReadyContext`] from the supplied [`HitlLookup`] and
/// [`NoopGhRunLookup`], then delegates to [`ready_tasks_with_context`]. New
/// callers should build a [`ReadyContext`] directly so they can also opt into
/// non-Hitl lookups ([`GhRunLookup`], and future GhPr / Mail / Change).
#[deprecated(
    since = "0.1.0",
    note = "build a ReadyContext and call ready_tasks_with_context instead; \
            this shim will be removed once all external callers migrate"
)]
pub fn ready_tasks_with_hitl<'a>(
    tasks: &'a [WorkflowTask],
    now: DateTime<Utc>,
    hitl: &dyn HitlLookup,
) -> Vec<&'a WorkflowTask> {
    let ctx = ReadyContext {
        hitl,
        gh_run: &NoopGhRunLookup,
        gh_pr: &NoopGhPrLookup,
        change: &NoopChangeLookup,
        mail: &NoopMailLookup,
    };
    ready_tasks_with_context(tasks, now, &ctx)
}

/// Return all tasks that are ready to be claimed right now, consulting `ctx`
/// for any external-signal gates ([`AwaitType::Human`], [`AwaitType::GhRun`],
/// …).
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
///      - `Human { approval_id }` and `ctx.hitl.ticket_status(approval_id)`
///        reports a terminal status;
///      - `GhRun { run_id, .. }` and `ctx.gh_run.run_status(run_id)` reports a
///        terminal status;
///      - `GhPr { pr_id, .. }` and `ctx.gh_pr.pr_state(pr_id)` reports a
///        terminal status;
///      - `Change { artifact_id }` and `ctx.change.change_state(artifact_id)`
///        reports a terminal status;
///      - `Mail { thread_id }` and `ctx.mail.thread_event(thread_id)` reports a
///        terminal event (ReplyReceived / Closed).
/// 5. Not `(ephemeral && status == Closed)` — ephemeral tasks are never
///    surfaced once closed (redundant given gate 1, but kept for clarity).
///
/// Results are sorted by `(priority ASC, id bytes)` for determinism.
pub fn ready_tasks_with_context<'a>(
    tasks: &'a [WorkflowTask],
    now: DateTime<Utc>,
    ctx: &ReadyContext<'_>,
) -> Vec<&'a WorkflowTask> {
    let mut ready: Vec<&WorkflowTask> = tasks
        .iter()
        .filter(|t| is_ready(t, tasks, now, ctx))
        .collect();

    ready.sort_by_key(|t| (t.priority, t.id.hash));
    ready
}

fn is_ready(
    task: &WorkflowTask,
    all: &[WorkflowTask],
    now: DateTime<Utc>,
    ctx: &ReadyContext<'_>,
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
                if !is_human_ready(await_type, ctx.hitl) {
                    return false;
                }
                // Ticket reached a terminal state — gate passes.
            }
            AwaitType::GhRun { .. } => {
                if !is_gh_run_ready(await_type, ctx.gh_run) {
                    return false;
                }
                // GitHub run reached a terminal state — gate passes.
            }
            AwaitType::GhPr { .. } => {
                if !is_gh_pr_ready(await_type, ctx.gh_pr) {
                    return false;
                }
                // GitHub PR reached a terminal state — gate passes.
            }
            AwaitType::Change { .. } => {
                if !is_change_ready(await_type, ctx.change) {
                    return false;
                }
                // Change artifact reached a terminal state — gate passes.
            }
            AwaitType::Mail { .. } => {
                if !is_mail_ready(await_type, ctx.mail) {
                    return false;
                }
                // Thread received a reply or was closed — gate passes.
            }
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
