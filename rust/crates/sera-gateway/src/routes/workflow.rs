//! Workflow task HTTP routes — Wave E Phase 1 (sera-kgi8) + Mail gate (sera-0zch) + GhRun gate (sera-4fel) + Change gate (sera-7ggi) + Human gate (sera-dgk1) + GhPr gate (sera-comg).
//!
//! Routes:
//!   POST /api/workflow/tasks                      — create a task (Timer, Mail, GhRun, Change, and Human gates)
//!   GET  /api/workflow/tasks                      — list every known task
//!   GET  /api/workflow/tasks/{id}                 — fetch a single task
//!   POST /api/workflow/mail/deliver               — deliver a mail event (in-memory; for tests)
//!   POST /api/workflow/tasks/{id}/resolve-change  — push a ChangeState event
//!   POST /api/workflow/tasks/{id}/resume          — resolve a Human-gated task
//!
//! Timer, Mail, GhRun, Change, and Human are fully wired end-to-end. GhPr creation
//! returns 501 until the GitHub API poller ships (sera-comg follow-up). Other non-Timer
//! `await_type` values also return 501 Not Implemented.
#![allow(dead_code)]

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use sera_hitl::{ApprovalId, TicketStatus};
use sera_mail::InMemoryMailLookup;
use sera_types::evolution::ChangeArtifactId;
use sera_workflow::task::{ChangeState, GhRunId, MailEvent, MailThreadId, WorkflowTaskInput};
use sera_workflow::{AwaitType, WorkflowTask, WorkflowTaskStatus, WorkflowTaskType};

use sera_gateway::workflow_store::{
    ChangeArtifactStateStore, HumanGateStore, SchedulerTaskStatus, WorkflowTaskRecord,
    WorkflowTaskStore,
};

// ── Request / response shapes ────────────────────────────────────────────────

/// Discriminator for the await gate on the HTTP surface.
///
/// Mirrors [`AwaitType`] but decoupled so the HTTP payload does not require
/// callers to thread through e.g. GitHub repo metadata when all they want is
/// a Timer gate. Timer, Mail, GhRun, Change, and Human are wired end-to-end; GhPr returns 501; other variants return 501.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwaitTypeTag {
    Timer,
    Human,
    GhRun,
    GhPr,
    Change,
    Mail,
}

/// Create-task request body.
///
/// - `timer` gate: supply `deadline`.
/// - `mail` gate: supply `thread_id` (opaque string identifying the email thread).
/// - `gh_run` gate: supply `run_id` and `repo` (in `owner/name` form).
/// - `change` gate: supply `artifact_id` (64-char hex SHA-256 of the change artifact).
/// - `human` gate: supply `approval_id`.
/// - `gh_pr` gate: supply `pr_id` (and optional `repo` for operator debugging). Returns 501 until GitHub API poller ships.
/// - `title` / `description` are always optional.
#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub await_type: AwaitTypeTag,
    pub agent_id: String,
    pub resume_token: String,
    /// Required for `timer` await_type; ignored otherwise.
    #[serde(default)]
    pub deadline: Option<DateTime<Utc>>,
    /// Required for `mail` await_type; the opaque thread identifier (e.g. RFC
    /// 2822 Message-ID) the scheduler will watch for a terminal [`MailEvent`].
    #[serde(default)]
    pub thread_id: Option<String>,
    /// Required for `gh_run` await_type; ignored otherwise.
    /// The numeric or opaque run identifier as a string.
    #[serde(default)]
    pub run_id: Option<String>,
    /// Required for `gh_run` await_type; ignored otherwise. Also optional for `gh_pr`.
    #[serde(default)]
    pub repo: Option<String>,
    /// Required for `change` await_type: 64-char hex SHA-256 of the change
    /// artifact. Ignored for other await types.
    #[serde(default)]
    pub artifact_id: Option<String>,
    /// Required for `human` await_type; ignored otherwise.
    #[serde(default)]
    pub approval_id: Option<String>,
    /// Required for `gh_pr` await_type: opaque pull-request identifier the
    /// scheduler keys on. Ignored for other await types.
    #[serde(default)]
    pub pr_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Request body for `POST /api/workflow/tasks/{id}/resolve-change`.
///
/// Pushes a [`ChangeState`] event for the artifact tracked by the identified
/// task. The scheduler picks it up on the next tick and marks the task
/// resolved if the state is terminal.
#[derive(Debug, Deserialize)]
pub struct ResolveChangeRequest {
    /// New state of the change artifact (e.g. `"applied"`, `"rejected"`).
    pub state: ChangeState,
}

/// Request body for `POST /api/workflow/tasks/{id}/resume`.
///
/// The caller supplies the terminal ticket status — the scheduler will pick
/// up the resolved Human gate on the next tick and mark the task resolved.
#[derive(Debug, Deserialize)]
pub struct ResumeTaskRequest {
    /// Terminal [`TicketStatus`] for the Human gate.
    /// Must be one of `"approved"`, `"rejected"`, or `"expired"`.
    pub ticket_status: TicketStatus,
}

/// JSON projection of a [`WorkflowTaskRecord`] returned by every route.
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowTaskView {
    pub id: String,
    pub agent_id: String,
    pub resume_token: String,
    pub status: SchedulerTaskStatus,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub await_type: Option<AwaitType>,
    pub title: String,
}

impl From<WorkflowTaskRecord> for WorkflowTaskView {
    fn from(rec: WorkflowTaskRecord) -> Self {
        Self {
            id: rec.task.id.to_string(),
            agent_id: rec.agent_id,
            resume_token: rec.resume_token,
            status: rec.status,
            resolved_at: rec.resolved_at,
            created_at: rec.task.created_at,
            await_type: rec.task.await_type.clone(),
            title: rec.task.title,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListTasksResponse {
    pub tasks: Vec<WorkflowTaskView>,
    pub count: usize,
}

// ── Auth ─────────────────────────────────────────────────────────────────────

fn check_auth(api_key: &Option<String>, headers: &HeaderMap) -> Result<(), StatusCode> {
    let expected = match api_key {
        None => return Ok(()),
        Some(k) => k,
    };
    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match provided {
        Some(k) if k == expected => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Parse a 64-character hex string into a [`ChangeArtifactId`].
fn parse_artifact_id(s: &str) -> Result<ChangeArtifactId, StatusCode> {
    let bytes = hex::decode(s).map_err(|_| StatusCode::BAD_REQUEST)?;
    let hash: [u8; 32] = bytes.try_into().map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(ChangeArtifactId { hash })
}

// ── AppState abstraction ─────────────────────────────────────────────────────

/// Abstraction over AppState for workflow handlers.
pub trait WorkflowAppState: Send + Sync + 'static {
    fn api_key(&self) -> &Option<String>;
    fn workflow_store(&self) -> Arc<dyn WorkflowTaskStore>;
    /// The in-memory mail lookup shared with the scheduler. Required for the
    /// `POST /api/workflow/mail/deliver` endpoint to inject test events without
    /// real SMTP/IMAP infrastructure.
    fn mail_lookup(&self) -> Arc<InMemoryMailLookup>;
    /// Returns the change-artifact state store backing the Change gate.
    /// Returning `None` means Change-gate wiring is not provisioned in this
    /// deployment — `create_task` for `await_type = "change"` returns 501 in
    /// that case, and `resolve-change` returns 501.
    fn change_artifact_store(&self) -> Option<Arc<dyn ChangeArtifactStateStore>>;
    /// Returns the human gate store backing the Human gate (sera-dgk1).
    /// Default returns `None` — deployments that don't wire the Human gate
    /// will return 501 from the resume endpoint and never resolve Human tasks.
    fn human_gate_store(&self) -> Option<Arc<dyn HumanGateStore>> {
        None
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// POST /api/workflow/tasks
pub async fn create_task<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Json(body): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<WorkflowTaskView>), StatusCode>
where
    S: WorkflowAppState,
{
    check_auth(state.api_key(), &headers)?;

    // Timer, Mail, GhRun, Change, and Human are wired end-to-end. GhPr creation
    // is blocked (returns 501) until the GitHub API poller is wired — starting
    // the scheduler with gh_pr_store=None would leave every GhPr-gated task
    // permanently pending (sera-comg review feedback).
    let await_type = match body.await_type {
        AwaitTypeTag::Timer => {
            let deadline = body.deadline.ok_or(StatusCode::BAD_REQUEST)?;
            AwaitType::Timer { not_before: deadline }
        }
        AwaitTypeTag::Mail => {
            let thread_id = body.thread_id.ok_or(StatusCode::BAD_REQUEST)?;
            AwaitType::Mail { thread_id: MailThreadId::new(thread_id) }
        }
        AwaitTypeTag::GhRun => {
            let run_id = body.run_id.ok_or(StatusCode::BAD_REQUEST)?;
            let repo = body.repo.ok_or(StatusCode::BAD_REQUEST)?;
            AwaitType::GhRun { run_id: GhRunId::new(run_id), repo }
        }
        AwaitTypeTag::Change => {
            // Refuse if the deployment hasn't wired a change-artifact store —
            // otherwise the task would block forever.
            if state.change_artifact_store().is_none() {
                return Err(StatusCode::NOT_IMPLEMENTED);
            }
            let raw = body.artifact_id.as_deref().ok_or(StatusCode::BAD_REQUEST)?;
            let artifact_id = parse_artifact_id(raw)?;
            AwaitType::Change { artifact_id }
        }
        AwaitTypeTag::Human => {
            let id_str = body.approval_id.ok_or(StatusCode::BAD_REQUEST)?;
            AwaitType::Human {
                approval_id: ApprovalId::new(id_str),
            }
        }
        // GhPr creation is blocked until the GitHub API poller is wired (sera-comg
        // follow-up). Scheduler starts with gh_pr_store=None so accepting the task
        // would leave it permanently pending with no path to resolution.
        AwaitTypeTag::GhPr => return Err(StatusCode::NOT_IMPLEMENTED),
    };

    let now = Utc::now();
    let title = body
        .title
        .unwrap_or_else(|| format!("await_{}", serde_json::to_string(&body.await_type).unwrap_or_default()));
    let description = body.description.unwrap_or_default();

    let mut task = WorkflowTask::new(WorkflowTaskInput {
        title,
        description,
        acceptance_criteria: Vec::new(),
        status: WorkflowTaskStatus::Open,
        priority: 5,
        task_type: WorkflowTaskType::Meta,
        source_formula: None,
        source_location: None,
        created_at: now,
    });
    task.await_type = Some(await_type);

    let record = WorkflowTaskRecord {
        task,
        agent_id: body.agent_id,
        resume_token: body.resume_token,
        status: SchedulerTaskStatus::Pending,
        resolved_at: None,
    };

    let stored = state.workflow_store().insert(record).await;
    Ok((StatusCode::CREATED, Json(stored.into())))
}

/// GET /api/workflow/tasks
pub async fn list_tasks<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
) -> Result<Json<ListTasksResponse>, StatusCode>
where
    S: WorkflowAppState,
{
    check_auth(state.api_key(), &headers)?;

    let records = state.workflow_store().list().await;
    let tasks: Vec<WorkflowTaskView> = records.into_iter().map(Into::into).collect();
    let count = tasks.len();
    Ok(Json(ListTasksResponse { tasks, count }))
}

/// GET /api/workflow/tasks/{id}
pub async fn get_task<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<WorkflowTaskView>, StatusCode>
where
    S: WorkflowAppState,
{
    check_auth(state.api_key(), &headers)?;

    let record = state
        .workflow_store()
        .get(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(record.into()))
}

// ── Mail deliver (in-memory, for tests) ──────────────────────────────────────

/// Request body for `POST /api/workflow/mail/deliver`.
#[derive(Debug, Deserialize)]
pub struct MailDeliverRequest {
    /// The thread id previously used when creating the Mail-gate task.
    pub thread_id: String,
    /// The event to inject: `"reply_received"` or `"closed"`.
    pub event: MailEvent,
}

/// `POST /api/workflow/mail/deliver` — inject a mail event into the in-memory
/// lookup so the scheduler resolves any pending Mail-gate task watching that
/// thread.
///
/// This endpoint is deliberately in-memory only: no real SMTP/IMAP involved.
/// Production delivery comes via `POST /api/mail/inbound` → correlator →
/// lookup. This route exists purely for test harnesses that need to drive the
/// full wait-then-resume cycle without running a mail server.
pub async fn deliver_mail<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Json(body): Json<MailDeliverRequest>,
) -> Result<StatusCode, StatusCode>
where
    S: WorkflowAppState,
{
    check_auth(state.api_key(), &headers)?;

    let thread_id = MailThreadId::new(body.thread_id);
    // gate_id is opaque for in-memory delivery; use a fixed sentinel.
    let gate_id = sera_mail::GateId::new("direct-deliver");
    state
        .mail_lookup()
        .notify(gate_id, &thread_id, body.event)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::NO_CONTENT)
}

// ── Change resolve ────────────────────────────────────────────────────────────

/// POST /api/workflow/tasks/{id}/resolve-change
///
/// Pushes a [`ChangeState`] event for the change artifact bound to the task
/// identified by `id`. The scheduler picks the new state up on the next tick
/// and marks the task resolved if the state is terminal.
///
/// 404 — task not found, or task is not bound to a Change gate.
/// 501 — change-artifact store not wired in this deployment.
pub async fn resolve_change<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ResolveChangeRequest>,
) -> Result<Json<WorkflowTaskView>, StatusCode>
where
    S: WorkflowAppState,
{
    check_auth(state.api_key(), &headers)?;

    let change_store = state
        .change_artifact_store()
        .ok_or(StatusCode::NOT_IMPLEMENTED)?;

    let record = state
        .workflow_store()
        .get(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    // Only meaningful for Change-gated tasks — refuse otherwise so callers do
    // not push state for the wrong artifact.
    let artifact_id = match &record.task.await_type {
        Some(AwaitType::Change { artifact_id }) => *artifact_id,
        _ => return Err(StatusCode::NOT_FOUND),
    };

    change_store.upsert(artifact_id, body.state).await;

    Ok(Json(record.into()))
}

/// POST /api/workflow/tasks/{id}/resume
///
/// Resolves a Human-gated task by recording a terminal [`TicketStatus`] for
/// its `approval_id`. The scheduler picks up the resolved gate on its next
/// tick and transitions the task from Pending → Resolved.
///
/// Returns 200 with the current task view on success.
/// Returns 404 when the task is not found.
/// Returns 409 when the task is not Human-gated.
/// Returns 422 when `ticket_status` is not terminal.
/// Returns 501 when the human gate store is not configured.
pub async fn resume_task<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ResumeTaskRequest>,
) -> Result<Json<WorkflowTaskView>, StatusCode>
where
    S: WorkflowAppState,
{
    check_auth(state.api_key(), &headers)?;

    let hitl_store = state.human_gate_store().ok_or(StatusCode::NOT_IMPLEMENTED)?;

    // Validate that the supplied status is terminal — resuming with a
    // non-terminal status would leave the gate in a non-ready state.
    if !body.ticket_status.is_terminal() {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let record = state
        .workflow_store()
        .get(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    // Extract the approval_id — only Human-gated tasks can be resumed here.
    let approval_id = match &record.task.await_type {
        Some(AwaitType::Human { approval_id }) => approval_id.clone(),
        _ => return Err(StatusCode::CONFLICT),
    };

    hitl_store
        .set_ticket_status(approval_id, body.ticket_status)
        .await;

    // Return the current (still Pending) view — the scheduler transitions it
    // to Resolved asynchronously on the next tick.
    Ok(Json(record.into()))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sera_gateway::workflow_store::{
        InMemoryChangeArtifactStateStore, InMemoryHumanGateStore, InMemoryWorkflowTaskStore,
    };
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
    };
    use tower::ServiceExt;

    struct TestState {
        api_key: Option<String>,
        store: Arc<InMemoryWorkflowTaskStore>,
        mail: Arc<InMemoryMailLookup>,
        change_store: Option<Arc<InMemoryChangeArtifactStateStore>>,
        hitl: Arc<InMemoryHumanGateStore>,
    }

    impl TestState {
        fn new(key: Option<&str>) -> Arc<Self> {
            Arc::new(Self {
                api_key: key.map(|k| k.to_owned()),
                store: Arc::new(InMemoryWorkflowTaskStore::new()),
                mail: Arc::new(InMemoryMailLookup::new()),
                change_store: Some(Arc::new(InMemoryChangeArtifactStateStore::new())),
                hitl: Arc::new(InMemoryHumanGateStore::new()),
            })
        }

        fn without_change_store(key: Option<&str>) -> Arc<Self> {
            Arc::new(Self {
                api_key: key.map(|k| k.to_owned()),
                store: Arc::new(InMemoryWorkflowTaskStore::new()),
                mail: Arc::new(InMemoryMailLookup::new()),
                change_store: None,
                hitl: Arc::new(InMemoryHumanGateStore::new()),
            })
        }
    }

    impl WorkflowAppState for TestState {
        fn api_key(&self) -> &Option<String> {
            &self.api_key
        }
        fn workflow_store(&self) -> Arc<dyn WorkflowTaskStore> {
            Arc::clone(&self.store) as Arc<dyn WorkflowTaskStore>
        }
        fn mail_lookup(&self) -> Arc<InMemoryMailLookup> {
            Arc::clone(&self.mail)
        }
        fn change_artifact_store(&self) -> Option<Arc<dyn ChangeArtifactStateStore>> {
            self.change_store
                .as_ref()
                .map(|s| Arc::clone(s) as Arc<dyn ChangeArtifactStateStore>)
        }
        fn human_gate_store(&self) -> Option<Arc<dyn HumanGateStore>> {
            Some(Arc::clone(&self.hitl) as Arc<dyn HumanGateStore>)
        }
    }

    fn test_router(state: Arc<TestState>) -> Router {
        Router::new()
            .route("/api/workflow/tasks", post(create_task::<TestState>))
            .route("/api/workflow/tasks", get(list_tasks::<TestState>))
            .route("/api/workflow/tasks/{id}", get(get_task::<TestState>))
            .route(
                "/api/workflow/mail/deliver",
                post(deliver_mail::<TestState>),
            )
            .route(
                "/api/workflow/tasks/{id}/resolve-change",
                post(resolve_change::<TestState>),
            )
            .route(
                "/api/workflow/tasks/{id}/resume",
                post(resume_task::<TestState>),
            )
            .with_state(state)
    }

    #[tokio::test]
    async fn create_timer_task_returns_created() {
        let app = test_router(TestState::new(None));
        let deadline = Utc::now() + chrono::Duration::seconds(60);
        let body = serde_json::json!({
            "await_type": "timer",
            "agent_id": "sera",
            "resume_token": "tok-1",
            "deadline": deadline,
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let view: WorkflowTaskView = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(view.agent_id, "sera");
        assert_eq!(view.resume_token, "tok-1");
        assert_eq!(view.status, SchedulerTaskStatus::Pending);
    }

    #[tokio::test]
    async fn create_timer_task_missing_deadline_is_bad_request() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "await_type": "timer",
            "agent_id": "sera",
            "resume_token": "tok-1"
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_gh_run_returns_created() {
        // GhRun gate is fully wired; POST /api/workflow/tasks with
        // await_type=gh_run + run_id + repo must return 201.
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "await_type": "gh_run",
            "agent_id": "sera",
            "resume_token": "tok-1",
            "run_id": "12345",
            "repo": "acme/my-repo",
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn create_gh_run_missing_run_id_is_bad_request() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "await_type": "gh_run",
            "agent_id": "sera",
            "resume_token": "tok-1",
            "repo": "acme/my-repo",
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_change_task_returns_created() {
        let app = test_router(TestState::new(None));
        let artifact_hex = "00".repeat(32);
        let body = serde_json::json!({
            "await_type": "change",
            "agent_id": "sera",
            "resume_token": "tok-c",
            "artifact_id": artifact_hex,
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn create_change_task_missing_artifact_id_is_bad_request() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "await_type": "change",
            "agent_id": "sera",
            "resume_token": "tok-c",
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_change_task_invalid_artifact_id_is_bad_request() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "await_type": "change",
            "agent_id": "sera",
            "resume_token": "tok-c",
            "artifact_id": "not-hex",
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_change_task_without_store_is_not_implemented() {
        // Deployments without a change-artifact store must refuse to create
        // Change-gated tasks rather than orphaning them.
        let app = test_router(TestState::without_change_store(None));
        let body = serde_json::json!({
            "await_type": "change",
            "agent_id": "sera",
            "resume_token": "tok-c",
            "artifact_id": "00".repeat(32),
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn create_human_task_returns_created() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "await_type": "human",
            "agent_id": "sera",
            "resume_token": "tok-h1",
            "approval_id": "appr-001",
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let view: WorkflowTaskView = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(view.status, SchedulerTaskStatus::Pending);
        assert!(matches!(view.await_type, Some(AwaitType::Human { .. })));
    }

    #[tokio::test]
    async fn create_human_task_missing_approval_id_is_bad_request() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "await_type": "human",
            "agent_id": "sera",
            "resume_token": "tok-h2",
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn resume_non_human_task_returns_conflict() {
        let state = TestState::new(None);
        let app = test_router(Arc::clone(&state));
        let deadline = Utc::now() + chrono::Duration::seconds(60);
        let create_body = serde_json::json!({
            "await_type": "timer",
            "agent_id": "sera",
            "resume_token": "tok-t1",
            "deadline": deadline,
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
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let view: WorkflowTaskView = serde_json::from_slice(&bytes).unwrap();
        let resume_body = serde_json::json!({ "ticket_status": "approved" });
        let resp = app
            .oneshot(
                Request::post(format!("/api/workflow/tasks/{}/resume", view.id))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&resume_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn resume_with_non_terminal_status_is_unprocessable() {
        let state = TestState::new(None);
        let app = test_router(Arc::clone(&state));
        let create_body = serde_json::json!({
            "await_type": "human",
            "agent_id": "sera",
            "resume_token": "tok-h3",
            "approval_id": "appr-002",
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
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let view: WorkflowTaskView = serde_json::from_slice(&bytes).unwrap();
        let resume_body = serde_json::json!({ "ticket_status": "pending" });
        let resp = app
            .oneshot(
                Request::post(format!("/api/workflow/tasks/{}/resume", view.id))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&resume_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn create_non_timer_non_human_returns_not_implemented() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "await_type": "gh_pr",
            "agent_id": "sera",
            "resume_token": "tok-1",
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn resolve_change_unknown_task_is_404() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({"state": "applied"});
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks/deadbeef/resolve-change")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn create_gh_pr_task_returns_not_implemented() {
        // sera-comg: GhPr creation returns 501 until the GitHub API poller is
        // wired. Scheduler starts with gh_pr_store=None so accepting the task
        // would leave it permanently pending with no path to resolution.
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "await_type": "gh_pr",
            "agent_id": "sera",
            "resume_token": "tok-pr",
            "pr_id": "owner/repo#42",
            "repo": "owner/repo",
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn create_gh_pr_task_missing_pr_id_is_not_implemented() {
        // sera-comg: GhPr creation returns 501 regardless of pr_id presence —
        // the gate is blocked until lookup wiring ships.
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "await_type": "gh_pr",
            "agent_id": "sera",
            "resume_token": "tok-pr",
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn list_tasks_empty() {
        let app = test_router(TestState::new(None));
        let resp = app
            .oneshot(
                Request::get("/api/workflow/tasks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: ListTasksResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(result.count, 0);
    }

    #[tokio::test]
    async fn get_unknown_task_is_404() {
        let app = test_router(TestState::new(None));
        let resp = app
            .oneshot(
                Request::get("/api/workflow/tasks/deadbeef")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn auth_denied_without_key() {
        let app = test_router(TestState::new(Some("secret")));
        let resp = app
            .oneshot(
                Request::get("/api/workflow/tasks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn create_mail_task_returns_created() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "await_type": "mail",
            "agent_id": "sera",
            "resume_token": "tok-mail-1",
            "thread_id": "<msg-001@example.com>",
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let view: WorkflowTaskView = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(view.agent_id, "sera");
        assert_eq!(view.resume_token, "tok-mail-1");
        assert_eq!(view.status, SchedulerTaskStatus::Pending);
        assert!(matches!(
            view.await_type,
            Some(AwaitType::Mail { .. })
        ));
    }

    #[tokio::test]
    async fn create_mail_task_missing_thread_id_is_bad_request() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "await_type": "mail",
            "agent_id": "sera",
            "resume_token": "tok-mail-2",
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn deliver_mail_returns_no_content() {
        let state = TestState::new(None);
        let app = test_router(Arc::clone(&state));
        let create_body = serde_json::json!({
            "await_type": "mail",
            "agent_id": "sera",
            "resume_token": "tok-mail-3",
            "thread_id": "<msg-003@example.com>",
        });
        let _ = app
            .clone()
            .oneshot(
                Request::post("/api/workflow/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let deliver_body = serde_json::json!({
            "thread_id": "<msg-003@example.com>",
            "event": "reply_received",
        });
        let resp = app
            .oneshot(
                Request::post("/api/workflow/mail/deliver")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&deliver_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        use sera_workflow::MailLookup;
        use sera_workflow::task::MailThreadId;
        let event = state
            .mail
            .thread_event(&MailThreadId::new("<msg-003@example.com>"));
        assert_eq!(event, Some(MailEvent::ReplyReceived));
    }
}
