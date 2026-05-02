//! Workflow scheduler — Wave E Phase 1 (sera-kgi8) + Mail gate (sera-0zch).
//!
//! Wakes every [`TICK_INTERVAL`] and asks [`WorkflowTaskStore::list_pending`]
//! for the current set of pending tasks. Each pending task is passed through
//! [`sera_workflow::ready::ready_tasks_with_context`] with a fresh
//! `chrono::Utc::now` snapshot; tasks whose gates have resolved are marked
//! resolved on the store.
//!
//! Timer and Mail gates are fully wired end-to-end. Other await types
//! (Human: sera-dgk1, GhPr: sera-comg, GhRun: sera-4fel, Change: sera-7ggi)
//! use no-op lookups and stay pending until their dedicated beads add real
//! lookup sources.
//!
//! Event emission: Phase 1 logs a `tracing::info!` line per resolution. Wiring
//! to the session SSE stream / event bus is deferred to a follow-up bead — the
//! scheduler itself can stay agnostic to the notification backend.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::Utc;

use sera_mail::InMemoryMailLookup;
use sera_workflow::MailLookup;
use sera_workflow::ready::{ReadyContext, ready_tasks_with_context};

use crate::workflow_store::WorkflowTaskStore;

/// How often the scheduler runs `tick`. 5s matches the Wave E Phase 1 spec —
/// short enough to feel responsive for Timer gates at second-granularity,
/// long enough to keep CPU wake cost negligible when no tasks are pending.
pub const TICK_INTERVAL: Duration = Duration::from_secs(5);

/// Single scheduler pass: snapshot pending tasks, ask sera-workflow which
/// are ready, and mark ready tasks resolved on the store.
///
/// `mail_lookup` is consulted for every [`sera_workflow::task::AwaitType::Mail`]
/// gate. Pass the `Arc<InMemoryMailLookup>` wired to the correlator for
/// production; tests may construct a fresh lookup and call
/// [`InMemoryMailLookup::notify`] directly to drive the gate.
///
/// Returns the number of tasks that transitioned from Pending → Resolved.
/// Exposed as `pub` (not just `pub(crate)`) so integration tests can drive
/// a single tick deterministically without racing the real ticker.
pub async fn tick(
    store: Arc<dyn WorkflowTaskStore>,
    mail_lookup: Arc<InMemoryMailLookup>,
) -> usize {
    let pending = store.list_pending().await;
    if pending.is_empty() {
        return 0;
    }

    // ready_tasks_with_context operates on `&[WorkflowTask]`. Clone the tasks
    // out of the records — cheap relative to the lock hold.
    let tasks: Vec<sera_workflow::WorkflowTask> =
        pending.iter().map(|r| r.task.clone()).collect();

    let now = Utc::now();
    let ctx = ReadyContext {
        mail: mail_lookup.as_ref() as &dyn MailLookup,
        ..ReadyContext::default_noop()
    };
    let ready = ready_tasks_with_context(&tasks, now, &ctx);

    let mut resolved = 0;
    for task in ready {
        let id = task.id.to_string();
        if store.mark_resolved(&id, now).await {
            resolved += 1;
            tracing::info!(
                event = "workflow_task_resolved",
                task_id = %id,
                title = %task.title,
                "workflow task gate resolved"
            );
        }
    }
    resolved
}

/// Spawn the scheduler background task.
///
/// `mail_lookup` is passed to every [`tick`] call so Mail-gate tasks resolve
/// as soon as the correlator pushes a terminal [`sera_workflow::task::MailEvent`]
/// into the lookup.
///
/// Ticks every [`TICK_INTERVAL`]. The loop exits when `shutting_down` flips
/// to `true` — observed between ticks so an in-flight `tick` call is never
/// interrupted mid-way.
///
/// Returns immediately after spawning; the returned `JoinHandle` is
/// intentionally dropped by most callers — shutdown is coordinated via the
/// shared `shutting_down` atomic the gateway already uses for other
/// background loops.
pub fn spawn_scheduler(
    store: Arc<dyn WorkflowTaskStore>,
    mail_lookup: Arc<InMemoryMailLookup>,
    shutting_down: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TICK_INTERVAL);
        // Skip missed ticks on pause/resume instead of bursting through them.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Consume the immediate first tick so the first real work happens
        // after one TICK_INTERVAL, matching the spec ("every 5 seconds").
        ticker.tick().await;

        loop {
            if shutting_down.load(Ordering::SeqCst) {
                tracing::info!(
                    event = "workflow_scheduler_stopped",
                    "workflow scheduler exiting (shutdown signalled)"
                );
                break;
            }
            ticker.tick().await;
            if shutting_down.load(Ordering::SeqCst) {
                break;
            }
            let resolved = tick(Arc::clone(&store), Arc::clone(&mail_lookup)).await;
            if resolved > 0 {
                tracing::debug!(
                    event = "workflow_scheduler_tick",
                    resolved,
                    "workflow scheduler tick resolved tasks"
                );
            }
        }
    })
}
