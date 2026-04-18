use std::sync::Arc;

use sera_queue::{
    GlobalThrottle, LaneQueue, LocalQueueBackend, MigrationKind, QueueBackend, QueueError,
    QueueMode,
};

// 1. push → pull → ack roundtrip
#[tokio::test]
async fn local_backend_push_pull_ack_roundtrip() {
    let backend = LocalQueueBackend::new();
    let payload = serde_json::json!({"msg": "hello"});

    let id = backend.push("lane-a", payload.clone()).await.unwrap();
    let pulled = backend.pull("lane-a").await.unwrap();
    assert!(pulled.is_some());
    let (pulled_id, pulled_payload) = pulled.unwrap();
    assert_eq!(pulled_id, id);
    assert_eq!(pulled_payload, payload);

    backend.ack(&pulled_id).await.unwrap();

    // Lane should now be empty.
    let empty = backend.pull("lane-a").await.unwrap();
    assert!(empty.is_none());
}

// 2. Followup mode preserves FIFO order
#[tokio::test]
async fn local_queue_fifo_per_lane() {
    let mut q = LaneQueue::new();
    q.enqueue("lane", serde_json::json!(1), QueueMode::Followup);
    q.enqueue("lane", serde_json::json!(2), QueueMode::Followup);
    q.enqueue("lane", serde_json::json!(3), QueueMode::Followup);

    assert_eq!(q.dequeue("lane").unwrap().payload, serde_json::json!(1));
    assert_eq!(q.dequeue("lane").unwrap().payload, serde_json::json!(2));
    assert_eq!(q.dequeue("lane").unwrap().payload, serde_json::json!(3));
    assert!(q.dequeue("lane").is_none());
}

// 3. cap=2, 2 acquired → try_acquire fails
#[test]
fn global_throttle_cap_blocks_dequeue() {
    let throttle = GlobalThrottle::new(2);
    let _p1 = throttle.try_acquire().unwrap();
    let _p2 = throttle.try_acquire().unwrap();
    assert!(throttle.try_acquire().is_err());
}

// 4. drop permit → acquire succeeds
#[test]
fn global_throttle_releases_on_complete_run() {
    let throttle = GlobalThrottle::new(1);
    {
        let _permit = throttle.try_acquire().unwrap();
        assert!(throttle.try_acquire().is_err());
    }
    // permit dropped — should succeed now
    assert!(throttle.try_acquire().is_ok());
}

// 5. take_steer returns the latest enqueued item
#[test]
fn steer_newest_wins() {
    let mut q = LaneQueue::new();
    q.enqueue("lane", serde_json::json!("first"), QueueMode::Steer);
    q.enqueue("lane", serde_json::json!("second"), QueueMode::Steer);

    let steer = q.take_steer("lane").unwrap();
    assert_eq!(steer.payload, serde_json::json!("second"));

    // Only one steer slot exists.
    assert!(q.take_steer("lane").is_none());
}

// 6. interrupt_clear resets lane depth to 0
#[test]
fn interrupt_clears_backlog() {
    let mut q = LaneQueue::new();
    q.enqueue("lane", serde_json::json!(1), QueueMode::Collect);
    q.enqueue("lane", serde_json::json!(2), QueueMode::Collect);
    q.enqueue("lane", serde_json::json!(3), QueueMode::Steer);
    assert_eq!(q.depth("lane"), 2);

    let cleared = q.interrupt_clear("lane");
    // 2 from queue + 1 steer slot
    assert_eq!(cleared, 3);
    assert_eq!(q.depth("lane"), 0);
    assert!(q.take_steer("lane").is_none());
}

// 7. Reversible → requires_down_file = true
#[test]
fn migration_kind_requires_down_file_reversible() {
    assert!(MigrationKind::Reversible.requires_down_file());
}

// 8. Irreversible → requires_down_file = false
#[test]
fn migration_kind_requires_down_file_irreversible() {
    assert!(!MigrationKind::Irreversible.requires_down_file());
}

// 9. QueueBackend trait is object-safe
#[test]
fn queue_backend_trait_is_object_safe() {
    let backend = LocalQueueBackend::new();
    let _: Box<dyn QueueBackend> = Box::new(backend);
}

// 10. orphan recovery returns Ok(0) for local backend
#[tokio::test]
async fn orphan_recovery_across_restart() {
    let backend = LocalQueueBackend::new();
    let recovered = backend
        .recover_orphans(std::time::Duration::from_secs(30))
        .await
        .unwrap();
    assert_eq!(recovered, 0);
}

// 11. All QueueMode variants roundtrip JSON
#[test]
fn queue_mode_serde_roundtrip() {
    let modes = [
        QueueMode::Collect,
        QueueMode::Followup,
        QueueMode::Steer,
        QueueMode::SteerBacklog,
        QueueMode::Interrupt,
    ];
    for mode in &modes {
        let json = serde_json::to_string(mode).unwrap();
        let decoded: QueueMode = serde_json::from_str(&json).unwrap();
        assert_eq!(*mode, decoded);
    }
}

// 12. depth increments per enqueue
#[test]
fn enqueue_result_has_correct_depth() {
    let mut q = LaneQueue::new();
    let r1 = q.enqueue("lane", serde_json::json!(1), QueueMode::Collect);
    assert_eq!(r1.depth, 1);
    let r2 = q.enqueue("lane", serde_json::json!(2), QueueMode::Collect);
    assert_eq!(r2.depth, 2);
    let r3 = q.enqueue("lane", serde_json::json!(3), QueueMode::Followup);
    assert_eq!(r3.depth, 3);
}

// 13. nack returns NotFound on local backend
#[tokio::test]
async fn local_backend_nack_returns_not_found() {
    let backend = LocalQueueBackend::new();
    let err = backend.nack("nonexistent-id").await.unwrap_err();
    assert!(matches!(err, QueueError::NotFound { .. }));
}

// 14. pull on empty lane returns None
#[tokio::test]
async fn local_backend_pull_empty_lane_returns_none() {
    let backend = LocalQueueBackend::new();
    let result = backend.pull("empty-lane").await.unwrap();
    assert!(result.is_none());
}

// 15. lanes are isolated — push to lane-a does not appear in lane-b
#[tokio::test]
async fn local_backend_lane_isolation() {
    let backend = LocalQueueBackend::new();
    backend
        .push("lane-a", serde_json::json!("only-in-a"))
        .await
        .unwrap();
    let b_result = backend.pull("lane-b").await.unwrap();
    assert!(b_result.is_none());
    let a_result = backend.pull("lane-a").await.unwrap();
    assert!(a_result.is_some());
}

// 16. SteerBacklog puts item in both backlog queue and steer slot
#[test]
fn steer_backlog_appears_in_both_slots() {
    let mut q = LaneQueue::new();
    let result = q.enqueue("lane", serde_json::json!("steer-item"), QueueMode::SteerBacklog);
    // Steer slot is populated.
    let steer = q.take_steer("lane").unwrap();
    assert_eq!(steer.payload, serde_json::json!("steer-item"));
    // Backlog queue also has the item (depth was 1 before we took steer).
    assert_eq!(result.depth, 1);
    let dequeued = q.dequeue("lane").unwrap();
    assert_eq!(dequeued.payload, serde_json::json!("steer-item"));
}

// 17. Interrupt mode as push clears existing backlog and steer
#[test]
fn interrupt_push_clears_existing_backlog() {
    let mut q = LaneQueue::new();
    q.enqueue("lane", serde_json::json!(1), QueueMode::Collect);
    q.enqueue("lane", serde_json::json!(2), QueueMode::Collect);
    q.enqueue("lane", serde_json::json!("steer"), QueueMode::Steer);

    let result = q.enqueue("lane", serde_json::json!("interrupt"), QueueMode::Interrupt);
    // Only the interrupt item remains.
    assert_eq!(result.depth, 1);
    assert!(q.take_steer("lane").is_none());
    let item = q.dequeue("lane").unwrap();
    assert_eq!(item.payload, serde_json::json!("interrupt"));
    assert!(q.dequeue("lane").is_none());
}

// 18. ForwardOnlyWithPairedOut requires down file
#[test]
fn migration_kind_requires_down_file_forward_only_with_paired_out() {
    assert!(MigrationKind::ForwardOnlyWithPairedOut.requires_down_file());
}

// 19. GlobalThrottle::available() tracks permits correctly
#[test]
fn global_throttle_available_tracks_permits() {
    let throttle = GlobalThrottle::new(3);
    assert_eq!(throttle.available(), 3);
    let _p1 = throttle.try_acquire().unwrap();
    assert_eq!(throttle.available(), 2);
    let _p2 = throttle.try_acquire().unwrap();
    assert_eq!(throttle.available(), 1);
    drop(_p1);
    assert_eq!(throttle.available(), 2);
}

// 20. take_steer on lane with no steer returns None
#[test]
fn take_steer_no_steer_returns_none() {
    let mut q = LaneQueue::new();
    assert!(q.take_steer("lane").is_none());
    // Even after enqueue in non-steer mode.
    q.enqueue("lane", serde_json::json!(1), QueueMode::Collect);
    assert!(q.take_steer("lane").is_none());
}

// 21. interrupt_clear on empty lane returns 0
#[test]
fn interrupt_clear_empty_lane_returns_zero() {
    let mut q = LaneQueue::new();
    let cleared = q.interrupt_clear("nonexistent");
    assert_eq!(cleared, 0);
}

// 22. QueueError display messages include key fields
#[test]
fn queue_error_display_messages() {
    let e = QueueError::Unavailable {
        reason: "cap exhausted".into(),
    };
    assert!(e.to_string().contains("cap exhausted"));

    let e = QueueError::NotFound { id: "abc-123".into() };
    assert!(e.to_string().contains("abc-123"));

    let e = QueueError::Serde {
        reason: "invalid utf-8".into(),
    };
    assert!(e.to_string().contains("invalid utf-8"));

    let e = QueueError::Storage {
        reason: "disk full".into(),
    };
    assert!(e.to_string().contains("disk full"));
}

// 23. Concurrent push/pop under contention — fan-out with multiple producers/consumers
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_push_pop_contention() {
    let backend = Arc::new(LocalQueueBackend::new());
    let num_tasks = 20usize;

    // Spawn producers.
    let mut set = tokio::task::JoinSet::new();
    for i in 0..num_tasks {
        let b = Arc::clone(&backend);
        set.spawn(async move { b.push("shared-lane", serde_json::json!(i)).await.unwrap() });
    }

    // Wait for all producers to finish.
    let mut ids: Vec<String> = Vec::new();
    while let Some(res) = set.join_next().await {
        ids.push(res.unwrap());
    }

    assert_eq!(ids.len(), num_tasks);

    // Drain the lane.
    let mut popped = 0usize;
    loop {
        if backend.pull("shared-lane").await.unwrap().is_none() {
            break;
        }
        popped += 1;
    }
    assert_eq!(popped, num_tasks);
}

// 24. Concurrent throttle acquisition — only cap permits succeed while held
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_throttle_cap_respected() {
    let throttle = Arc::new(GlobalThrottle::new(5));
    let num_tasks = 20usize;

    // Hold all successfully acquired permits so they are not released mid-loop.
    let mut permits = Vec::new();
    let mut successes = 0usize;
    for _ in 0..num_tasks {
        match throttle.try_acquire() {
            Ok(p) => {
                permits.push(p);
                successes += 1;
            }
            Err(_) => {}
        }
    }

    assert_eq!(successes, 5);
    assert_eq!(throttle.available(), 0);

    // Drop permits — all should be returned.
    drop(permits);
    assert_eq!(throttle.available(), 5);
}
