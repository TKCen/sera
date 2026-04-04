//! Task queue endpoints.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use sera_db::tasks::TaskRepository;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResponse {
    pub id: String,
    pub agent_instance_id: String,
    pub task: String,
    pub context: Option<serde_json::Value>,
    pub status: String,
    pub priority: i32,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub exit_reason: Option<String>,
}

fn task_to_response(r: sera_db::tasks::TaskRow) -> TaskResponse {
    TaskResponse {
        id: r.id.to_string(),
        agent_instance_id: r.agent_instance_id.to_string(),
        task: r.task,
        context: r.context,
        status: r.status,
        priority: r.priority,
        created_at: r.created_at.to_string(),
        started_at: r.started_at.map(|t| t.to_string()),
        completed_at: r.completed_at.map(|t| t.to_string()),
        result: r.result,
        error: r.error,
        exit_reason: r.exit_reason,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueTaskRequest {
    pub task: String,
    pub context: Option<serde_json::Value>,
    pub priority: Option<i32>,
}

/// POST /api/agents/:id/tasks
pub async fn enqueue_task(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(body): Json<EnqueueTaskRequest>,
) -> Result<(StatusCode, Json<TaskResponse>), AppError> {
    let row = TaskRepository::enqueue(
        state.db.inner(),
        &agent_id,
        &body.task,
        body.context.as_ref(),
        body.priority,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(task_to_response(row))))
}

/// GET /api/agents/:id/tasks/next
pub async fn poll_next_task(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let row = TaskRepository::poll_next(state.db.inner(), &agent_id).await?;
    match row {
        Some(task) => Ok(Json(serde_json::to_value(task_to_response(task)).unwrap())),
        None => Ok(Json(serde_json::json!(null))),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitResultRequest {
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub exit_reason: Option<String>,
}

/// POST /api/agents/:id/tasks/:taskId/result
pub async fn submit_task_result(
    State(state): State<AppState>,
    Path((_agent_id, task_id)): Path<(String, String)>,
    Json(body): Json<SubmitResultRequest>,
) -> Result<Json<TaskResponse>, AppError> {
    let row = TaskRepository::submit_result(
        state.db.inner(),
        &task_id,
        body.result.as_ref(),
        body.error.as_deref(),
        body.exit_reason.as_deref(),
    )
    .await?;
    Ok(Json(task_to_response(row)))
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<i64>,
}

/// GET /api/agents/:id/tasks/history
pub async fn get_task_history(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Query(params): Query<HistoryQuery>,
) -> Result<Json<Vec<TaskResponse>>, AppError> {
    let limit = params.limit.unwrap_or(50).min(500);
    let rows = TaskRepository::get_history(state.db.inner(), &agent_id, limit).await?;
    Ok(Json(rows.into_iter().map(task_to_response).collect()))
}
