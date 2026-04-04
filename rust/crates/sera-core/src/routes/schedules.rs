//! Schedules endpoint.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sera_db::schedules::ScheduleRepository;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleResponse {
    pub id: String,
    pub agent_name: Option<String>,
    pub name: String,
    pub cron: Option<String>,
    pub expression: Option<String>,
    pub r#type: Option<String>,
    pub source: String,
    pub status: Option<String>,
    pub last_run_at: Option<String>,
    pub last_run_status: Option<String>,
    pub next_run_at: Option<String>,
    pub category: Option<String>,
    pub description: Option<String>,
}

/// GET /api/schedules
pub async fn list_schedules(
    State(state): State<AppState>,
) -> Result<Json<Vec<ScheduleResponse>>, AppError> {
    let rows = ScheduleRepository::list_schedules(state.db.inner()).await?;
    let schedules: Vec<ScheduleResponse> = rows
        .into_iter()
        .map(|r| ScheduleResponse {
            id: r.id.to_string(),
            agent_name: r.agent_name,
            name: r.name,
            cron: r.cron,
            expression: r.expression,
            r#type: r.r#type,
            source: r.source,
            status: r.status,
            last_run_at: r.last_run_at.map(|t| t.to_string()),
            last_run_status: r.last_run_status,
            next_run_at: r.next_run_at.map(|t| t.to_string()),
            category: r.category,
            description: r.description,
        })
        .collect();
    Ok(Json(schedules))
}

/// Request body for creating a schedule.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateScheduleRequest {
    pub agent_instance_id: Option<String>,
    pub agent_name: String,
    pub name: String,
    pub r#type: Option<String>,
    pub expression: String,
    pub task: Value,
    pub status: Option<String>,
    pub category: Option<String>,
    pub description: Option<String>,
}

/// POST /api/schedules
pub async fn create_schedule(
    State(state): State<AppState>,
    Json(body): Json<CreateScheduleRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let schedule_type = body.r#type.as_deref().unwrap_or("cron");
    let status = body.status.as_deref().unwrap_or("active");

    // Normalize task: wrap plain strings as {"prompt": "..."}
    let task = if body.task.is_string() {
        serde_json::json!({"prompt": body.task.as_str().unwrap_or("")})
    } else {
        body.task
    };

    ScheduleRepository::create_schedule(
        state.db.inner(),
        &id,
        body.agent_instance_id.as_deref(),
        &body.agent_name,
        &body.name,
        schedule_type,
        &body.expression,
        &task,
        "api",
        status,
        body.category.as_deref(),
        body.description.as_deref(),
    )
    .await?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({
        "id": id,
        "name": body.name,
        "status": status,
    }))))
}

/// Request body for updating a schedule.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateScheduleRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub expression: Option<String>,
    pub task: Option<Value>,
    pub status: Option<String>,
    pub category: Option<String>,
}

/// PATCH /api/schedules/:id
pub async fn update_schedule(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateScheduleRequest>,
) -> Result<Json<Value>, AppError> {
    ScheduleRepository::update_schedule(
        state.db.inner(),
        &id,
        body.name.as_deref(),
        body.description.as_deref(),
        body.expression.as_deref(),
        body.task.as_ref(),
        body.status.as_deref(),
        body.category.as_deref(),
    )
    .await?;

    Ok(Json(serde_json::json!({"success": true, "id": id})))
}

/// DELETE /api/schedules/:id
pub async fn delete_schedule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    // Check if manifest-sourced
    let source = ScheduleRepository::get_source(state.db.inner(), &id).await?;
    if source == "manifest" {
        return Err(AppError::Forbidden(
            "Manifest-declared schedules cannot be deleted via API".to_string(),
        ));
    }

    ScheduleRepository::delete_schedule(state.db.inner(), &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
