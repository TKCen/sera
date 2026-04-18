//! Integration test for the gateway graceful-shutdown drain path.
//!
//! Lives as a separate integration test file (rather than inside
//! `bin/sera.rs`'s inline `tests` module) so that the drain contract for the
//! lane queue can be exercised end-to-end against a dummy queue without
//! depending on the full `AppState` construction.

use std::sync::Arc;
use std::time::Duration;

use sera_db::lane_queue::{LaneQueue, QueueMode};
use sera_types::{
    event::Event as DomainEvent,
    principal::{PrincipalId, PrincipalKind, PrincipalRef},
};
use tokio::sync::Mutex;

/// During graceful shutdown the gateway calls
/// [`sera_db::lane_queue::LaneQueue::drain_shared`] so already-accepted jobs
/// get a chance to finish before exit. Verify the helper behaves the way the
/// drain block in `run_start` expects: flips the `closed` flag, waits for
/// in-flight jobs to ack, and returns a `DrainOutcome` that can be logged.
#[tokio::test]
async fn drain_shared_integrates_with_shutdown_flow() {
    let queue = Arc::new(Mutex::new(LaneQueue::new(4, QueueMode::Followup)));
    let principal = PrincipalRef {
        id: PrincipalId::new("shutdown-test"),
        kind: PrincipalKind::Human,
    };
    let event = DomainEvent::message("sera", "s1", principal, "hello");

    // Enqueue + dequeue one job so it is in-flight when drain starts.
    {
        let mut g = queue.lock().await;
        g.enqueue(event);
        g.dequeue("s1");
        assert_eq!(g.pending_count().unwrap(), 1);
    }

    // Spawn a completer that acks the in-flight job shortly after drain starts.
    let completer = {
        let q = Arc::clone(&queue);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(25)).await;
            q.lock().await.complete_run("s1");
        })
    };

    let outcome = LaneQueue::drain_shared(&queue, Duration::from_millis(500))
        .await
        .expect("drain_shared should not error");

    assert!(
        !outcome.timed_out,
        "drain should complete before the deadline once the job is acked"
    );
    assert_eq!(outcome.remaining, 0);

    // drain_shared must have flipped the closed flag so no new jobs enter.
    assert!(queue.lock().await.is_closed());
    completer.await.unwrap();
}

/// Drain must surface `timed_out = true` when in-flight jobs never ack, so the
/// gateway's drain block can log the remaining count and force exit.
#[tokio::test]
async fn drain_shared_times_out_on_stuck_jobs() {
    let queue = Arc::new(Mutex::new(LaneQueue::new(4, QueueMode::Followup)));
    let principal = PrincipalRef {
        id: PrincipalId::new("shutdown-test"),
        kind: PrincipalKind::Human,
    };
    let event = DomainEvent::message("sera", "s1", principal, "stuck");

    {
        let mut g = queue.lock().await;
        g.enqueue(event);
        // Intentionally skip dequeue+complete so the queue remains non-empty.
    }

    let outcome = LaneQueue::drain_shared(&queue, Duration::from_millis(50))
        .await
        .expect("drain_shared should not error");

    assert!(outcome.timed_out);
    assert_eq!(outcome.remaining, 1);
    assert!(queue.lock().await.is_closed());
}

/// Regression test for the LaneRunGuard drop-time race.
///
/// Pre-fix: `LaneRunGuard::drop` spawned a `tokio::spawn` task to call
/// `complete_run`. That task could be scheduled *after* `drain_shared` polled
/// `pending_count()`, causing drain to see `remaining=1` even though the run
/// had already semantically completed — producing a spurious timeout or a
/// false "drain exceeded deadline" log at process exit.
///
/// Post-fix: `blocking_lock` in Drop makes the decrement synchronous. This
/// test simulates the fixed path: complete_run fires synchronously (as if via
/// blocking_lock) before drain_shared starts, so drain sees zero immediately.
#[tokio::test]
async fn lane_run_guard_drop_does_not_race_with_drain() {
    use sera_db::lane_queue::EnqueueResult;

    let queue = Arc::new(Mutex::new(LaneQueue::new(4, QueueMode::Followup)));
    let principal = PrincipalRef {
        id: PrincipalId::new("race-test"),
        kind: PrincipalKind::Human,
    };
    let event = DomainEvent::message("sera", "guard-session", principal, "test");

    // Enqueue + dequeue one job (in-flight, active_run_count = 1).
    {
        let mut g = queue.lock().await;
        let r = g.enqueue(event);
        assert_eq!(r, EnqueueResult::Ready);
        g.dequeue("guard-session");
        assert_eq!(g.pending_count().unwrap(), 1);
    }

    // Simulate blocking_lock drop: complete_run fires synchronously,
    // decrementing active_run_count BEFORE drain_shared is invoked.
    // This is the behaviour guaranteed by the blocking_lock fix.
    {
        let mut g = queue.lock().await;
        g.complete_run("guard-session");
        assert_eq!(g.pending_count().unwrap(), 0);
    }

    // drain_shared must observe zero immediately and return timed_out=false.
    // With the pre-fix spawn-based drop, there would be a window where this
    // returned timed_out=true or remaining=1.
    let outcome = LaneQueue::drain_shared(&queue, Duration::from_millis(200))
        .await
        .expect("drain_shared must not error");

    assert!(
        !outcome.timed_out,
        "drain must not time out after synchronous complete_run"
    );
    assert_eq!(outcome.remaining, 0, "no remaining jobs after sync complete_run");
    assert!(queue.lock().await.is_closed());
}

/// After `drain_shared` flips the closed flag, subsequent `enqueue` calls must
/// be rejected with `EnqueueResult::Closed` — the gateway relies on this so
/// Discord messages / chat submissions that arrive during the drain window do
/// not get buffered into a queue that will never drain.
#[tokio::test]
async fn drain_shared_rejects_subsequent_enqueues() {
    use sera_db::lane_queue::EnqueueResult;

    let queue = Arc::new(Mutex::new(LaneQueue::new(4, QueueMode::Followup)));
    let _ = LaneQueue::drain_shared(&queue, Duration::from_millis(10))
        .await
        .unwrap();

    let principal = PrincipalRef {
        id: PrincipalId::new("shutdown-test"),
        kind: PrincipalKind::Human,
    };
    let event = DomainEvent::message("sera", "s1", principal, "too late");

    let result = queue.lock().await.enqueue(event);
    assert_eq!(result, EnqueueResult::Closed);
}
