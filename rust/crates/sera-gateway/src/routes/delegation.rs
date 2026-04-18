//! Delegation endpoints.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use sera_db::delegations::DelegationRepository;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct DelegationResponse {
    pub id: String,
    pub principal_type: String,
    pub principal_id: String,
    pub principal_name: String,
    pub actor_agent_id: String,
    pub scope: serde_json::Value,
    pub grant_type: String,
    pub issued_at: Option<String>,
    pub expires_at: Option<String>,
    pub use_count: Option<i32>,
}

fn to_response(r: sera_db::delegations::DelegationRow) -> DelegationResponse {
    use super::iso8601_opt;
    DelegationResponse {
        id: r.id.to_string(),
        principal_type: r.principal_type,
        principal_id: r.principal_id,
        principal_name: r.principal_name,
        actor_agent_id: r.actor_agent_id,
        scope: r.scope,
        grant_type: r.grant_type,
        issued_at: iso8601_opt(r.issued_at),
        expires_at: iso8601_opt(r.expires_at),
        use_count: r.use_count,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDelegationsQuery {
    pub agent_id: Option<String>,
}

/// GET /api/delegation
pub async fn list_delegations(
    State(state): State<AppState>,
    Query(params): Query<ListDelegationsQuery>,
) -> Result<Json<Vec<DelegationResponse>>, AppError> {
    let rows = DelegationRepository::list(state.db.inner(), params.agent_id.as_deref()).await?;
    Ok(Json(rows.into_iter().map(to_response).collect()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDelegationRequest {
    pub principal_type: String,
    pub principal_id: String,
    pub principal_name: String,
    pub actor_agent_id: String,
    pub scope: serde_json::Value,
    pub grant_type: String,
    pub credential_secret_name: String,
}

/// POST /api/delegation/issue
pub async fn issue_delegation(
    State(state): State<AppState>,
    Json(body): Json<IssueDelegationRequest>,
) -> Result<(StatusCode, Json<DelegationResponse>), AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let row = DelegationRepository::issue(
        state.db.inner(),
        &id,
        &body.principal_type,
        &body.principal_id,
        &body.principal_name,
        &body.actor_agent_id,
        &body.scope,
        &body.grant_type,
        &body.credential_secret_name,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(to_response(row))))
}

/// DELETE /api/delegation/:id
pub async fn revoke_delegation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let revoked = DelegationRepository::revoke(state.db.inner(), &id).await?;
    if !revoked {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "delegation",
            key: "id",
            value: id,
        }));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/delegation/:id/children
pub async fn get_delegation_children(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<DelegationResponse>>, AppError> {
    let rows = DelegationRepository::get_children(state.db.inner(), &id).await?;
    Ok(Json(rows.into_iter().map(to_response).collect()))
}

/// GET /api/agents/:agentId/delegations
pub async fn get_agent_delegations(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<Vec<DelegationResponse>>, AppError> {
    let rows = DelegationRepository::list(state.db.inner(), Some(&agent_id)).await?;
    Ok(Json(rows.into_iter().map(to_response).collect()))
}
