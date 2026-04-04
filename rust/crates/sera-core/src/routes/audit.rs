//! Audit log endpoint.

use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

use sera_db::audit::AuditRepository;

use crate::error::AppError;
use crate::state::AppState;

/// Query params for audit log.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogQuery {
    pub actor_id: Option<String>,
    pub event_type: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Audit entry response (camelCase for API compatibility).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntryResponse {
    pub sequence: i64,
    pub timestamp: String,
    pub actor_type: String,
    pub actor_id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub hash: String,
}

/// GET /api/audit/log
pub async fn get_audit_log(
    State(state): State<AppState>,
    Query(params): Query<AuditLogQuery>,
) -> Result<Json<Vec<AuditEntryResponse>>, AppError> {
    let limit = params.limit.unwrap_or(50).min(1000);
    let offset = params.offset.unwrap_or(0);

    let rows = AuditRepository::get_entries(
        state.db.inner(),
        params.actor_id.as_deref(),
        params.event_type.as_deref(),
        limit,
        offset,
    )
    .await?;

    let entries: Vec<AuditEntryResponse> = rows
        .into_iter()
        .map(|r| AuditEntryResponse {
            sequence: r.sequence,
            timestamp: r.timestamp.to_string(),
            actor_type: r.actor_type,
            actor_id: r.actor_id,
            event_type: r.event_type,
            payload: r.payload,
            hash: r.hash,
        })
        .collect();

    Ok(Json(entries))
}
