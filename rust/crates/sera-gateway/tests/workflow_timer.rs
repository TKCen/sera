//! End-to-end integration test for the Wave E Phase 1 Timer gate (sera-kgi8).
//!
//! Exercises the full scheduler loop without the 5s ticker: a Timer task is
//! created with a deadline 2s in the past, [`tick`] runs once, and the store
//! reflects the resolved transition.
//!
//! The scheduler consults `chrono::Utc::now` which cannot be driven by
//! `tokio::time::pause` / `advance`, so the Phase 1 test puts the deadline in
//! the past and drives a single tick directly. The 5-second ticker behaviour
//! is covered by unit tests in the scheduler module; this test exercises the
//! HTTP → store → scheduler → store wake path.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration as ChronoDuration, Utc};

use sera_gateway::scheduler::{spawn_scheduler, tick};
use sera_gateway::workflow_store::{
    InMemoryWorkflowTaskStore, SchedulerTaskStatus, WorkflowTaskRecord, WorkflowTaskStore,
};
use sera_workflow::task::WorkflowTaskInput;
use sera_workflow::{AwaitType, WorkflowTask, WorkflowTaskStatus, WorkflowTaskType};

fn make_timer_task(not_before_offset_secs: i64) -> WorkflowTask {
    let now = Utc::now();
    let mut task = WorkflowTask::new(WorkflowTaskInput {
        title: "timer gate exemplar".into(),
        description: "Wave E Phase 1".into(),
        acceptance_criteria: Vec::new(),
        status: WorkflowTaskStatus::Open,
        priority: 5,
        task_type: WorkflowTaskType::Meta,
        source_formula: None,
        source_location: None,
        created_at: now,
    });
    task.await_type = Some(AwaitType::Timer {
        not_before: now + ChronoDuration::seconds(not_before_offset_secs),
    });
    task
}

#[tokio::test]
async fn timer_gate_resolves_after_deadline() {
    let store = Arc::new(InMemoryWorkflowTaskStore::new());

    // Deadline already elapsed → the first tick must resolve.
    let task = make_timer_task(-2);
    let task_id = task.id.to_string();

    let record = WorkflowTaskRecord {
        task,
        agent_id: "sera".into(),
        resume_token: "tok-1".into(),
        status: SchedulerTaskStatus::Pending,
        resolved_at: None,
    };
    store.insert(record).await;

    let resolved = tick(Arc::clone(&store) as Arc<dyn WorkflowTaskStore>).await;
    assert_eq!(resolved, 1, "timer gate with past deadline must resolve on first tick");

    let rec = store.get(&task_id).await.expect("record is still in store");
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
    assert!(rec.resolved_at.is_some());
}

#[tokio::test]
async fn timer_gate_blocks_before_deadline() {
    let store = Arc::new(InMemoryWorkflowTaskStore::new());

    // Deadline 60s in the future → tick must NOT resolve.
    let task = make_timer_task(60);
    let task_id = task.id.to_string();

    let record = WorkflowTaskRecord {
        task,
        agent_id: "sera".into(),
        resume_token: "tok-2".into(),
        status: SchedulerTaskStatus::Pending,
        resolved_at: None,
    };
    store.insert(record).await;

    let resolved = tick(Arc::clone(&store) as Arc<dyn WorkflowTaskStore>).await;
    assert_eq!(resolved, 0, "future-deadline timer must not resolve");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Pending);
    assert!(rec.resolved_at.is_none());
}

#[tokio::test]
async fn scheduler_background_task_resolves_pending_timer() {
    // Exercises spawn_scheduler end-to-end: start the background task, insert
    // a task with an elapsed deadline, wait briefly, and confirm the
    // scheduler marked it resolved. Bounded by a 10s timeout so a stalled
    // scheduler fails fast rather than hanging CI.
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let shutting_down = Arc::new(AtomicBool::new(false));

    let handle = spawn_scheduler(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::clone(&shutting_down),
    );

    let task = make_timer_task(-1);
    let task_id = task.id.to_string();
    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "sera".into(),
            resume_token: "tok-bg".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    // Poll for resolution — the scheduler ticks every TICK_INTERVAL (5s). We
    // allow up to 10s so a single missed tick boundary does not flake.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let resolved = loop {
        if let Some(rec) = store.get(&task_id).await
            && rec.status == SchedulerTaskStatus::Resolved
        {
            break true;
        }
        if std::time::Instant::now() >= deadline {
            break false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    };

    shutting_down.store(true, std::sync::atomic::Ordering::SeqCst);
    // Best-effort cleanup: drop the handle; the loop exits on next iteration.
    drop(handle);

    assert!(resolved, "background scheduler must resolve pending timer");
}
