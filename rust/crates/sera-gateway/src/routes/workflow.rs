//! Workflow task HTTP routes — Wave E Phase 1 (sera-kgi8) + Mail gate (sera-0zch).
//!
//! Routes:
//!   POST /api/workflow/tasks           — create a task (Timer and Mail gates)
//!   GET  /api/workflow/tasks           — list every known task
//!   GET  /api/workflow/tasks/{id}      — fetch a single task
//!   POST /api/workflow/mail/deliver    — deliver a mail event (in-memory; for tests)
//!
//! Timer and Mail are fully wired end-to-end. Other non-Timer `await_type`
//! values still return 501 Not Implemented — their wiring ships in follow-up
//! beads (Human: sera-dgk1, GhPr: sera-comg, GhRun: sera-4fel, Change: sera-7ggi).
#![allow(dead_code)]

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use sera_mail::InMemoryMailLookup;
use sera_workflow::task::{MailEvent, MailThreadId, WorkflowTaskInput};
use sera_workflow::{AwaitType, WorkflowTask, WorkflowTaskStatus, WorkflowTaskType};

use sera_gateway::workflow_store::{
    SchedulerTaskStatus, WorkflowTaskRecord, WorkflowTaskStore,
};

// ── Request / response shapes ────────────────────────────────────────────────

/// Discriminator for the await gate on the HTTP surface.
///
/// Mirrors [`AwaitType`] but decoupled so the HTTP payload does not require
/// callers to thread through e.g. GitHub repo metadata when all they want is
/// a Timer gate. Phase 1 only accepts `timer`; other variants return 501.
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
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
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

// ── AppState abstraction ─────────────────────────────────────────────────────

/// Abstraction over AppState for workflow handlers.
pub trait WorkflowAppState: Send + Sync + 'static {
    fn api_key(&self) -> &Option<String>;
    fn workflow_store(&self) -> Arc<dyn WorkflowTaskStore>;
    /// The in-memory mail lookup shared with the scheduler. Required for the
    /// `POST /api/workflow/mail/deliver` endpoint to inject test events without
    /// real SMTP/IMAP infrastructure.
    fn mail_lookup(&self) -> Arc<InMemoryMailLookup>;
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

    // Timer and Mail are wired end-to-end. Other variants are accepted at the
    // tag level but short-circuit with 501 — their gate wiring lands in
    // follow-up beads (Human: sera-dgk1, GhPr: sera-comg, GhRun: sera-4fel,
    // Change: sera-7ggi).
    let await_type = match body.await_type {
        AwaitTypeTag::Timer => {
            let deadline = body.deadline.ok_or(StatusCode::BAD_REQUEST)?;
            AwaitType::Timer { not_before: deadline }
        }
        AwaitTypeTag::Mail => {
            let thread_id = body.thread_id.ok_or(StatusCode::BAD_REQUEST)?;
            AwaitType::Mail { thread_id: MailThreadId::new(thread_id) }
        }
        _ => return Err(StatusCode::NOT_IMPLEMENTED),
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

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sera_gateway::workflow_store::InMemoryWorkflowTaskStore;
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
    }

    impl TestState {
        fn new(key: Option<&str>) -> Arc<Self> {
            Arc::new(Self {
                api_key: key.map(|k| k.to_owned()),
                store: Arc::new(InMemoryWorkflowTaskStore::new()),
                mail: Arc::new(InMemoryMailLookup::new()),
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
    async fn create_non_timer_returns_not_implemented() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "await_type": "human",
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
        // await_type must be Mail with the supplied thread_id.
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
        // First create a mail task so there's something to observe.
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

        // Deliver a reply_received event.
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

        // The lookup should now reflect the terminal event.
        use sera_workflow::MailLookup;
        use sera_workflow::task::MailThreadId;
        let event = state
            .mail
            .thread_event(&MailThreadId::new("<msg-003@example.com>"));
        assert_eq!(event, Some(MailEvent::ReplyReceived));
    }
}
