//! Lane-aware FIFO queue — in-memory, single-writer-per-session with global concurrency throttle.
//!
//! Per SPEC-gateway §5, each session gets its own "lane" that enforces ordering
//! and mode-specific delivery semantics (collect, followup, steer, etc.).
//! This is the Tier-1 (local/embedded) implementation — no database required.

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use sera_types::event::Event;

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

/// The main lane-aware queue manager.
///
/// Keyed by `session_key`; enforces a global concurrency cap across all lanes.
pub struct LaneQueue {
    lanes: HashMap<String, Lane>,
    max_concurrent_runs: usize,
    active_run_count: usize,
    default_mode: QueueMode,
}

impl LaneQueue {
    /// Create a new `LaneQueue`.
    ///
    /// * `max_concurrent_runs` — global cap on simultaneously active runs
    /// * `default_mode` — `QueueMode` applied to newly-created lanes
    pub fn new(max_concurrent_runs: usize, default_mode: QueueMode) -> Self {
        Self {
            lanes: HashMap::new(),
            max_concurrent_runs,
            active_run_count: 0,
            default_mode,
        }
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
        let session_key = event.session_key.clone();
        let lane = self.get_or_create_lane(&session_key);

        if !lane.is_processing {
            // Lane is idle — push and signal ready.
            lane.queue.push_back(QueuedEvent::new(event));
            return EnqueueResult::Ready;
        }

        // A run is active — apply mode-specific behaviour.
        match lane.mode {
            QueueMode::Collect => {
                lane.queue.push_back(QueuedEvent::new(event));
                EnqueueResult::Queued
            }
            QueueMode::Followup => {
                lane.queue.push_back(QueuedEvent::new(event));
                EnqueueResult::Queued
            }
            QueueMode::Steer => {
                // Newest steer wins; replace any outstanding one.
                lane.steer = Some(QueuedEvent::new_steer(event));
                EnqueueResult::Steer
            }
            QueueMode::SteerBacklog => {
                // Inject at tool boundary AND keep a backlog copy for follow-up.
                let backlog_event = event.clone();
                lane.steer = Some(QueuedEvent::new_steer(event));
                lane.queue.push_back(QueuedEvent::new(backlog_event));
                EnqueueResult::Steer
            }
            QueueMode::Interrupt => {
                // Clear any buffered events; the active run must be aborted.
                lane.queue.clear();
                lane.steer = None;
                lane.queue.push_back(QueuedEvent::new(event));
                EnqueueResult::Interrupt
            }
        }
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
    /// Has no effect if the lane does not exist or was not processing.
    pub fn complete_run(&mut self, session_key: &str) {
        if let Some(lane) = self.lanes.get_mut(session_key)
            && lane.is_processing
        {
            lane.is_processing = false;
            self.active_run_count = self.active_run_count.saturating_sub(1);
        }
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
        ];
        for (result, expected) in results {
            let json = serde_json::to_string(&result).unwrap();
            assert_eq!(json, expected);
            let parsed: EnqueueResult = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, result);
        }
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
}
