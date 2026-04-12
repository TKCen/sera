//! Operator request endpoints.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

use sera_db::operator_requests::OperatorRequestRepository;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRequestResponse {
    pub id: String,
    pub agent_id: String,
    pub agent_name: Option<String>,
    pub r#type: String,
    pub title: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub response: Option<serde_json::Value>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

fn to_response(r: sera_db::operator_requests::OperatorRequestRow) -> OperatorRequestResponse {
    OperatorRequestResponse {
        id: r.id.to_string(),
        agent_id: r.agent_id,
        agent_name: r.agent_name,
        r#type: r.r#type,
        title: r.title,
        payload: r.payload,
        status: r.status,
        response: r.response,
        created_at: super::iso8601(r.created_at),
        resolved_at: r.resolved_at.map(super::iso8601),
    }
}

/// GET /api/operator-requests/pending/count
pub async fn pending_count(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let count = OperatorRequestRepository::count_pending(state.db.inner()).await?;
    Ok(Json(serde_json::json!({"count": count})))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRequestsQuery {
    pub status: Option<String>,
    pub agent_id: Option<String>,
    pub limit: Option<i64>,
}

/// GET /api/operator-requests
pub async fn list_requests(
    State(state): State<AppState>,
    Query(params): Query<ListRequestsQuery>,
) -> Result<Json<Vec<OperatorRequestResponse>>, AppError> {
    let limit = params.limit.unwrap_or(50).min(500);
    let rows = OperatorRequestRepository::list(
        state.db.inner(),
        params.status.as_deref(),
        params.agent_id.as_deref(),
        limit,
    )
    .await?;
    Ok(Json(rows.into_iter().map(to_response).collect()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RespondRequest {
    pub status: String,
    pub response: Option<serde_json::Value>,
}

/// POST /api/operator-requests/:id/respond
pub async fn respond_to_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RespondRequest>,
) -> Result<Json<OperatorRequestResponse>, AppError> {
    let row = OperatorRequestRepository::respond(
        state.db.inner(),
        &id,
        &body.status,
        body.response.as_ref(),
    )
    .await?;
    Ok(Json(to_response(row)))
}
