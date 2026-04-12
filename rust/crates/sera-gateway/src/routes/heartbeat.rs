//! Heartbeat and lifecycle endpoints.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;

use sera_db::agents::AgentRepository;

use crate::error::AppError;
use crate::state::AppState;

/// POST /api/agents/:id/heartbeat
pub async fn heartbeat(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    sqlx::query("UPDATE agent_instances SET last_heartbeat_at = NOW() WHERE id::text = $1")
        .bind(&id)
        .execute(state.db.inner())
        .await
        .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    Ok(Json(serde_json::json!({"status": "ok"})))
}

/// Lifecycle status response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleResponse {
    pub id: String,
    pub name: String,
    pub status: String,
    pub lifecycle_mode: Option<String>,
    pub container_id: Option<String>,
    pub last_heartbeat_at: Option<String>,
}

/// GET /api/agents/:id/lifecycle
pub async fn get_lifecycle(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<LifecycleResponse>, AppError> {
    let row = AgentRepository::get_instance(state.db.inner(), &id).await?;
    Ok(Json(LifecycleResponse {
        id: row.id.to_string(),
        name: row.name,
        status: row.status.unwrap_or_else(|| "active".to_string()),
        lifecycle_mode: row.lifecycle_mode,
        container_id: row.container_id,
        last_heartbeat_at: super::iso8601_opt(row.last_heartbeat_at),
    }))
}
