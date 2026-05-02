//! End-to-end integration test for the Change workflow gate (sera-7ggi).
//!
//! Mirrors the Timer-gate test (workflow_timer.rs): exercises the full
//! scheduler loop without the 5s ticker. A Change task is created with an
//! `artifact_id` whose state is initially missing, [`tick`] is run to confirm
//! the task remains pending, the change-artifact state is updated to a
//! terminal value, and a second [`tick`] resolves the task.
//!
//! The scheduler consults `chrono::Utc::now`, so this test drives `tick`
//! directly to keep the round trip deterministic.

use std::sync::Arc;

use chrono::Utc;

use sera_gateway::scheduler::tick;
use sera_gateway::workflow_store::{
    ChangeArtifactStateStore, InMemoryChangeArtifactStateStore, InMemoryWorkflowTaskStore,
    SchedulerTaskStatus, WorkflowTaskRecord, WorkflowTaskStore,
};
use sera_mail::lookup::InMemoryMailLookup;
use sera_types::evolution::ChangeArtifactId;
use sera_workflow::task::{ChangeState, WorkflowTaskInput};
use sera_workflow::{AwaitType, WorkflowTask, WorkflowTaskStatus, WorkflowTaskType};

fn make_change_task(artifact_id: ChangeArtifactId) -> WorkflowTask {
    let now = Utc::now();
    let mut task = WorkflowTask::new(WorkflowTaskInput {
        title: "change gate exemplar".into(),
        description: "sera-7ggi".into(),
        acceptance_criteria: Vec::new(),
        status: WorkflowTaskStatus::Open,
        priority: 5,
        task_type: WorkflowTaskType::Meta,
        source_formula: None,
        source_location: None,
        created_at: now,
    });
    task.await_type = Some(AwaitType::Change { artifact_id });
    task
}

#[tokio::test]
async fn change_gate_resolves_when_state_becomes_terminal() {
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let change_store = Arc::new(InMemoryChangeArtifactStateStore::new());

    let artifact_id = ChangeArtifactId { hash: [0xab; 32] };
    let task = make_change_task(artifact_id);
    let task_id = task.id.to_string();

    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "sera".into(),
            resume_token: "tok-change".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    // First tick — artifact state unknown, must remain pending.
    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        None,
        Some(Arc::clone(&change_store) as Arc<dyn ChangeArtifactStateStore>),
        None,
    )
    .await;
    assert_eq!(resolved, 0, "unknown artifact state must not resolve");
    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Pending);

    // Push a terminal state via the store (in production this would land via
    // POST /api/workflow/tasks/{id}/resolve-change → ChangeArtifactStateStore).
    change_store.upsert(artifact_id, ChangeState::Applied).await;

    // Second tick — gate must now wake the task.
    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        None,
        Some(Arc::clone(&change_store) as Arc<dyn ChangeArtifactStateStore>),
        None,
    )
    .await;
    assert_eq!(resolved, 1, "applied artifact must resolve the gate");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
    assert!(rec.resolved_at.is_some());
}

#[tokio::test]
async fn change_gate_blocks_on_non_terminal_state() {
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let change_store = Arc::new(InMemoryChangeArtifactStateStore::new());

    let artifact_id = ChangeArtifactId { hash: [0xcd; 32] };
    let task = make_change_task(artifact_id);
    let task_id = task.id.to_string();

    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "sera".into(),
            resume_token: "tok-change-pending".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    // Approved is deliberately non-terminal in sera-workflow — apply step has
    // not run yet, so the gate must not wake.
    change_store.upsert(artifact_id, ChangeState::Approved).await;

    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        None,
        Some(Arc::clone(&change_store) as Arc<dyn ChangeArtifactStateStore>),
        None,
    )
    .await;
    assert_eq!(resolved, 0, "approved (non-terminal) artifact must not resolve");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Pending);
}

#[tokio::test]
async fn change_gate_resolves_on_rejected_too() {
    // Rejection is terminal — the task must wake so its handler can branch on
    // the denial. Mirrors the unit-test contract in sera-workflow.
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let change_store = Arc::new(InMemoryChangeArtifactStateStore::new());

    let artifact_id = ChangeArtifactId { hash: [0xef; 32] };
    let task = make_change_task(artifact_id);
    let task_id = task.id.to_string();

    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "sera".into(),
            resume_token: "tok-change-rejected".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    change_store.upsert(artifact_id, ChangeState::Rejected).await;

    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        None,
        Some(Arc::clone(&change_store) as Arc<dyn ChangeArtifactStateStore>),
        None,
    )
    .await;
    assert_eq!(resolved, 1, "rejected artifact must resolve the gate");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
}
