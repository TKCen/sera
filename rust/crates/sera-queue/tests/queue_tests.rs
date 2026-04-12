use sera_queue::{
    GlobalThrottle, LaneQueue, LocalQueueBackend, MigrationKind, QueueBackend, QueueMode,
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
