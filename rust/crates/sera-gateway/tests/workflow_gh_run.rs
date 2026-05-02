//! Integration test for the GhRun workflow gate (sera-4fel).
//!
//! Exercises the full scheduler loop for GhRun-gated tasks:
//!
//! 1. A GhRun task is created (Pending) with no status in the store →
//!    tick must NOT resolve it (unknown run = not ready).
//! 2. The run is marked terminal in `InMemoryGhRunStateStore` →
//!    tick resolves the task.
//! 3. Background scheduler (`spawn_scheduler`) resolves the task after
//!    the run status is populated.
//!
//! No real GitHub HTTP dependency — all lookup is via the in-process store.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::Utc;

use sera_gateway::scheduler::{spawn_scheduler, tick};
use sera_gateway::workflow_store::{
    GhRunStateStore, InMemoryGhRunStateStore, InMemoryWorkflowTaskStore, SchedulerTaskStatus,
    WorkflowTaskRecord, WorkflowTaskStore,
};
use sera_mail::lookup::InMemoryMailLookup;
use sera_workflow::task::{GhRunId, GhRunStatus, WorkflowTaskInput};
use sera_workflow::{AwaitType, WorkflowTask, WorkflowTaskStatus, WorkflowTaskType};

fn make_gh_run_task(run_id: &str, repo: &str) -> WorkflowTask {
    let now = Utc::now();
    let mut task = WorkflowTask::new(WorkflowTaskInput {
        title: format!("await gh_run {run_id}"),
        description: "GhRun gate integration test".into(),
        acceptance_criteria: Vec::new(),
        status: WorkflowTaskStatus::Open,
        priority: 5,
        task_type: WorkflowTaskType::Meta,
        source_formula: None,
        source_location: None,
        created_at: now,
    });
    task.await_type = Some(AwaitType::GhRun {
        run_id: GhRunId::new(run_id),
        repo: repo.to_owned(),
    });
    task
}

#[tokio::test]
async fn gh_run_gate_blocks_when_run_unknown() {
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let gh_run_store = Arc::new(InMemoryGhRunStateStore::new());

    let task = make_gh_run_task("42", "acme/my-repo");
    let task_id = task.id.to_string();

    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "agent-1".into(),
            resume_token: "tok-unknown".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    // No status in store → tick must NOT resolve.
    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        Some(Arc::clone(&gh_run_store) as Arc<dyn GhRunStateStore>),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(resolved, 0, "unknown run must not resolve");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Pending);
    assert!(rec.resolved_at.is_none());
}

#[tokio::test]
async fn gh_run_gate_resolves_on_terminal_status() {
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let gh_run_store = Arc::new(InMemoryGhRunStateStore::new());

    let run_id = GhRunId::new("99");
    let task = make_gh_run_task(run_id.as_str(), "acme/my-repo");
    let task_id = task.id.to_string();

    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "agent-1".into(),
            resume_token: "tok-terminal".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    // Mark the run as completed (terminal) in the lookup store.
    gh_run_store.upsert(run_id, GhRunStatus::Completed).await;

    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        Some(Arc::clone(&gh_run_store) as Arc<dyn GhRunStateStore>),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(resolved, 1, "completed run must resolve on first tick");

    let rec = store.get(&task_id).await.expect("record still in store");
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
    assert!(rec.resolved_at.is_some());
}

#[tokio::test]
async fn gh_run_gate_resolves_on_failed_status() {
    // Deliberately proceed on Failed too — the task handler branches on outcome.
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let gh_run_store = Arc::new(InMemoryGhRunStateStore::new());

    let run_id = GhRunId::new("77");
    let task = make_gh_run_task(run_id.as_str(), "acme/other-repo");
    let task_id = task.id.to_string();

    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "agent-2".into(),
            resume_token: "tok-failed".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    gh_run_store.upsert(run_id, GhRunStatus::Failed).await;

    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        Some(Arc::clone(&gh_run_store) as Arc<dyn GhRunStateStore>),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(resolved, 1, "failed run must also resolve (handler branches on outcome)");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
}

#[tokio::test]
async fn gh_run_gate_background_scheduler_resolves_after_status_populated() {
    // Exercises spawn_scheduler end-to-end: start the background task, insert
    // a GhRun task, wait a tick, populate the run status, wait for resolution.
    // Bounded by 10s so a stalled scheduler fails fast rather than hanging CI.
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let gh_run_store = Arc::new(InMemoryGhRunStateStore::new());
    let shutting_down = Arc::new(AtomicBool::new(false));

    let handle = spawn_scheduler(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        Some(Arc::clone(&gh_run_store) as Arc<dyn GhRunStateStore>),
        None,
        None,
        None,
        Arc::clone(&shutting_down),
    );

    let run_id = GhRunId::new("55");
    let task = make_gh_run_task(run_id.as_str(), "acme/bg-repo");
    let task_id = task.id.to_string();

    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "agent-bg".into(),
            resume_token: "tok-bg-run".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    // Populate the run status so the next scheduler tick resolves it.
    gh_run_store.upsert(run_id, GhRunStatus::Completed).await;

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

    assert!(resolved, "background scheduler must resolve pending GhRun task");
}
