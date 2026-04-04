//! Notification channel endpoints.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use sera_db::notifications::NotificationRepository;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelResponse {
    pub id: String,
    pub name: String,
    pub r#type: String,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub created_at: Option<String>,
    pub description: Option<String>,
}

/// GET /api/channels
pub async fn list_channels(
    State(state): State<AppState>,
) -> Result<Json<Vec<ChannelResponse>>, AppError> {
    let rows = NotificationRepository::list(state.db.inner()).await?;
    let channels: Vec<ChannelResponse> = rows
        .into_iter()
        .map(|r| ChannelResponse {
            id: r.id.to_string(),
            name: r.name,
            r#type: r.r#type,
            config: r.config,
            enabled: r.enabled,
            created_at: r.created_at.map(|t| t.to_string()),
            description: r.description,
        })
        .collect();
    Ok(Json(channels))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChannelRequest {
    pub name: String,
    pub r#type: String,
    pub config: serde_json::Value,
    pub description: Option<String>,
}

/// POST /api/channels
pub async fn create_channel(
    State(state): State<AppState>,
    Json(body): Json<CreateChannelRequest>,
) -> Result<(StatusCode, Json<ChannelResponse>), AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let row = NotificationRepository::create(
        state.db.inner(),
        &id,
        &body.name,
        &body.r#type,
        &body.config,
        body.description.as_deref(),
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(ChannelResponse {
            id: row.id.to_string(),
            name: row.name,
            r#type: row.r#type,
            config: row.config,
            enabled: row.enabled,
            created_at: row.created_at.map(|t| t.to_string()),
            description: row.description,
        }),
    ))
}

/// DELETE /api/channels/:id
pub async fn delete_channel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let deleted = NotificationRepository::delete(state.db.inner(), &id).await?;
    if !deleted {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "notification_channel",
            key: "id",
            value: id,
        }));
    }
    Ok(StatusCode::NO_CONTENT)
}
