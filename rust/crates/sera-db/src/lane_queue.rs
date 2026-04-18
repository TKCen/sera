//! Lane-aware FIFO queue — in-memory, single-writer-per-session with global concurrency throttle.
//!
//! Per SPEC-gateway §5, each session gets its own "lane" that enforces ordering
//! and mode-specific delivery semantics (collect, followup, steer, etc.).
//! This is the Tier-1 (local/embedded) implementation — no database required.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sera_types::event::Event;

use crate::error::DbError;
use crate::lane_queue_counter::LaneCounterStoreDyn;

/// How queued messages are handled while a run is active for this session.
///
/// SPEC-gateway §5.2
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueMode {
    /// Coalesce queued messages into one follow-up turn after current run completes.
    Collect,
    /// Wait until current run ends, process queued messages as sequential follow-up turns.
    Followup,
    /// Inject incoming message at next tool boundary in current run.
    Steer,
    /// Steer now AND preserve for follow-up after current run.
    SteerBacklog,
    /// Abort active run, start new run with newest message.
    Interrupt,
}

/// Result of an enqueue operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnqueueResult {
    /// Event is ready to be processed (lane was idle).
    Ready,
    /// Event queued behind active run.
    Queued,
    /// Event marked for steer injection at next tool boundary.
    Steer,
    /// Active run should be interrupted with this event.
    Interrupt,
    /// Queue is closed (graceful shutdown in progress); the event was rejected.
    Closed,
}

/// An event wrapped with queue-level metadata.
#[derive(Debug, Clone)]
pub struct QueuedEvent {
    pub event: Event,
    pub enqueued_at: std::time::Instant,
    /// True if this event should be injected at a tool boundary (steer/steer_backlog modes).
    pub is_steer: bool,
}

impl QueuedEvent {
    fn new(event: Event) -> Self {
        Self {
            event,
            enqueued_at: std::time::Instant::now(),
            is_steer: false,
        }
    }

    fn new_steer(event: Event) -> Self {
        Self {
            event,
            enqueued_at: std::time::Instant::now(),
            is_steer: true,
        }
    }
}

/// One session's queue — serialises all activity for a single session_key.
struct Lane {
    queue: VecDeque<QueuedEvent>,
    /// Pending steer event (at most one outstanding at a time; newest wins).
    steer: Option<QueuedEvent>,
    mode: QueueMode,
    /// True when a run is active for this session.
    is_processing: bool,
}

impl Lane {
    fn new(_session_key: impl Into<String>, mode: QueueMode) -> Self {
        Self {
            queue: VecDeque::new(),
            steer: None,
            mode,
            is_processing: false,
        }
    }
}

/// Outcome of a [`LaneQueue::drain`] call.
///
/// * `drained` — the number of queued/in-flight jobs present when drain started
///   that had been released by the time drain returned.
/// * `remaining` — the number of queued/in-flight jobs still outstanding at
///   return time. `0` on a clean drain; positive when `timed_out` is `true`.
/// * `timed_out` — `true` if the drain deadline elapsed before the queue
///   reached zero pending jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrainOutcome {
    pub drained: usize,
    pub remaining: usize,
    pub timed_out: bool,
}

/// The main lane-aware queue manager.
///
/// Keyed by `session_key`; enforces a global concurrency cap across all lanes.
pub struct LaneQueue {
    lanes: HashMap<String, Lane>,
    max_concurrent_runs: usize,
    active_run_count: usize,
    default_mode: QueueMode,
    /// When `true`, [`LaneQueue::enqueue`] refuses new jobs. Flipped by
    /// [`LaneQueue::close`] during graceful shutdown.
    closed: bool,
    /// Counts [`LaneQueue::complete_run`] calls that arrived after the queue
    /// was already closed **and** the lane was no longer processing (i.e. the
    /// run had already been counted as done). Non-zero values indicate that a
    /// `complete_run` arrived after [`LaneQueue::drain_shared`] had already
    /// observed the queue as empty — harmless but useful for diagnosing timing
    /// of the drop-time race.
    post_close_stale_complete_runs: usize,
    /// Optional shared counter backend. When `Some`, every mutation of the
    /// in-process pending count also fans out to this store so multiple
    /// gateway pods can share a consistent view of per-lane admission-control
    /// state. `None` preserves the legacy single-pod behaviour (the default).
    ///
    /// The store is updated via `tokio::spawn` from the synchronous mutation
    /// paths (`enqueue`, `dequeue`, `complete_run`). Failures are logged at
    /// `warn` level but do not block the in-process path — the in-memory
    /// counter remains authoritative for the local pod's admission decisions,
    /// with the persistent store providing cross-pod visibility via
    /// [`LaneQueue::pending_count_for_lane_async`].
    counter_store: Option<Arc<dyn LaneCounterStoreDyn>>,
}

impl LaneQueue {
    /// Create a new `LaneQueue`.
    ///
    /// * `max_concurrent_runs` — global cap on simultaneously active runs
    /// * `default_mode` — `QueueMode` applied to newly-created lanes
    ///
    /// The returned queue is backed by the in-process counter only. Use
    /// [`LaneQueue::new_with_counter_store`] to additionally mirror pending
    /// counts to a shared backend.
    pub fn new(max_concurrent_runs: usize, default_mode: QueueMode) -> Self {
        Self {
            lanes: HashMap::new(),
            max_concurrent_runs,
            active_run_count: 0,
            default_mode,
            closed: false,
            post_close_stale_complete_runs: 0,
            counter_store: None,
        }
    }

    /// Create a `LaneQueue` that mirrors every pending-count mutation to
    /// `counter_store` in addition to the in-process count.
    ///
    /// The store is used for cross-pod visibility only — the local in-process
    /// count remains the authoritative admission-control signal. Writes to the
    /// store are fire-and-forget (dispatched via `tokio::spawn`) so a flaky
    /// backend cannot stall the hot path. Callers can read the persistent
    /// count via [`LaneQueue::pending_count_for_lane_async`].
    pub fn new_with_counter_store(
        max_concurrent_runs: usize,
        default_mode: QueueMode,
        counter_store: Arc<dyn LaneCounterStoreDyn>,
    ) -> Self {
        Self {
            lanes: HashMap::new(),
            max_concurrent_runs,
            active_run_count: 0,
            default_mode,
            closed: false,
            post_close_stale_complete_runs: 0,
            counter_store: Some(counter_store),
        }
    }

    /// Whether this queue mirrors pending counts to a persistent store.
    pub fn has_counter_store(&self) -> bool {
        self.counter_store.is_some()
    }

    /// Fire-and-forget an `increment(delta)` against the configured counter
    /// store, if any. Runs on the caller's Tokio runtime; errors are logged.
    fn notify_counter_increment(&self, lane_id: &str, delta: i64) {
        let Some(store) = self.counter_store.as_ref() else {
            return;
        };
        let store = Arc::clone(store);
        let lane_id = lane_id.to_string();
        tokio::spawn(async move {
            if let Err(e) = store.increment_dyn(&lane_id, delta).await {
                tracing::warn!(
                    lane_id = %lane_id,
                    delta,
                    error = %e,
                    "lane counter-store increment failed"
                );
            }
        });
    }

    /// Fire-and-forget a `decrement(delta)` against the configured counter
    /// store, if any. Runs on the caller's Tokio runtime; errors are logged.
    fn notify_counter_decrement(&self, lane_id: &str, delta: i64) {
        let Some(store) = self.counter_store.as_ref() else {
            return;
        };
        let store = Arc::clone(store);
        let lane_id = lane_id.to_string();
        tokio::spawn(async move {
            if let Err(e) = store.decrement_dyn(&lane_id, delta).await {
                tracing::warn!(
                    lane_id = %lane_id,
                    delta,
                    error = %e,
                    "lane counter-store decrement failed"
                );
            }
        });
    }

    // --- private helpers ---------------------------------------------------

    fn get_or_create_lane(&mut self, session_key: &str) -> &mut Lane {
        let mode = self.default_mode;
        self.lanes
            .entry(session_key.to_string())
            .or_insert_with(|| Lane::new(session_key, mode))
    }

    // --- public API --------------------------------------------------------

    /// Add an event to the lane for `event.session_key`.
    ///
    /// Returns an [`EnqueueResult`] describing what the caller should do next:
    ///
    /// * [`EnqueueResult::Ready`] — the lane was idle; the event can be dequeued immediately.
    /// * [`EnqueueResult::Queued`] — a run is active; the event has been buffered.
    /// * [`EnqueueResult::Steer`] — the event has been stored as a steer injection.
    /// * [`EnqueueResult::Interrupt`] — the caller should abort the active run.
    pub fn enqueue(&mut self, event: Event) -> EnqueueResult {
        // Reject new jobs once the queue has been closed for shutdown.
        if self.closed {
            return EnqueueResult::Closed;
        }

        let session_key = event.session_key.clone();
        let lane = self.get_or_create_lane(&session_key);

        if !lane.is_processing {
            // Lane is idle — push and signal ready.
            lane.queue.push_back(QueuedEvent::new(event));
            self.notify_counter_increment(&session_key, 1);
            return EnqueueResult::Ready;
        }

        // A run is active — apply mode-specific behaviour.
        //
        // Track the net pending-count delta so we can mirror it to the
        // persistent counter store. Pending-count is `lane.queue.len() +
        // active_run_count`; steer slots and active_run_count are untouched
        // here, so only queue-length changes matter.
        let (result, pending_delta): (EnqueueResult, i64) = match lane.mode {
            QueueMode::Collect => {
                lane.queue.push_back(QueuedEvent::new(event));
                (EnqueueResult::Queued, 1)
            }
            QueueMode::Followup => {
                lane.queue.push_back(QueuedEvent::new(event));
                (EnqueueResult::Queued, 1)
            }
            QueueMode::Steer => {
                // Newest steer wins; replace any outstanding one. Steer slots
                // do not contribute to `pending_count`, so delta=0.
                lane.steer = Some(QueuedEvent::new_steer(event));
                (EnqueueResult::Steer, 0)
            }
            QueueMode::SteerBacklog => {
                // Inject at tool boundary AND keep a backlog copy for follow-up.
                // Only the backlog push counts toward pending_count.
                let backlog_event = event.clone();
                lane.steer = Some(QueuedEvent::new_steer(event));
                lane.queue.push_back(QueuedEvent::new(backlog_event));
                (EnqueueResult::Steer, 1)
            }
            QueueMode::Interrupt => {
                // Clear any buffered events; the active run must be aborted.
                let cleared = lane.queue.len() as i64;
                lane.queue.clear();
                lane.steer = None;
                lane.queue.push_back(QueuedEvent::new(event));
                // Net change = 1 new entry minus however many we just cleared.
                (EnqueueResult::Interrupt, 1 - cleared)
            }
        };

        if pending_delta > 0 {
            self.notify_counter_increment(&session_key, pending_delta);
        } else if pending_delta < 0 {
            self.notify_counter_decrement(&session_key, -pending_delta);
        }

        result
    }

    /// Return the next batch of events for `session_key` and mark the lane as processing.
    ///
    /// * `Collect` mode: returns **all** queued events as one batch.
    /// * `Followup` (and any other mode): returns **one** event.
    /// * Returns `None` if the global concurrency cap is reached or the lane has no events.
    pub fn dequeue(&mut self, session_key: &str) -> Option<Vec<QueuedEvent>> {
        if self.active_run_count >= self.max_concurrent_runs {
            return None;
        }

        let lane = self.lanes.get_mut(session_key)?;

        if lane.queue.is_empty() {
            return None;
        }

        let batch: Vec<QueuedEvent> = match lane.mode {
            QueueMode::Collect => lane.queue.drain(..).collect(),
            _ => {
                // Followup, Steer, SteerBacklog, Interrupt — one event at a time.
                let event = lane.queue.pop_front()?;
                vec![event]
            }
        };

        lane.is_processing = true;
        self.active_run_count += 1;

        Some(batch)
    }

    /// Peek at the pending steer event for this session (checked at tool boundaries).
    pub fn peek_steer(&self, session_key: &str) -> Option<&QueuedEvent> {
        self.lanes.get(session_key)?.steer.as_ref()
    }

    /// Remove and return the steer event for this session.
    pub fn take_steer(&mut self, session_key: &str) -> Option<QueuedEvent> {
        self.lanes.get_mut(session_key)?.steer.take()
    }

    /// Mark the active run for `session_key` as complete and decrement the global counter.
    ///
    /// **Idempotent / tolerant:** if the lane does not exist, was not
    /// processing, or the queue is already closed, this is a no-op. The
    /// `post_close_stale_complete_runs` counter is incremented when a call
    /// arrives after the queue is closed but the lane is no longer processing —
    /// this signals the drop-time race (guard dropped after drain saw zero) and
    /// is safe to ignore.
    pub fn complete_run(&mut self, session_key: &str) {
        let mut decrement_store = false;
        if let Some(lane) = self.lanes.get_mut(session_key) {
            if lane.is_processing {
                lane.is_processing = false;
                self.active_run_count = self.active_run_count.saturating_sub(1);
                decrement_store = true;
            } else if self.closed {
                // The run was already counted as done (drain saw it complete),
                // but the RAII guard's drop fired after drain exited. Track it
                // for observability; no state change needed.
                self.post_close_stale_complete_runs =
                    self.post_close_stale_complete_runs.saturating_add(1);
            }
        }

        if decrement_store {
            // pending_count = queued + in_flight; completing a run drops
            // in_flight by 1, so the persistent counter must track that.
            self.notify_counter_decrement(session_key, 1);
        }
    }

    /// How many [`LaneQueue::complete_run`] calls arrived after the queue was
    /// closed and the lane was no longer processing. A non-zero value means a
    /// `LaneRunGuard` drop raced with the drain window — harmless but
    /// observable.
    pub fn post_close_stale_complete_runs(&self) -> usize {
        self.post_close_stale_complete_runs
    }

    /// Change the queue mode for a session.
    ///
    /// Creates the lane if it does not yet exist.
    pub fn set_mode(&mut self, session_key: &str, mode: QueueMode) {
        let lane = self.get_or_create_lane(session_key);
        lane.mode = mode;
    }

    /// How many events are queued (not yet delivered) for this session.
    pub fn lane_depth(&self, session_key: &str) -> usize {
        self.lanes
            .get(session_key)
            .map(|l| l.queue.len())
            .unwrap_or(0)
    }

    /// Current number of globally active runs.
    pub fn active_runs(&self) -> usize {
        self.active_run_count
    }

    /// Whether the lane has at least one queued event.
    pub fn has_pending(&self, session_key: &str) -> bool {
        self.lanes
            .get(session_key)
            .map(|l| !l.queue.is_empty())
            .unwrap_or(false)
    }

    /// Total number of jobs currently waiting in any lane **plus** jobs that
    /// have been dequeued but whose `complete_run` has not yet fired.
    ///
    /// Returned as a `Result` so future Postgres-backed implementations of the
    /// same API shape can surface database errors without breaking callers.
    pub fn pending_count(&self) -> Result<usize, DbError> {
        let queued: usize = self.lanes.values().map(|l| l.queue.len()).sum();
        // Each active run represents one in-flight job that has been dequeued
        // but not yet acked via `complete_run`.
        Ok(queued + self.active_run_count)
    }

    /// Cross-pod pending count for a single lane, read from the configured
    /// counter store.
    ///
    /// When a persistent counter store is wired via
    /// [`LaneQueue::new_with_counter_store`], this returns that store's view of
    /// the lane's pending count (summed across every gateway pod sharing the
    /// backend). When no store is configured, this returns the local
    /// per-lane queue depth (queued items only — in-flight items are tracked
    /// globally via `active_run_count` and do not have a per-lane breakdown).
    ///
    /// Callers that need strict cross-pod admission control should configure a
    /// Postgres-backed store in the constructor.
    pub async fn pending_count_for_lane_async(
        &self,
        session_key: &str,
    ) -> Result<i64, DbError> {
        if let Some(store) = self.counter_store.as_ref() {
            store.snapshot_dyn(session_key).await
        } else {
            Ok(self.lane_depth(session_key) as i64)
        }
    }

    /// Mark the queue as closed so that subsequent [`LaneQueue::enqueue`]
    /// calls return [`EnqueueResult::Closed`]. Idempotent.
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Whether the queue has been closed via [`LaneQueue::close`].
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Poll [`LaneQueue::pending_count`] until it reaches zero or `deadline`
    /// elapses, returning a [`DrainOutcome`] that summarises the outcome.
    ///
    /// This does **not** cancel or abort in-flight jobs — higher-level
    /// shutdown code is responsible for that. `drain` only waits for already
    /// accepted jobs to finish.
    ///
    /// **Locking note:** when a `LaneQueue` is shared behind a
    /// `tokio::sync::Mutex` (as in the gateway), calling `drain(&self)` from a
    /// held mutex guard blocks every other task that wants to call
    /// `complete_run` — which means pending jobs can never finish. Prefer
    /// [`LaneQueue::drain_shared`] for the mutex-wrapped case; `drain(&self)`
    /// is intended for owning callers and for tests.
    pub async fn drain(&self, deadline: Duration) -> Result<DrainOutcome, DbError> {
        let start_count = self.pending_count()?;
        let wall_clock_start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(100);

        loop {
            let remaining = self.pending_count()?;
            if remaining == 0 {
                return Ok(DrainOutcome {
                    drained: start_count,
                    remaining: 0,
                    timed_out: false,
                });
            }

            if wall_clock_start.elapsed() >= deadline {
                return Ok(DrainOutcome {
                    drained: start_count.saturating_sub(remaining),
                    remaining,
                    timed_out: true,
                });
            }

            // Sleep for whichever is shorter: the poll interval, or the time
            // left until the deadline.
            let left = deadline.saturating_sub(wall_clock_start.elapsed());
            let sleep_for = std::cmp::min(poll_interval, left);
            if sleep_for.is_zero() {
                // deadline has just elapsed; loop back to emit timed_out.
                continue;
            }
            tokio::time::sleep(sleep_for).await;
        }
    }

    /// Graceful-shutdown drain for a [`LaneQueue`] wrapped in a
    /// [`tokio::sync::Mutex`]. This is the variant used by the gateway.
    ///
    /// Unlike [`LaneQueue::drain`], this helper flips the closed flag up-front
    /// (so no new jobs enter while draining) and **releases the mutex between
    /// polls**, so other tasks (e.g. the `event_loop` calling `complete_run`)
    /// can make progress during the wait.
    pub async fn drain_shared(
        queue: &tokio::sync::Mutex<LaneQueue>,
        deadline: Duration,
    ) -> Result<DrainOutcome, DbError> {
        // Flip the closed flag and snapshot the starting pending count under
        // the lock, then drop the guard before any await.
        let start_count = {
            let mut q = queue.lock().await;
            q.close();
            q.pending_count()?
        };

        let wall_clock_start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(100);

        loop {
            let remaining = {
                let q = queue.lock().await;
                q.pending_count()?
            };

            if remaining == 0 {
                return Ok(DrainOutcome {
                    drained: start_count,
                    remaining: 0,
                    timed_out: false,
                });
            }

            if wall_clock_start.elapsed() >= deadline {
                return Ok(DrainOutcome {
                    drained: start_count.saturating_sub(remaining),
                    remaining,
                    timed_out: true,
                });
            }

            let left = deadline.saturating_sub(wall_clock_start.elapsed());
            let sleep_for = std::cmp::min(poll_interval, left);
            if sleep_for.is_zero() {
                continue;
            }
            tokio::time::sleep(sleep_for).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::{
        event::{Event, EventSource},
        principal::{PrincipalId, PrincipalKind, PrincipalRef},
    };

    fn principal() -> PrincipalRef {
        PrincipalRef {
            id: PrincipalId::new("test-user"),
            kind: PrincipalKind::Human,
        }
    }

    fn make_event(session_key: &str) -> Event {
        Event::message("sera", session_key, principal(), "hello")
    }

    // --- basic happy path --------------------------------------------------

    #[test]
    fn enqueue_single_event_to_idle_lane_is_ready() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        let event = make_event("s1");
        let result = q.enqueue(event);
        assert_eq!(result, EnqueueResult::Ready);
        assert_eq!(q.lane_depth("s1"), 1);
    }

    #[test]
    fn dequeue_returns_event_and_marks_processing() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(make_event("s1"));
        let batch = q.dequeue("s1").expect("should return batch");
        assert_eq!(batch.len(), 1);
        assert_eq!(q.active_runs(), 1);
        assert_eq!(q.lane_depth("s1"), 0);
    }

    // --- enqueue while processing ------------------------------------------

    #[test]
    fn enqueue_while_processing_returns_queued() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(make_event("s1"));
        q.dequeue("s1"); // lane now processing
        let result = q.enqueue(make_event("s1"));
        assert_eq!(result, EnqueueResult::Queued);
    }

    // --- collect mode ------------------------------------------------------

    #[test]
    fn collect_mode_batches_all_queued_events() {
        let mut q = LaneQueue::new(4, QueueMode::Collect);
        q.enqueue(make_event("s1"));
        q.dequeue("s1"); // lane is processing after first event
        q.enqueue(make_event("s1")); // +1
        q.enqueue(make_event("s1")); // +1
        q.complete_run("s1"); // finish run; 2 events remain queued

        // dequeue should return all 2 as one batch
        let batch = q.dequeue("s1").expect("should return batch");
        assert_eq!(batch.len(), 2);
    }

    // --- followup mode -----------------------------------------------------

    #[test]
    fn followup_mode_delivers_one_at_a_time() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(make_event("s1"));
        q.dequeue("s1"); // processing
        q.enqueue(make_event("s1")); // queued #1
        q.enqueue(make_event("s1")); // queued #2
        q.complete_run("s1");

        let batch1 = q.dequeue("s1").expect("first");
        assert_eq!(batch1.len(), 1);
        q.complete_run("s1");

        let batch2 = q.dequeue("s1").expect("second");
        assert_eq!(batch2.len(), 1);
        q.complete_run("s1");

        assert!(q.dequeue("s1").is_none());
    }

    // --- steer mode --------------------------------------------------------

    #[test]
    fn steer_mode_enqueue_during_run_sets_steer_event() {
        let mut q = LaneQueue::new(4, QueueMode::Steer);
        q.enqueue(make_event("s1"));
        q.dequeue("s1"); // processing

        let result = q.enqueue(make_event("s1"));
        assert_eq!(result, EnqueueResult::Steer);

        let peeked = q.peek_steer("s1").expect("steer should exist");
        assert!(peeked.is_steer);
    }

    #[test]
    fn take_steer_removes_it() {
        let mut q = LaneQueue::new(4, QueueMode::Steer);
        q.enqueue(make_event("s1"));
        q.dequeue("s1");
        q.enqueue(make_event("s1"));

        let taken = q.take_steer("s1").expect("steer present");
        assert!(taken.is_steer);
        assert!(q.peek_steer("s1").is_none());
    }

    #[test]
    fn steer_newest_wins() {
        let mut q = LaneQueue::new(4, QueueMode::Steer);
        q.enqueue(make_event("s1"));
        q.dequeue("s1");

        let mut e1 = make_event("s1");
        e1.text = Some("first steer".to_string());
        q.enqueue(e1);

        let mut e2 = make_event("s1");
        e2.text = Some("second steer".to_string());
        q.enqueue(e2);

        let taken = q.take_steer("s1").expect("steer present");
        assert_eq!(taken.event.text.as_deref(), Some("second steer"));
    }

    // --- steer_backlog mode ------------------------------------------------

    #[test]
    fn steer_backlog_sets_steer_and_queues_copy() {
        let mut q = LaneQueue::new(4, QueueMode::SteerBacklog);
        q.enqueue(make_event("s1"));
        q.dequeue("s1"); // processing

        let result = q.enqueue(make_event("s1"));
        assert_eq!(result, EnqueueResult::Steer);

        // steer slot populated
        assert!(q.peek_steer("s1").is_some());
        // backlog copy queued for follow-up
        assert_eq!(q.lane_depth("s1"), 1);
    }

    // --- interrupt mode ----------------------------------------------------

    #[test]
    fn interrupt_mode_clears_existing_queue() {
        let mut q = LaneQueue::new(4, QueueMode::Interrupt);
        q.enqueue(make_event("s1"));
        q.dequeue("s1"); // processing
        q.enqueue(make_event("s1")); // this gets cleared by interrupt below
        assert_eq!(q.lane_depth("s1"), 1);

        let result = q.enqueue(make_event("s1")); // interrupt
        assert_eq!(result, EnqueueResult::Interrupt);
        // previous event cleared; only the interrupt event remains
        assert_eq!(q.lane_depth("s1"), 1);
    }

    // --- global concurrency cap --------------------------------------------

    #[test]
    fn global_concurrency_cap_blocks_dequeue() {
        let mut q = LaneQueue::new(2, QueueMode::Followup);

        q.enqueue(make_event("s1"));
        q.enqueue(make_event("s2"));
        q.enqueue(make_event("s3"));

        assert!(q.dequeue("s1").is_some()); // slot 1
        assert!(q.dequeue("s2").is_some()); // slot 2
        assert!(q.dequeue("s3").is_none()); // cap reached
        assert_eq!(q.active_runs(), 2);
    }

    // --- complete_run ------------------------------------------------------

    #[test]
    fn complete_run_decrements_active_count() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(make_event("s1"));
        q.dequeue("s1");
        assert_eq!(q.active_runs(), 1);
        q.complete_run("s1");
        assert_eq!(q.active_runs(), 0);
    }

    #[test]
    fn complete_run_on_nonexistent_lane_is_noop() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.complete_run("nonexistent"); // must not panic
        assert_eq!(q.active_runs(), 0);
    }

    #[test]
    fn complete_run_twice_does_not_underflow() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(make_event("s1"));
        q.dequeue("s1");
        q.complete_run("s1");
        q.complete_run("s1"); // second call — must not underflow
        assert_eq!(q.active_runs(), 0);
    }

    // --- set_mode ----------------------------------------------------------

    #[test]
    fn set_mode_changes_behaviour() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(make_event("s1"));
        q.dequeue("s1"); // processing under Followup

        q.set_mode("s1", QueueMode::Interrupt);

        // Now the lane is in Interrupt mode; new event should trigger interrupt
        q.enqueue(make_event("s1")); // normal followup queue entry
        q.complete_run("s1");

        // switch to collect before second run
        q.set_mode("s1", QueueMode::Collect);
        q.dequeue("s1"); // processing
        q.enqueue(make_event("s1")); // +1
        q.enqueue(make_event("s1")); // +1
        q.complete_run("s1");

        let batch = q.dequeue("s1").expect("collect batch");
        assert_eq!(batch.len(), 2);
    }

    // --- lane_depth --------------------------------------------------------

    #[test]
    fn lane_depth_accuracy() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        assert_eq!(q.lane_depth("s1"), 0);

        q.enqueue(make_event("s1"));
        assert_eq!(q.lane_depth("s1"), 1);

        q.enqueue(make_event("s1")); // also goes to queue since lane not yet processing
        // Wait — first enqueue returns Ready, so lane is idle.
        // Second enqueue: lane still idle (dequeue hasn't been called), so also Ready.
        assert_eq!(q.lane_depth("s1"), 2);

        q.dequeue("s1"); // pops 1 event
        assert_eq!(q.lane_depth("s1"), 1);
    }

    // --- empty dequeue -----------------------------------------------------

    #[test]
    fn dequeue_from_empty_lane_returns_none() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        assert!(q.dequeue("s1").is_none());
    }

    #[test]
    fn dequeue_nonexistent_lane_returns_none() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        assert!(q.dequeue("no-such-session").is_none());
    }

    // --- has_pending -------------------------------------------------------

    #[test]
    fn has_pending_reflects_queue_state() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        assert!(!q.has_pending("s1"));

        q.enqueue(make_event("s1"));
        assert!(q.has_pending("s1"));

        q.dequeue("s1");
        assert!(!q.has_pending("s1"));
    }

    // --- serde round-trips -------------------------------------------------

    #[test]
    fn queue_mode_serde_round_trip() {
        let modes = [
            (QueueMode::Collect, "\"collect\""),
            (QueueMode::Followup, "\"followup\""),
            (QueueMode::Steer, "\"steer\""),
            (QueueMode::SteerBacklog, "\"steer_backlog\""),
            (QueueMode::Interrupt, "\"interrupt\""),
        ];
        for (mode, expected) in modes {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, expected);
            let parsed: QueueMode = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, mode);
        }
    }

    #[test]
    fn enqueue_result_serde_round_trip() {
        let results = [
            (EnqueueResult::Ready, "\"ready\""),
            (EnqueueResult::Queued, "\"queued\""),
            (EnqueueResult::Steer, "\"steer\""),
            (EnqueueResult::Interrupt, "\"interrupt\""),
            (EnqueueResult::Closed, "\"closed\""),
        ];
        for (result, expected) in results {
            let json = serde_json::to_string(&result).unwrap();
            assert_eq!(json, expected);
            let parsed: EnqueueResult = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, result);
        }
    }

    // --- pending_count ------------------------------------------------------

    #[test]
    fn pending_count_empty_queue_is_zero() {
        let q = LaneQueue::new(4, QueueMode::Followup);
        assert_eq!(q.pending_count().unwrap(), 0);
    }

    #[test]
    fn pending_count_matches_total_queued_jobs() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(make_event("s1"));
        q.enqueue(make_event("s1"));
        q.enqueue(make_event("s2"));
        assert_eq!(q.pending_count().unwrap(), 3);
    }

    #[test]
    fn pending_count_includes_in_flight_jobs() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(make_event("s1"));
        // Dequeue moves the job from "queued" to "in-flight"; pending_count
        // must still report it until complete_run is called.
        let batch = q.dequeue("s1").expect("one item dequeued");
        assert_eq!(batch.len(), 1);
        assert_eq!(q.pending_count().unwrap(), 1);

        q.complete_run("s1");
        assert_eq!(q.pending_count().unwrap(), 0);
    }

    // --- close / is_closed -------------------------------------------------

    #[test]
    fn close_blocks_new_enqueues() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        assert!(!q.is_closed());
        q.close();
        assert!(q.is_closed());
        let result = q.enqueue(make_event("s1"));
        assert_eq!(result, EnqueueResult::Closed);
        assert_eq!(q.pending_count().unwrap(), 0);
    }

    #[test]
    fn close_is_idempotent() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.close();
        q.close();
        assert!(q.is_closed());
    }

    // --- drain -------------------------------------------------------------

    #[tokio::test]
    async fn drain_empty_queue_returns_immediately() {
        let q = LaneQueue::new(4, QueueMode::Followup);
        let outcome = q.drain(Duration::from_millis(50)).await.unwrap();
        assert_eq!(outcome.drained, 0);
        assert_eq!(outcome.remaining, 0);
        assert!(!outcome.timed_out);
    }

    #[tokio::test]
    async fn drain_returns_when_jobs_are_acked() {
        // We cannot mutate `q` concurrently with `q.drain(&self)` from the
        // same task without extra coordination, so simulate a completed
        // run by acking _before_ calling drain. That's sufficient to prove
        // that drain exits when the count reaches zero.
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(make_event("s1"));
        q.dequeue("s1");
        q.complete_run("s1");
        assert_eq!(q.pending_count().unwrap(), 0);

        let outcome = q.drain(Duration::from_millis(50)).await.unwrap();
        assert_eq!(outcome.remaining, 0);
        assert!(!outcome.timed_out);
    }

    #[tokio::test]
    async fn drain_times_out_when_jobs_never_ack() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(make_event("s1"));
        q.enqueue(make_event("s2"));
        // Two jobs still pending (never dequeued / acked) — drain must time out.
        let outcome = q.drain(Duration::from_millis(50)).await.unwrap();
        assert!(outcome.timed_out);
        assert_eq!(outcome.remaining, 2);
        assert_eq!(outcome.drained, 0);
    }

    #[test]
    fn drain_outcome_serde_round_trip() {
        let outcome = DrainOutcome {
            drained: 3,
            remaining: 1,
            timed_out: true,
        };
        let json = serde_json::to_string(&outcome).unwrap();
        let parsed: DrainOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, outcome);
    }

    #[tokio::test]
    async fn drain_shared_closes_queue_and_waits_for_completion() {
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let q = Arc::new(Mutex::new(LaneQueue::new(4, QueueMode::Followup)));

        // Preload one in-flight job.
        {
            let mut g = q.lock().await;
            g.enqueue(make_event("s1"));
            g.dequeue("s1");
        }

        // Spawn a completer that will ack the in-flight job after a short delay.
        let completer = {
            let q = Arc::clone(&q);
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                let mut g = q.lock().await;
                g.complete_run("s1");
            })
        };

        let outcome = LaneQueue::drain_shared(&q, Duration::from_millis(500))
            .await
            .unwrap();

        assert!(!outcome.timed_out, "drain should finish before deadline");
        assert_eq!(outcome.remaining, 0);

        // drain_shared must have flipped the closed flag.
        assert!(q.lock().await.is_closed());

        completer.await.unwrap();
    }

    #[tokio::test]
    async fn drain_shared_times_out_when_no_completion() {
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let q = Arc::new(Mutex::new(LaneQueue::new(4, QueueMode::Followup)));
        {
            let mut g = q.lock().await;
            g.enqueue(make_event("s1"));
        }

        let outcome = LaneQueue::drain_shared(&q, Duration::from_millis(50))
            .await
            .unwrap();

        assert!(outcome.timed_out);
        assert_eq!(outcome.remaining, 1);
    }

    // --- drop-time race regression tests -----------------------------------

    /// Regression: `complete_run` called after `drain_shared` has already
    /// observed the queue as empty (simulating `LaneRunGuard::drop` firing
    /// after the drain window closes) must be a no-op and must NOT underflow
    /// `active_run_count`.
    ///
    /// Pre-fix, this scenario could produce a spurious "drain timed out with
    /// remaining=1" because the spawned drop task ran after `drain_shared`
    /// returned, leaving `active_run_count` incorrectly at 1 during the drain
    /// poll. The fix (`blocking_lock` in Drop) prevents the task from being
    /// deferred, but the `complete_run` tolerance is a defence-in-depth guard.
    #[test]
    fn complete_run_after_close_is_noop_and_counted() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(make_event("s1"));
        q.dequeue("s1"); // run is now active (active_run_count = 1)
        q.complete_run("s1"); // normal ack — active_run_count back to 0
        assert_eq!(q.active_runs(), 0);

        // Simulate drain observing count=0 and then calling close().
        q.close();

        // Now a stale `complete_run` arrives (as if the drop task was
        // deferred and runs after drain already exited).
        q.complete_run("s1");

        // active_run_count must still be 0 — no underflow.
        assert_eq!(q.active_runs(), 0);
        // The stale call must be counted for telemetry.
        assert_eq!(q.post_close_stale_complete_runs(), 1);

        // A second stale call continues to accumulate without underflowing.
        q.complete_run("s1");
        assert_eq!(q.active_runs(), 0);
        assert_eq!(q.post_close_stale_complete_runs(), 2);
    }

    /// Regression: `drain_shared` must reach zero and return `timed_out=false`
    /// even when the in-flight guard's `complete_run` fires synchronously (via
    /// `blocking_lock`) during the poll interval.
    ///
    /// This is the happy-path after the fix: `blocking_lock` in Drop ensures
    /// the decrement happens before any subsequent `pending_count()` poll in
    /// `drain_shared`, so drain sees zero and exits cleanly.
    #[tokio::test]
    async fn drain_shared_sees_zero_after_synchronous_complete_run() {
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let q = Arc::new(Mutex::new(LaneQueue::new(4, QueueMode::Followup)));

        // Enqueue + dequeue one job (in-flight, active_run_count = 1).
        {
            let mut g = q.lock().await;
            g.enqueue(make_event("s1"));
            g.dequeue("s1");
            assert_eq!(g.pending_count().unwrap(), 1);
        }

        // Simulate the blocking_lock Drop: complete_run fires synchronously
        // before drain_shared is called, so pending_count is already 0.
        {
            let mut g = q.lock().await;
            g.complete_run("s1");
            assert_eq!(g.pending_count().unwrap(), 0);
        }

        // drain_shared should see zero immediately and return without timing out.
        let outcome = LaneQueue::drain_shared(&q, Duration::from_millis(200))
            .await
            .unwrap();

        assert!(
            !outcome.timed_out,
            "drain must not time out when count is already 0"
        );
        assert_eq!(outcome.remaining, 0);
        assert_eq!(outcome.drained, 0); // started at 0 after the sync complete_run
    }

    /// Regression: `complete_run` called while queue is closed but the lane IS
    /// still marked processing (the normal in-flight completion path during
    /// drain) must still decrement `active_run_count` correctly — we must not
    /// accidentally short-circuit the normal path.
    #[test]
    fn complete_run_while_closed_and_still_processing_decrements_count() {
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(make_event("s1"));
        q.dequeue("s1"); // active_run_count = 1
        q.close(); // queue closed while run is still active

        // complete_run arrives while closed but lane is_processing = true.
        // This is the normal drain path: run finishes naturally.
        q.complete_run("s1");

        assert_eq!(q.active_runs(), 0);
        // This was NOT a stale call — the lane was still processing.
        assert_eq!(q.post_close_stale_complete_runs(), 0);
    }

    // --- source field roundtrip (sanity check Event clone) -----------------

    #[test]
    fn queued_event_preserves_source() {
        let event = Event {
            source: EventSource::Api,
            ..make_event("s1")
        };
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(event);
        let batch = q.dequeue("s1").unwrap();
        assert_eq!(batch[0].event.source, EventSource::Api);
    }

    // --- counter-store wiring (sera-bsq2) ----------------------------------
    //
    // These tests exercise the LaneQueue↔LaneCounterStore seam added in
    // sera-bsq2. They use the in-memory backend because the goal is to verify
    // the wiring, not the Postgres SQL (covered by `sera-db/tests/integration/
    // lane_queue_counter.rs`). The helpers below wait briefly for the
    // fire-and-forget `tokio::spawn` increments to land; this mirrors how
    // real callers observe the store after a lane mutation.

    use crate::lane_queue_counter::{InMemoryLaneCounter, LaneCounterStoreDyn};
    use std::sync::Arc;

    /// Poll the store up to 500 ms waiting for `lane_id` to reach `expected`.
    /// Returns the final observed value.
    async fn wait_for_store(
        store: &Arc<dyn LaneCounterStoreDyn>,
        lane_id: &str,
        expected: i64,
    ) -> i64 {
        for _ in 0..50 {
            let v = store.snapshot_dyn(lane_id).await.unwrap();
            if v == expected {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        store.snapshot_dyn(lane_id).await.unwrap()
    }

    #[tokio::test]
    async fn new_defaults_to_no_counter_store() {
        // `LaneQueue::new` must preserve the legacy single-pod default: no
        // persistent counter, `has_counter_store` returns false.
        let q = LaneQueue::new(4, QueueMode::Followup);
        assert!(!q.has_counter_store(), "new() must not wire a counter store");
    }

    #[tokio::test]
    async fn enqueue_mirrors_pending_to_counter_store() {
        let store: Arc<dyn LaneCounterStoreDyn> =
            Arc::new(InMemoryLaneCounter::new());
        let mut q = LaneQueue::new_with_counter_store(
            4,
            QueueMode::Followup,
            Arc::clone(&store),
        );

        assert!(q.has_counter_store());
        q.enqueue(make_event("s1"));
        q.enqueue(make_event("s1"));

        // Two enqueues → persistent count for lane s1 reaches 2.
        assert_eq!(wait_for_store(&store, "s1", 2).await, 2);
        // Other lanes stay at 0.
        assert_eq!(store.snapshot_dyn("s2").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn complete_run_decrements_counter_store() {
        // Verify the full enqueue → dequeue → complete_run lifecycle leaves the
        // persistent count at zero. Dequeue must NOT perturb the store because
        // the job simply moves from "queued" to "in-flight" (net zero).
        let store: Arc<dyn LaneCounterStoreDyn> =
            Arc::new(InMemoryLaneCounter::new());
        let mut q = LaneQueue::new_with_counter_store(
            4,
            QueueMode::Followup,
            Arc::clone(&store),
        );

        q.enqueue(make_event("s1"));
        assert_eq!(wait_for_store(&store, "s1", 1).await, 1);

        // Dequeue is a no-op for the persistent counter (queue -> in-flight).
        q.dequeue("s1");
        // Small sleep to rule out a spurious store write from dequeue.
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(store.snapshot_dyn("s1").await.unwrap(), 1);

        // complete_run drops pending_count by 1.
        q.complete_run("s1");
        assert_eq!(wait_for_store(&store, "s1", 0).await, 0);
    }

    #[tokio::test]
    async fn two_lane_queues_share_counter_store() {
        // Multi-pod simulation: two independent LaneQueue instances sharing
        // the same counter store must see a consistent global view of the
        // per-lane pending count — the admission-control invariant that
        // PostgresLaneCounter exists to guarantee in production.
        let store: Arc<dyn LaneCounterStoreDyn> =
            Arc::new(InMemoryLaneCounter::new());

        let mut pod_a = LaneQueue::new_with_counter_store(
            4,
            QueueMode::Followup,
            Arc::clone(&store),
        );
        let mut pod_b = LaneQueue::new_with_counter_store(
            4,
            QueueMode::Followup,
            Arc::clone(&store),
        );

        // Each pod accepts one job for the same lane.
        pod_a.enqueue(make_event("lane-shared"));
        pod_b.enqueue(make_event("lane-shared"));

        // Store reflects both.
        assert_eq!(wait_for_store(&store, "lane-shared", 2).await, 2);

        // pod_a runs and completes its job.
        pod_a.dequeue("lane-shared");
        pod_a.complete_run("lane-shared");
        assert_eq!(wait_for_store(&store, "lane-shared", 1).await, 1);

        // pod_b's async query via the new seam reads the authoritative count.
        let seen_by_pod_b = pod_b
            .pending_count_for_lane_async("lane-shared")
            .await
            .unwrap();
        assert_eq!(seen_by_pod_b, 1, "pod_b must observe pod_a's completion");
    }

    #[tokio::test]
    async fn interrupt_mode_decrements_counter_store_for_cleared_queue() {
        // Interrupt mode clears any buffered events on every enqueue during an
        // active run, so the persistent counter must track the net delta of
        // `+1 new - cleared`. This guards the delta math on the interrupt
        // branch of `enqueue`.
        //
        // To stage "multiple queued events cleared by interrupt" we start in
        // Followup mode (so enqueues accumulate), then flip to Interrupt and
        // issue a single enqueue that must clear all of them atomically.
        let store: Arc<dyn LaneCounterStoreDyn> =
            Arc::new(InMemoryLaneCounter::new());
        let mut q = LaneQueue::new_with_counter_store(
            4,
            QueueMode::Followup,
            Arc::clone(&store),
        );

        q.enqueue(make_event("s1")); // Ready; +1 → 1
        q.dequeue("s1"); // queued → in-flight; store unchanged at 1
        q.enqueue(make_event("s1")); // Queued (Followup); +1 → 2
        q.enqueue(make_event("s1")); // Queued (Followup); +1 → 3

        // Baseline: 1 in-flight + 2 queued = 3.
        assert_eq!(wait_for_store(&store, "s1", 3).await, 3);

        // Flip to Interrupt and issue one more enqueue — clears the 2 buffered
        // items, pushes 1. Net delta = 1 - 2 = -1, so store drops to 2.
        q.set_mode("s1", QueueMode::Interrupt);
        q.enqueue(make_event("s1"));
        assert_eq!(wait_for_store(&store, "s1", 2).await, 2);
    }

    #[tokio::test]
    async fn pending_count_for_lane_async_uses_store_when_present() {
        let store: Arc<dyn LaneCounterStoreDyn> =
            Arc::new(InMemoryLaneCounter::new());
        // Seed the store directly to simulate a sibling pod having enqueued
        // something before this pod came up.
        store.increment_dyn("s-seeded", 5).await.unwrap();

        let q = LaneQueue::new_with_counter_store(
            4,
            QueueMode::Followup,
            Arc::clone(&store),
        );
        // Local lanes map is empty, but the authoritative store has 5.
        assert_eq!(q.lane_depth("s-seeded"), 0);
        assert_eq!(
            q.pending_count_for_lane_async("s-seeded").await.unwrap(),
            5
        );
    }

    #[tokio::test]
    async fn pending_count_for_lane_async_falls_back_to_local_depth() {
        // Without a counter store, the async helper still reports the local
        // per-lane queue depth so single-node callers see a sensible value.
        let mut q = LaneQueue::new(4, QueueMode::Followup);
        q.enqueue(make_event("s1"));
        q.enqueue(make_event("s1"));
        assert_eq!(
            q.pending_count_for_lane_async("s1").await.unwrap(),
            2
        );
        // And for an unseen lane.
        assert_eq!(
            q.pending_count_for_lane_async("missing").await.unwrap(),
            0
        );
    }
}
