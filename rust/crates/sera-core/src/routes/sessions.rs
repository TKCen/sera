//! Sessions endpoint.

use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

use sera_db::sessions::SessionRepository;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsQuery {
    pub agent_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResponse {
    pub id: String,
    pub agent_name: String,
    pub agent_instance_id: Option<String>,
    pub title: String,
    pub message_count: Option<i32>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// GET /api/sessions
pub async fn list_sessions(
    State(state): State<AppState>,
    Query(params): Query<SessionsQuery>,
) -> Result<Json<Vec<SessionResponse>>, AppError> {
    let rows =
        SessionRepository::list_sessions(state.db.inner(), params.agent_name.as_deref()).await?;
    let sessions: Vec<SessionResponse> = rows
        .into_iter()
        .map(|r| SessionResponse {
            id: r.id.to_string(),
            agent_name: r.agent_name,
            agent_instance_id: r.agent_instance_id.map(|id| id.to_string()),
            title: r.title,
            message_count: r.message_count,
            created_at: r.created_at.map(|t| t.to_string()),
            updated_at: r.updated_at.map(|t| t.to_string()),
        })
        .collect();
    Ok(Json(sessions))
}
