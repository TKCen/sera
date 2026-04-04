//! Audit log endpoint.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    pub id: String,
    pub sequence: i64,
    pub timestamp: String,
    pub actor_type: String,
    pub actor_id: String,
    pub acting_context: Option<serde_json::Value>,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub hash: String,
}

/// GET /api/audit/log
pub async fn get_audit_log(
    State(state): State<AppState>,
    Query(params): Query<AuditLogQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
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
        .map(|r| {
            use super::iso8601;
            AuditEntryResponse {
                id: format!("audit-{}", r.sequence),
                sequence: r.sequence,
                timestamp: iso8601(r.timestamp),
                actor_type: r.actor_type,
                actor_id: r.actor_id,
                acting_context: r.acting_context,
                event_type: r.event_type,
                payload: r.payload,
                hash: r.hash,
            }
        })
        .collect();

    let total = AuditRepository::count_entries(
        state.db.inner(),
        params.actor_id.as_deref(),
        params.event_type.as_deref(),
    )
    .await
    .unwrap_or(entries.len() as i64);

    Ok(Json(serde_json::json!({
        "entries": entries,
        "total": total,
        "page": 1,
        "pageSize": limit,
    })))
}

/// Request body for appending an audit event.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendAuditRequest {
    pub actor_type: String,
    pub actor_id: String,
    pub acting_context: Option<serde_json::Value>,
    pub event_type: String,
    pub payload: serde_json::Value,
}

/// Audit append response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendAuditResponse {
    pub sequence: i64,
    pub hash: String,
}

/// POST /api/audit
pub async fn append_audit(
    State(state): State<AppState>,
    Json(body): Json<AppendAuditRequest>,
) -> Result<(StatusCode, Json<AppendAuditResponse>), AppError> {
    // Get the latest hash for chain continuation
    let prev = AuditRepository::get_latest(state.db.inner()).await?;
    let prev_hash = prev.as_ref().map(|r| r.hash.as_str());

    // Compute SHA-256 hash: prev_hash + actor_type + actor_id + event_type + payload
    let mut hasher = Sha256::new();
    if let Some(ph) = prev_hash {
        hasher.update(ph.as_bytes());
    }
    hasher.update(body.actor_type.as_bytes());
    hasher.update(body.actor_id.as_bytes());
    hasher.update(body.event_type.as_bytes());
    hasher.update(body.payload.to_string().as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    let sequence = AuditRepository::append(
        state.db.inner(),
        &body.actor_type,
        &body.actor_id,
        body.acting_context.as_ref(),
        &body.event_type,
        &body.payload,
        &hash,
        prev_hash,
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(AppendAuditResponse { sequence, hash }),
    ))
}
