//! Agent and template read endpoints.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sera_db::agents::AgentRepository;

use crate::error::AppError;
use crate::state::AppState;

/// Query params for listing instances.
#[derive(Debug, Deserialize)]
pub struct ListInstancesQuery {
    pub status: Option<String>,
}

/// Template response (camelCase for API compatibility).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateResponse {
    pub name: String,
    pub display_name: Option<String>,
    pub builtin: bool,
    pub category: Option<String>,
    pub spec: Value,
}

/// Instance response (camelCase for API compatibility).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceResponse {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub template_ref: String,
    pub circle: Option<String>,
    pub status: String,
    pub lifecycle_mode: Option<String>,
    pub parent_instance_id: Option<String>,
    pub workspace_path: Option<String>,
    pub container_id: Option<String>,
    pub last_heartbeat_at: Option<String>,
    pub updated_at: Option<String>,
    pub created_at: Option<String>,
}

/// GET /api/agents/templates
pub async fn list_templates(
    State(state): State<AppState>,
) -> Result<Json<Vec<TemplateResponse>>, AppError> {
    let rows = AgentRepository::list_templates(state.db.inner()).await?;
    let templates: Vec<TemplateResponse> = rows
        .into_iter()
        .map(|r| TemplateResponse {
            name: r.name,
            display_name: r.display_name,
            builtin: r.builtin,
            category: r.category,
            spec: r.spec,
        })
        .collect();
    Ok(Json(templates))
}

/// GET /api/agents
pub async fn list_instances(
    State(state): State<AppState>,
    Query(params): Query<ListInstancesQuery>,
) -> Result<Json<Vec<InstanceResponse>>, AppError> {
    let rows =
        AgentRepository::list_instances(state.db.inner(), params.status.as_deref()).await?;
    let instances: Vec<InstanceResponse> = rows.into_iter().map(instance_to_response).collect();
    Ok(Json(instances))
}

/// GET /api/agents/:id
pub async fn get_instance(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<InstanceResponse>, AppError> {
    let row = AgentRepository::get_instance(state.db.inner(), &id).await?;
    Ok(Json(instance_to_response(row)))
}

fn instance_to_response(r: sera_db::agents::InstanceRow) -> InstanceResponse {
    InstanceResponse {
        id: r.id.to_string(),
        name: r.name,
        display_name: r.display_name,
        template_ref: r.template_ref.unwrap_or(r.template_name),
        circle: r.circle,
        status: r.status.unwrap_or_else(|| "active".to_string()),
        lifecycle_mode: r.lifecycle_mode,
        parent_instance_id: r.parent_instance_id.map(|id| id.to_string()),
        workspace_path: Some(r.workspace_path),
        container_id: r.container_id,
        last_heartbeat_at: r.last_heartbeat_at.map(|t| t.to_string()),
        updated_at: r.updated_at.map(|t| t.to_string()),
        created_at: r.created_at.map(|t| t.to_string()),
    }
}
