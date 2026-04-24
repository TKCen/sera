//! Workflow task HTTP routes — Wave E Phase 1 (sera-kgi8).
//!
//! Routes:
//!   POST /api/workflow/tasks      — create a task (Timer gate only in Phase 1)
//!   GET  /api/workflow/tasks      — list every known task
//!   GET  /api/workflow/tasks/{id} — fetch a single task
//!
//! Non-Timer `await_type` values are accepted by the schema but return
//! 501 Not Implemented — their wiring ships in follow-up beads (Human:
//! sera-dgk1, GhPr: sera-comg, GhRun: sera-4fel, Change: sera-7ggi,
//! Mail: sera-0zch).
#![allow(dead_code)]

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use sera_workflow::task::WorkflowTaskInput;
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
/// Phase 1 Timer-only path: pass `await_type = "timer"` and a `deadline`.
/// `title` / `description` are optional — defaults keep the synthetic
/// "wait for deadline" task compact when the caller doesn't care.
#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub await_type: AwaitTypeTag,
    pub agent_id: String,
    pub resume_token: String,
    /// Required for `timer` await_type; ignored otherwise.
    #[serde(default)]
    pub deadline: Option<DateTime<Utc>>,
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

    // Phase 1: only Timer is wired end-to-end. Other variants are accepted
    // at the tag level but short-circuit with 501 — their gate wiring lands
    // in follow-up beads (Human: sera-dgk1, GhPr: sera-comg, GhRun:
    // sera-4fel, Change: sera-7ggi, Mail: sera-0zch).
    let await_type = match body.await_type {
        AwaitTypeTag::Timer => {
            let deadline = body.deadline.ok_or(StatusCode::BAD_REQUEST)?;
            AwaitType::Timer { not_before: deadline }
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
    }

    impl TestState {
        fn new(key: Option<&str>) -> Arc<Self> {
            Arc::new(Self {
                api_key: key.map(|k| k.to_owned()),
                store: Arc::new(InMemoryWorkflowTaskStore::new()),
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
    }

    fn test_router(state: Arc<TestState>) -> Router {
        Router::new()
            .route("/api/workflow/tasks", post(create_task::<TestState>))
            .route("/api/workflow/tasks", get(list_tasks::<TestState>))
            .route("/api/workflow/tasks/{id}", get(get_task::<TestState>))
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
}
