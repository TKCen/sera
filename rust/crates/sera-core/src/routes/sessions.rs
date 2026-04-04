//! Sessions endpoint.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
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

/// Message response for session detail.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageResponse {
    pub id: String,
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<serde_json::Value>,
    pub created_at: Option<String>,
}

/// Session detail response (with messages).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetailResponse {
    #[serde(flatten)]
    pub session: SessionResponse,
    pub messages: Vec<MessageResponse>,
}

/// GET /api/sessions/:id
pub async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SessionDetailResponse>, AppError> {
    let row = SessionRepository::get_by_id(state.db.inner(), &id).await?;
    let messages = SessionRepository::get_messages(state.db.inner(), &id).await?;

    let session = SessionResponse {
        id: row.id.to_string(),
        agent_name: row.agent_name,
        agent_instance_id: row.agent_instance_id.map(|id| id.to_string()),
        title: row.title,
        message_count: row.message_count,
        created_at: row.created_at.map(|t| t.to_string()),
        updated_at: row.updated_at.map(|t| t.to_string()),
    };

    let msgs: Vec<MessageResponse> = messages
        .into_iter()
        .map(|m| MessageResponse {
            id: m.id.to_string(),
            role: m.role,
            content: m.content,
            tool_calls: m.tool_calls,
            created_at: m.created_at.map(|t| t.to_string()),
        })
        .collect();

    Ok(Json(SessionDetailResponse {
        session,
        messages: msgs,
    }))
}

/// Request body for creating a session.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    pub agent_name: String,
    pub title: Option<String>,
}

/// POST /api/sessions
pub async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionResponse>), AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let row = SessionRepository::create(
        state.db.inner(),
        &id,
        &body.agent_name,
        body.title.as_deref(),
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(SessionResponse {
            id: row.id.to_string(),
            agent_name: row.agent_name,
            agent_instance_id: row.agent_instance_id.map(|id| id.to_string()),
            title: row.title,
            message_count: row.message_count,
            created_at: row.created_at.map(|t| t.to_string()),
            updated_at: row.updated_at.map(|t| t.to_string()),
        }),
    ))
}

/// Request body for updating a session.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSessionRequest {
    pub title: String,
}

/// PUT /api/sessions/:id
pub async fn update_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateSessionRequest>,
) -> Result<Json<SessionResponse>, AppError> {
    let row = SessionRepository::update_title(state.db.inner(), &id, &body.title).await?;
    Ok(Json(SessionResponse {
        id: row.id.to_string(),
        agent_name: row.agent_name,
        agent_instance_id: row.agent_instance_id.map(|id| id.to_string()),
        title: row.title,
        message_count: row.message_count,
        created_at: row.created_at.map(|t| t.to_string()),
        updated_at: row.updated_at.map(|t| t.to_string()),
    }))
}

/// DELETE /api/sessions/:id
pub async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = SessionRepository::delete(state.db.inner(), &id).await?;
    if !deleted {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "session",
            key: "id",
            value: id,
        }));
    }
    Ok(Json(serde_json::json!({"success": true})))
}
