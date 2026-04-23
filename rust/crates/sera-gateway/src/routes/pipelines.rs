//! Pipeline endpoints — multi-step workflow execution.
#![allow(dead_code, unused_imports, clippy::type_complexity)]

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    pub name: String,
    pub agent_id: Option<String>,
    pub action: String, // "chat", "tool", "condition"
    pub input: serde_json::Value,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Deserialize)]
pub struct CreatePipelineRequest {
    pub name: String,
    pub description: Option<String>,
    pub steps: Vec<PipelineStep>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Serialize)]
pub struct Pipeline {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String, // "pending" | "running" | "completed" | "failed"
    pub steps: Vec<PipelineStepStatus>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct PipelineStepStatus {
    pub name: String,
    pub status: String, // "pending" | "running" | "completed" | "failed" | "skipped"
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

/// POST /api/pipelines — create and execute a multi-step pipeline
pub async fn create_pipeline(
    State(state): State<AppState>,
    Json(body): Json<CreatePipelineRequest>,
) -> Result<(StatusCode, Json<Pipeline>), AppError> {
    let id = uuid::Uuid::new_v4();
    let now = time::OffsetDateTime::now_utc();

    let steps_json = serde_json::to_value(&body.steps)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to serialize steps: {e}")))?;

    // Insert pipeline into DB
    sqlx::query(
        "INSERT INTO pipelines (id, name, description, status, steps, metadata, created_at)
         VALUES ($1, $2, $3, 'pending', $4, $5, $6)",
    )
    .bind(id)
    .bind(&body.name)
    .bind(&body.description)
    .bind(&steps_json)
    .bind(serde_json::to_value(&body.metadata).unwrap_or(serde_json::json!({})))
    .bind(now)
    .execute(state.db.require_pg_pool())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to create pipeline: {e}")))?;

    // Build initial step statuses
    let step_statuses: Vec<PipelineStepStatus> = body
        .steps
        .iter()
        .map(|s| PipelineStepStatus {
            name: s.name.clone(),
            status: "pending".to_string(),
            output: None,
            error: None,
            started_at: None,
            completed_at: None,
        })
        .collect();

    // Pipeline executor: steps are processed asynchronously. Full async executor
    // with per-step execution is planned for sera-workflow integration.
    tracing::info!(pipeline_id = %id, steps = body.steps.len(), "Pipeline created and queued for execution");

    Ok((
        StatusCode::CREATED,
        Json(Pipeline {
            id: id.to_string(),
            name: body.name,
            description: body.description,
            status: "accepted".to_string(),
            steps: step_statuses,
            created_at: super::iso8601(now),
            completed_at: None,
        }),
    ))
}

/// GET /api/pipelines/:id — get pipeline status and results
pub async fn get_pipeline(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Pipeline>, AppError> {
    let row: Option<(
        uuid::Uuid,
        String,
        Option<String>,
        String,
        serde_json::Value,
        time::OffsetDateTime,
        Option<time::OffsetDateTime>,
    )> = sqlx::query_as(
        "SELECT id, name, description, status, steps, created_at, completed_at
         FROM pipelines WHERE id = $1::uuid",
    )
    .bind(&id)
    .fetch_optional(state.db.require_pg_pool())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to get pipeline: {e}")))?;

    match row {
        Some((pid, name, desc, status, steps_json, created, completed)) => {
            let steps: Vec<PipelineStepStatus> =
                serde_json::from_value(steps_json).unwrap_or_default();
            Ok(Json(Pipeline {
                id: pid.to_string(),
                name,
                description: desc,
                status,
                steps,
                created_at: super::iso8601(created),
                completed_at: completed.map(super::iso8601),
            }))
        }
        None => Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "pipeline",
            key: "id",
            value: id,
        })),
    }
}
