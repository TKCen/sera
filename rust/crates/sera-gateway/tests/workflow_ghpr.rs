//! End-to-end integration test for the GhPr workflow gate (sera-comg).
//!
//! Mirrors the Timer-gate test (sera-kgi8): a GhPr-gated task is inserted
//! directly into the workflow store, the scheduler tick observes the mock
//! [`GhPrLookup`] reporting a non-terminal state and leaves the task
//! pending, then a state change to `Merged` is published and the next tick
//! resolves the task — fully exercising the store → scheduler → store wake
//! path with a real lookup.
//!
//! HTTP-route coverage for `await_type=gh_pr` lives in
//! `crates/sera-gateway/src/routes/workflow.rs` — that module is loaded
//! directly into the binary (`#[path]`) and is not re-exported through the
//! lib, so this test exercises the gate plumbing rather than the route
//! handler. The mock [`InMemoryGhPrStateStore`] stands in for the
//! production GitHub API poller that lives in a follow-up bead.

use std::sync::Arc;

use chrono::Utc;

use sera_gateway::scheduler::tick;
use sera_gateway::workflow_store::{
    GhPrStateStore, InMemoryGhPrStateStore, InMemoryWorkflowTaskStore, SchedulerTaskStatus,
    WorkflowTaskRecord, WorkflowTaskStore,
};
use sera_mail::InMemoryMailLookup;
use sera_workflow::task::{AwaitType, GhPrId, GhPrState, WorkflowTaskInput};
use sera_workflow::{WorkflowTask, WorkflowTaskStatus, WorkflowTaskType};

fn make_gh_pr_task(pr_id: &str) -> WorkflowTask {
    let now = Utc::now();
    let mut task = WorkflowTask::new(WorkflowTaskInput {
        title: "gh_pr gate exemplar".into(),
        description: "sera-comg".into(),
        acceptance_criteria: Vec::new(),
        status: WorkflowTaskStatus::Open,
        priority: 5,
        task_type: WorkflowTaskType::Meta,
        source_formula: None,
        source_location: None,
        created_at: now,
    });
    task.await_type = Some(AwaitType::GhPr {
        pr_id: GhPrId::new(pr_id.to_string()),
        repo: "owner/repo".into(),
    });
    task
}

#[tokio::test]
async fn gh_pr_gate_blocks_while_pr_open_then_resolves_on_merge() {
    // Wait-then-resume round trip. Insert a GhPr-gated task, drive a tick
    // with the mock lookup reporting Open (non-terminal) — task stays
    // pending. Then publish state=Merged into the lookup and drive another
    // tick — task transitions to Resolved.
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let gh_pr_store: Arc<InMemoryGhPrStateStore> = Arc::new(InMemoryGhPrStateStore::new());

    let task = make_gh_pr_task("owner/repo#42");
    let task_id = task.id.to_string();
    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "sera".into(),
            resume_token: "tok-pr".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    // Lookup currently knows nothing about this PR — Unknown maps to
    // not-ready, so the first tick must NOT resolve the task.
    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        None,
        None,
        None,
        Some(Arc::clone(&gh_pr_store) as Arc<dyn GhPrStateStore>),
    )
    .await;
    assert_eq!(resolved, 0, "unknown PR state must not resolve the gate");
    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Pending);

    // Publish a non-terminal Open state — gate still blocks.
    gh_pr_store
        .upsert(GhPrId::new("owner/repo#42"), GhPrState::Open)
        .await;
    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        None,
        None,
        None,
        Some(Arc::clone(&gh_pr_store) as Arc<dyn GhPrStateStore>),
    )
    .await;
    assert_eq!(resolved, 0, "Open PR must not resolve the gate");
    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Pending);

    // Publish Merged — terminal state, gate resolves on the next tick.
    gh_pr_store
        .upsert(GhPrId::new("owner/repo#42"), GhPrState::Merged)
        .await;
    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        None,
        None,
        None,
        Some(Arc::clone(&gh_pr_store) as Arc<dyn GhPrStateStore>),
    )
    .await;
    assert_eq!(resolved, 1, "Merged PR must resolve the gate on next tick");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
    assert!(rec.resolved_at.is_some());
}

#[tokio::test]
async fn gh_pr_gate_resolves_on_close_without_merge() {
    // Closed-without-merge is also terminal — the workflow handler is
    // expected to branch on Merged vs Closed itself once the gate resolves,
    // so we wake the task in both cases.
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let gh_pr_store: Arc<InMemoryGhPrStateStore> = Arc::new(InMemoryGhPrStateStore::new());

    let task = make_gh_pr_task("owner/repo#7");
    let task_id = task.id.to_string();
    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "sera".into(),
            resume_token: "tok-pr-closed".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    gh_pr_store
        .upsert(GhPrId::new("owner/repo#7"), GhPrState::Closed)
        .await;
    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        None,
        None,
        None,
        Some(Arc::clone(&gh_pr_store) as Arc<dyn GhPrStateStore>),
    )
    .await;
    assert_eq!(resolved, 1, "Closed PR must resolve the gate");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
}

#[tokio::test]
async fn gh_pr_gate_blocks_when_no_lookup_provided() {
    // When `tick` is called with `None` for the GhPr lookup (production
    // boot before the GitHub API poller bead lands), GhPr-gated tasks
    // remain pending — the no-op lookup reports Unknown and the gate
    // never self-satisfies.
    let store = Arc::new(InMemoryWorkflowTaskStore::new());

    let task = make_gh_pr_task("owner/repo#1");
    let task_id = task.id.to_string();
    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "sera".into(),
            resume_token: "tok-noop".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(resolved, 0, "no lookup wired → GhPr gate must stay pending");
    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Pending);
}
