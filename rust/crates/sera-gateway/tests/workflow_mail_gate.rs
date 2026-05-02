//! End-to-end integration test for the Mail gate (sera-0zch).
//!
//! Exercises the full wait-then-deliver-then-resolve cycle without real SMTP/IMAP:
//!
//!  1. Create a Mail-gate task via the HTTP route.
//!  2. Drive a scheduler tick — task must remain Pending (no mail yet).
//!  3. Inject a terminal [`MailEvent::ReplyReceived`] via
//!     [`InMemoryMailLookup::notify`].
//!  4. Drive another tick — task must transition to Resolved.
//!
//! Also exercises [`spawn_scheduler`] end-to-end: starts the background task,
//! injects a mail event, waits up to 10s for the scheduler to resolve the task.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use sera_gateway::scheduler::{spawn_scheduler, tick};
use sera_gateway::workflow_store::{
    InMemoryWorkflowTaskStore, SchedulerTaskStatus, WorkflowTaskRecord, WorkflowTaskStore,
};
use sera_mail::{GateId, InMemoryMailLookup};
use sera_workflow::task::{MailEvent, MailThreadId, WorkflowTaskInput};
use sera_workflow::{AwaitType, WorkflowTask, WorkflowTaskStatus, WorkflowTaskType};

use chrono::Utc;

fn make_mail_task(thread_id: &str) -> WorkflowTask {
    let now = Utc::now();
    let mut task = WorkflowTask::new(WorkflowTaskInput {
        title: "mail gate exemplar".into(),
        description: "sera-0zch".into(),
        acceptance_criteria: Vec::new(),
        status: WorkflowTaskStatus::Open,
        priority: 5,
        task_type: WorkflowTaskType::Meta,
        source_formula: None,
        source_location: None,
        created_at: now,
    });
    task.await_type = Some(AwaitType::Mail {
        thread_id: MailThreadId::new(thread_id),
    });
    task
}

fn pending_record(task: WorkflowTask, token: &str) -> WorkflowTaskRecord {
    WorkflowTaskRecord {
        task,
        agent_id: "sera".into(),
        resume_token: token.into(),
        status: SchedulerTaskStatus::Pending,
        resolved_at: None,
    }
}

// ── tick-level tests ─────────────────────────────────────────────────────────

#[tokio::test]
async fn mail_gate_blocks_before_reply() {
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let lookup = Arc::new(InMemoryMailLookup::new());

    let task = make_mail_task("<thread-1@example.com>");
    let task_id = task.id.to_string();
    store.insert(pending_record(task, "tok-1")).await;

    // No mail delivered yet — tick must NOT resolve.
    let resolved = tick(Arc::clone(&store) as Arc<dyn WorkflowTaskStore>, Arc::clone(&lookup), None, None, None, None).await;
    assert_eq!(resolved, 0, "mail gate without reply must not resolve");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Pending);
    assert!(rec.resolved_at.is_none());
}

#[tokio::test]
async fn mail_gate_resolves_after_reply_received() {
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let lookup = Arc::new(InMemoryMailLookup::new());

    let thread_id = MailThreadId::new("<thread-2@example.com>");
    let task = make_mail_task(thread_id.as_str());
    let task_id = task.id.to_string();
    store.insert(pending_record(task, "tok-2")).await;

    // Inject a terminal event.
    lookup
        .notify(GateId::new("test-gate"), &thread_id, MailEvent::ReplyReceived)
        .unwrap();

    let resolved = tick(Arc::clone(&store) as Arc<dyn WorkflowTaskStore>, Arc::clone(&lookup), None, None, None, None).await;
    assert_eq!(resolved, 1, "mail gate with reply must resolve on next tick");

    let rec = store.get(&task_id).await.expect("record must still exist");
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
    assert!(rec.resolved_at.is_some());
}

#[tokio::test]
async fn mail_gate_resolves_on_closed() {
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let lookup = Arc::new(InMemoryMailLookup::new());

    let thread_id = MailThreadId::new("<thread-3@example.com>");
    let task = make_mail_task(thread_id.as_str());
    let task_id = task.id.to_string();
    store.insert(pending_record(task, "tok-3")).await;

    // Administratively close the thread — gate must also resolve.
    lookup
        .notify(GateId::new("test-gate"), &thread_id, MailEvent::Closed)
        .unwrap();

    let resolved = tick(Arc::clone(&store) as Arc<dyn WorkflowTaskStore>, Arc::clone(&lookup), None, None, None, None).await;
    assert_eq!(resolved, 1, "mail gate with Closed event must resolve");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
}

#[tokio::test]
async fn mail_gate_stays_pending_on_non_terminal_event() {
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let lookup = Arc::new(InMemoryMailLookup::new());

    let thread_id = MailThreadId::new("<thread-4@example.com>");
    let task = make_mail_task(thread_id.as_str());
    let task_id = task.id.to_string();
    store.insert(pending_record(task, "tok-4")).await;

    // Inject a non-terminal Pending event — should not resolve.
    lookup
        .notify(GateId::new("test-gate"), &thread_id, MailEvent::Pending)
        .unwrap();

    let resolved = tick(Arc::clone(&store) as Arc<dyn WorkflowTaskStore>, Arc::clone(&lookup), None, None, None, None).await;
    assert_eq!(resolved, 0, "non-terminal mail event must not resolve gate");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Pending);
}

// ── spawn_scheduler end-to-end test ─────────────────────────────────────────

#[tokio::test]
async fn scheduler_background_task_resolves_pending_mail() {
    // Exercises spawn_scheduler end-to-end: start the background task, inject
    // a reply event, wait briefly, and confirm the scheduler marked it resolved.
    // Bounded by a 10s timeout so a stalled scheduler fails fast.
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let lookup = Arc::new(InMemoryMailLookup::new());
    let shutting_down = Arc::new(AtomicBool::new(false));

    let handle = spawn_scheduler(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::clone(&lookup),
        None,
        None,
        None,
        None,
        Arc::clone(&shutting_down),
    );

    let thread_id = MailThreadId::new("<thread-bg@example.com>");
    let task = make_mail_task(thread_id.as_str());
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

    // Deliver a reply so the scheduler can resolve on its next tick.
    lookup
        .notify(GateId::new("test-gate"), &thread_id, MailEvent::ReplyReceived)
        .unwrap();

    // Poll for resolution — scheduler ticks every TICK_INTERVAL (5s).
    // Allow up to 10s so a single missed tick boundary does not flake.
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
    drop(handle);

    assert!(resolved, "background scheduler must resolve pending mail gate");
}
