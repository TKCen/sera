//! End-to-end integration test for the Human gate (sera-dgk1).
//!
//! Exercises the full scheduler loop without the 5s ticker: a Human-gated
//! task is created with a pending approval, the approval is resolved via
//! [`HumanGateStore::set_ticket_status`], and a single [`tick`] call confirms
//! the scheduler transitions the task to Resolved.
//!
//! Also exercises the HTTP resume endpoint via axum's `oneshot` helper and
//! the background scheduler path (spawn_scheduler), mirroring
//! `workflow_timer.rs`.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::{get, post},
};
use tower::ServiceExt;

use sera_gateway::scheduler::{spawn_scheduler, tick};
use sera_gateway::workflow_store::{
    HumanGateStore, InMemoryHumanGateStore, InMemoryWorkflowTaskStore, SchedulerTaskStatus,
    WorkflowTaskRecord, WorkflowTaskStore,
};
use sera_hitl::{ApprovalId, TicketStatus};
use sera_workflow::task::WorkflowTaskInput;
use sera_workflow::{AwaitType, WorkflowTask, WorkflowTaskStatus, WorkflowTaskType};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_human_task(approval_id: &str) -> WorkflowTask {
    let now = chrono::Utc::now();
    let mut task = WorkflowTask::new(WorkflowTaskInput {
        title: "human gate exemplar".into(),
        description: "sera-dgk1".into(),
        acceptance_criteria: Vec::new(),
        status: WorkflowTaskStatus::Open,
        priority: 5,
        task_type: WorkflowTaskType::Meta,
        source_formula: None,
        source_location: None,
        created_at: now,
    });
    task.await_type = Some(AwaitType::Human {
        approval_id: ApprovalId::new(approval_id),
    });
    task
}

// ── Tick-level tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn human_gate_blocks_without_resolution() {
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let hitl = Arc::new(InMemoryHumanGateStore::new());

    let task = make_human_task("appr-block-001");
    let task_id = task.id.to_string();

    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "sera".into(),
            resume_token: "tok-block".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    // No ticket status recorded → gate must NOT resolve.
    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Some(Arc::clone(&hitl) as Arc<dyn HumanGateStore>),
    )
    .await;
    assert_eq!(resolved, 0, "human gate without resolution must not resolve");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Pending);
    assert!(rec.resolved_at.is_none());
}

#[tokio::test]
async fn human_gate_resolves_after_approval() {
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let hitl = Arc::new(InMemoryHumanGateStore::new());

    let task = make_human_task("appr-approve-001");
    let task_id = task.id.to_string();

    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "sera".into(),
            resume_token: "tok-approve".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    // Record an Approved status → the gate should resolve on the next tick.
    hitl.set_ticket_status(ApprovalId::new("appr-approve-001"), TicketStatus::Approved)
        .await;

    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Some(Arc::clone(&hitl) as Arc<dyn HumanGateStore>),
    )
    .await;
    assert_eq!(resolved, 1, "approved human gate must resolve on first tick");

    let rec = store.get(&task_id).await.expect("record is still in store");
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
    assert!(rec.resolved_at.is_some());
}

#[tokio::test]
async fn human_gate_resolves_on_rejection() {
    // Workflows must also wake on Rejected so the handler can branch.
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let hitl = Arc::new(InMemoryHumanGateStore::new());

    let task = make_human_task("appr-reject-001");
    let task_id = task.id.to_string();

    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "sera".into(),
            resume_token: "tok-reject".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    hitl.set_ticket_status(ApprovalId::new("appr-reject-001"), TicketStatus::Rejected)
        .await;

    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Some(Arc::clone(&hitl) as Arc<dyn HumanGateStore>),
    )
    .await;
    assert_eq!(resolved, 1, "rejected human gate must also resolve");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
}

#[tokio::test]
async fn human_gate_resolves_on_expiry() {
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let hitl = Arc::new(InMemoryHumanGateStore::new());

    let task = make_human_task("appr-expire-001");
    let task_id = task.id.to_string();

    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "sera".into(),
            resume_token: "tok-expire".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    hitl.set_ticket_status(ApprovalId::new("appr-expire-001"), TicketStatus::Expired)
        .await;

    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Some(Arc::clone(&hitl) as Arc<dyn HumanGateStore>),
    )
    .await;
    assert_eq!(resolved, 1, "expired human gate must also resolve");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
}

// ── Background scheduler test ─────────────────────────────────────────────────

#[tokio::test]
async fn scheduler_background_task_resolves_human_gate() {
    // Exercises spawn_scheduler end-to-end: start the background task, insert
    // a human-gated task, record the approval, and confirm resolution within
    // 10s (two TICK_INTERVAL cycles).
    let store = Arc::new(InMemoryWorkflowTaskStore::new());
    let hitl = Arc::new(InMemoryHumanGateStore::new());
    let shutting_down = Arc::new(AtomicBool::new(false));

    let handle = spawn_scheduler(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Some(Arc::clone(&hitl) as Arc<dyn HumanGateStore>),
        Arc::clone(&shutting_down),
    );

    let task = make_human_task("appr-bg-001");
    let task_id = task.id.to_string();
    store
        .insert(WorkflowTaskRecord {
            task,
            agent_id: "sera".into(),
            resume_token: "tok-bg-human".into(),
            status: SchedulerTaskStatus::Pending,
            resolved_at: None,
        })
        .await;

    // Record the approval *after* inserting the task — verify the scheduler
    // polls the HITL store on each tick rather than caching a stale snapshot.
    hitl.set_ticket_status(ApprovalId::new("appr-bg-001"), TicketStatus::Approved)
        .await;

    // Poll for resolution — allow up to 10s (two TICK_INTERVAL cycles).
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

    assert!(resolved, "background scheduler must resolve human-gated task after approval");
}

// ── HTTP resume endpoint test ─────────────────────────────────────────────────

// Minimal AppState for route-level tests. Uses the #[path] trick to import
// the route module the same way the binary does.
#[path = "../src/routes/workflow.rs"]
mod route_workflow;

use route_workflow::WorkflowAppState;
use sera_gateway::workflow_store::WorkflowTaskStore as WfStore;
use sera_gateway::workflow_store::HumanGateStore as HumanGateStoreTrait;

struct TestState {
    store: Arc<InMemoryWorkflowTaskStore>,
    hitl: Arc<InMemoryHumanGateStore>,
}

impl TestState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            store: Arc::new(InMemoryWorkflowTaskStore::new()),
            hitl: Arc::new(InMemoryHumanGateStore::new()),
        })
    }
}

impl WorkflowAppState for TestState {
    fn api_key(&self) -> &Option<String> {
        &None::<String>
    }
    fn workflow_store(&self) -> Arc<dyn WfStore> {
        Arc::clone(&self.store) as Arc<dyn WfStore>
    }
    fn human_gate_store(&self) -> Option<Arc<dyn HumanGateStoreTrait>> {
        Some(Arc::clone(&self.hitl) as Arc<dyn HumanGateStoreTrait>)
    }
}

fn test_router(state: Arc<TestState>) -> Router {
    Router::new()
        .route(
            "/api/workflow/tasks",
            post(route_workflow::create_task::<TestState>)
                .get(route_workflow::list_tasks::<TestState>),
        )
        .route(
            "/api/workflow/tasks/{id}",
            get(route_workflow::get_task::<TestState>),
        )
        .route(
            "/api/workflow/tasks/{id}/resume",
            post(route_workflow::resume_task::<TestState>),
        )
        .with_state(state)
}

#[tokio::test]
async fn http_create_human_task_and_resume_round_trip() {
    // Full HTTP round-trip: create → poll (pending) → resume → tick → resolved.
    let state = TestState::new();
    let app = test_router(Arc::clone(&state));

    // 1. Create a Human-gated task.
    let create_body = serde_json::json!({
        "await_type": "human",
        "agent_id": "sera",
        "resume_token": "tok-http-h1",
        "approval_id": "appr-http-001",
    });
    let resp = app
        .clone()
        .oneshot(
            Request::post("/api/workflow/tasks")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let created: route_workflow::WorkflowTaskView = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(created.status, SchedulerTaskStatus::Pending);

    // 2. Poll GET — should still be Pending (no tick yet).
    let resp = app
        .clone()
        .oneshot(
            Request::get(format!("/api/workflow/tasks/{}", created.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let view: route_workflow::WorkflowTaskView = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(view.status, SchedulerTaskStatus::Pending, "task must be pending before resume");

    // 3. Resume (approve) the task.
    let resume_body = serde_json::json!({ "ticket_status": "approved" });
    let resp = app
        .clone()
        .oneshot(
            Request::post(format!("/api/workflow/tasks/{}/resume", created.id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&resume_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "resume must return 200");

    // 4. Tick the scheduler — the Human gate should now resolve.
    let resolved = tick(
        Arc::clone(&state.store) as Arc<dyn WfStore>,
        Some(Arc::clone(&state.hitl) as Arc<dyn HumanGateStoreTrait>),
    )
    .await;
    assert_eq!(resolved, 1, "scheduler tick must resolve the human-gated task");

    // 5. Poll GET — task must now be Resolved.
    let resp = app
        .oneshot(
            Request::get(format!("/api/workflow/tasks/{}", created.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let view: route_workflow::WorkflowTaskView = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        view.status,
        SchedulerTaskStatus::Resolved,
        "task must be resolved after tick"
    );
    assert!(view.resolved_at.is_some());
}
