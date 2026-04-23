//! Task queue endpoints.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use sera_db::tasks::TaskRepository;
use sera_gateway::envelope::{Op, Submission, W3cTraceContext};
use sera_gateway::session_store::SessionStore as _;

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
    pub retry_count: i32,
    pub max_retries: i32,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub exit_reason: Option<String>,
}

fn task_to_response(r: sera_db::tasks::TaskRow) -> TaskResponse {
    use super::iso8601;
    use super::iso8601_opt;
    TaskResponse {
        id: r.id.to_string(),
        agent_instance_id: r.agent_instance_id.to_string(),
        task: r.task,
        context: r.context,
        status: r.status,
        priority: r.priority,
        retry_count: r.retry_count,
        max_retries: r.max_retries,
        created_at: iso8601(r.created_at),
        started_at: iso8601_opt(r.started_at),
        completed_at: iso8601_opt(r.completed_at),
        result: r.result,
        error: r.error,
        exit_reason: r.exit_reason,
    }
}

#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

/// GET /api/agents/:id/tasks — list tasks (optionally filtered by status)
pub async fn list_tasks(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Query(params): Query<ListTasksQuery>,
) -> Result<Json<Vec<TaskResponse>>, AppError> {
    let limit = params.limit.unwrap_or(50).min(500);
    // If status=all or no status, get full history; otherwise filter
    let rows = if params.status.as_deref() == Some("all") || params.status.is_none() {
        TaskRepository::get_history(state.db.inner(), &agent_id, limit).await?
    } else {
        // Use history and filter in-memory (DB method doesn't support status filter yet)
        let all = TaskRepository::get_history(state.db.inner(), &agent_id, limit).await?;
        let status_filter = params.status.unwrap_or_default();
        all.into_iter().filter(|t| t.status == status_filter).collect()
    };
    Ok(Json(rows.into_iter().map(task_to_response).collect()))
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
    // Emit a Submission envelope before the DB write so this action is
    // auditable and replayable even if the write fails.
    //
    // Spec shape decision (bead sera-r1g8): task enqueue has no dedicated
    // Op variant yet. We use Op::UserTurn with a single Text item carrying
    // the task description — consistent with how chat messages are wrapped —
    // and set approval_policy to the agent id for correlation. A dedicated
    // Op::Task variant is deferred until the task-queue spec is finalised.
    let envelope = Submission {
        id: Uuid::new_v4(),
        op: Op::UserTurn {
            items: vec![sera_types::content_block::ContentBlock::Text {
                text: body.task.clone(),
            }],
            cwd: None,
            approval_policy: Some(agent_id.clone()),
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: body.context.clone(),
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
    };
    if let Err(e) = state
        .session_store
        .append_envelope(&agent_id, &envelope)
        .await
    {
        tracing::warn!(error = %e, agent_id = %agent_id, "session_store.append_envelope failed for enqueue_task; continuing");
    }

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
    Path((agent_id, task_id)): Path<(String, String)>,
    Json(body): Json<SubmitResultRequest>,
) -> Result<Json<TaskResponse>, AppError> {
    // Emit envelope before the DB write — task result submission is an
    // observable mutation (changes task status from running → completed/failed).
    //
    // Spec shape decision (bead sera-r1g8): same Op::UserTurn approach as
    // enqueue_task. The result payload is carried in final_output_schema so
    // replay tooling can reconstruct the outcome without a separate DB query.
    let envelope = Submission {
        id: Uuid::new_v4(),
        op: Op::UserTurn {
            items: vec![sera_types::content_block::ContentBlock::Text {
                text: format!("task_result:{task_id}"),
            }],
            cwd: None,
            approval_policy: Some(agent_id.clone()),
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: body.result.clone(),
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
    };
    if let Err(e) = state
        .session_store
        .append_envelope(&agent_id, &envelope)
        .await
    {
        tracing::warn!(error = %e, agent_id = %agent_id, task_id = %task_id, "session_store.append_envelope failed for submit_task_result; continuing");
    }

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

/// GET /api/agents/:id/tasks/:taskId — get a single task
pub async fn get_task(
    State(state): State<AppState>,
    Path((_agent_id, task_id)): Path<(String, String)>,
) -> Result<Json<TaskResponse>, AppError> {
    let row = TaskRepository::get_by_id(state.db.inner(), &task_id).await?;
    Ok(Json(task_to_response(row)))
}

/// DELETE /api/agents/:id/tasks/:taskId — cancel a queued task
pub async fn cancel_task(
    State(state): State<AppState>,
    Path((_agent_id, task_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    TaskRepository::cancel(state.db.inner(), &task_id).await?;
    Ok(Json(serde_json::json!({"success": true})))
}
