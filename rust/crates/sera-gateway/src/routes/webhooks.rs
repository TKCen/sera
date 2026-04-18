//! Webhook endpoints.

use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use sera_db::webhooks::WebhookRepository;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookResponse {
    pub id: String,
    pub name: String,
    pub url_path: String,
    pub event_type: String,
    pub enabled: bool,
    pub created_at: String,
}

/// GET /api/webhooks
pub async fn list_webhooks(
    State(state): State<AppState>,
) -> Result<Json<Vec<WebhookResponse>>, AppError> {
    let rows = WebhookRepository::list(state.db.inner()).await?;
    let webhooks: Vec<WebhookResponse> = rows
        .into_iter()
        .map(|r| WebhookResponse {
            id: r.id.to_string(),
            name: r.name,
            url_path: r.url_path,
            event_type: r.event_type,
            enabled: r.enabled,
            created_at: super::iso8601(r.created_at),
        })
        .collect();
    Ok(Json(webhooks))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWebhookRequest {
    pub name: String,
    pub url_path: String,
    pub secret: String,
    pub event_type: String,
}

/// POST /api/webhooks
pub async fn create_webhook(
    State(state): State<AppState>,
    Json(body): Json<CreateWebhookRequest>,
) -> Result<(StatusCode, Json<WebhookResponse>), AppError> {
    let row = WebhookRepository::create(
        state.db.inner(),
        &body.name,
        &body.url_path,
        &body.secret,
        &body.event_type,
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(WebhookResponse {
            id: row.id.to_string(),
            name: row.name,
            url_path: row.url_path,
            event_type: row.event_type,
            enabled: row.enabled,
            created_at: super::iso8601(row.created_at),
        }),
    ))
}
