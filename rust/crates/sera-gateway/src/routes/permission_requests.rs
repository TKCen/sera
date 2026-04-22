//! Permission request endpoints for filesystem/network access grants.
#![allow(dead_code, unused_imports, clippy::type_complexity, clippy::too_many_arguments)]

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use sera_gateway::envelope::{Op, Submission, W3cTraceContext};
use sera_gateway::session_store::SessionStore as _;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequestResponse {
    pub id: String,
    pub agent_instance_id: String,
    pub permission_type: String,
    pub resource: String,
    pub access_level: String,
    pub justification: Option<String>,
    pub status: String,
    pub reviewed_by: Option<String>,
    pub reviewed_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePermissionRequestBody {
    pub agent_instance_id: String,
    pub permission_type: String,
    pub resource: String,
    pub access_level: String,
    pub justification: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPermissionRequestsQuery {
    pub status: Option<String>,
    pub agent_id: Option<String>,
}

fn to_permission_response(
    id: uuid::Uuid,
    agent_instance_id: uuid::Uuid,
    permission_type: String,
    resource: String,
    access_level: String,
    justification: Option<String>,
    status: String,
    reviewed_by: Option<String>,
    reviewed_at: Option<time::OffsetDateTime>,
    created_at: time::OffsetDateTime,
) -> PermissionRequestResponse {
    PermissionRequestResponse {
        id: id.to_string(),
        agent_instance_id: agent_instance_id.to_string(),
        permission_type,
        resource,
        access_level,
        justification,
        status,
        reviewed_by,
        reviewed_at: reviewed_at.map(super::iso8601),
        created_at: super::iso8601(created_at),
    }
}

/// POST /api/permission-requests — request permission grant
pub async fn create_request(
    State(state): State<AppState>,
    Json(body): Json<CreatePermissionRequestBody>,
) -> Result<(StatusCode, Json<PermissionRequestResponse>), AppError> {
    let id = uuid::Uuid::new_v4();
    let now = time::OffsetDateTime::now_utc();
    let agent_id = uuid::Uuid::parse_str(&body.agent_instance_id)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Invalid agent instance ID format")))?;

    // Emit envelope before the DB write — permission requests are observable
    // mutations (an agent requesting access to a resource).
    //
    // Spec shape decision (bead sera-r1g8): Op::ApprovalResponse is outbound
    // (operator → gateway). For an inbound permission *request* we use
    // Op::UserTurn with the resource path as the text item, matching the
    // "agent submitting a request" semantic. The HITL round-trip will produce
    // a follow-up Op::ApprovalResponse when the operator responds.
    let envelope = Submission {
        id: Uuid::new_v4(),
        op: Op::UserTurn {
            items: vec![sera_types::content_block::ContentBlock::Text {
                text: format!(
                    "permission_request:{}:{}:{}",
                    body.permission_type, body.resource, body.access_level
                ),
            }],
            cwd: None,
            approval_policy: Some(body.agent_instance_id.clone()),
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: None,
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
    };
    if let Err(e) = state
        .session_store
        .append_envelope(&body.agent_instance_id, &envelope)
        .await
    {
        tracing::warn!(
            error = %e,
            agent_instance_id = %body.agent_instance_id,
            "session_store.append_envelope failed for create_request; continuing"
        );
    }

    sqlx::query(
        "INSERT INTO permission_requests (id, agent_instance_id, permission_type, resource, access_level, justification, status, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7)"
    )
    .bind(id)
    .bind(agent_id)
    .bind(&body.permission_type)
    .bind(&body.resource)
    .bind(&body.access_level)
    .bind(&body.justification)
    .bind(now)
    .execute(state.db.inner())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to create permission request: {e}")))?;

    Ok((
        StatusCode::CREATED,
        Json(to_permission_response(
            id,
            agent_id,
            body.permission_type,
            body.resource,
            body.access_level,
            body.justification,
            "pending".to_string(),
            None,
            None,
            now,
        )),
    ))
}

/// GET /api/permission-requests — list permission requests
/// Note: The `permission_requests` table may not exist in all deployments.
/// Falls back to returning an empty array if the table is missing.
pub async fn list_requests(
    State(state): State<AppState>,
    Query(query): Query<ListPermissionRequestsQuery>,
) -> Result<Json<Vec<PermissionRequestResponse>>, AppError> {
    let status_filter = query.status.unwrap_or_else(|| "pending".to_string());

    let rows_result: Result<Vec<(uuid::Uuid, uuid::Uuid, String, String, String, Option<String>, String, Option<String>, Option<time::OffsetDateTime>, time::OffsetDateTime)>, _> =
        if let Some(agent_id) = &query.agent_id {
            let parsed_id = uuid::Uuid::parse_str(agent_id)
                .map_err(|_| AppError::Internal(anyhow::anyhow!("Invalid agent ID format")))?;
            sqlx::query_as(
                "SELECT id, agent_instance_id, permission_type, resource, access_level, justification, status, reviewed_by, reviewed_at, created_at
                 FROM permission_requests WHERE status = $1 AND agent_instance_id = $2 ORDER BY created_at DESC"
            )
            .bind(&status_filter)
            .bind(parsed_id)
            .fetch_all(state.db.inner())
            .await
        } else {
            sqlx::query_as(
                "SELECT id, agent_instance_id, permission_type, resource, access_level, justification, status, reviewed_by, reviewed_at, created_at
                 FROM permission_requests WHERE status = $1 ORDER BY created_at DESC"
            )
            .bind(&status_filter)
            .fetch_all(state.db.inner())
            .await
        };

    // If the table doesn't exist, return empty array instead of 500
    let rows = match rows_result {
        Ok(r) => r,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("does not exist") || msg.contains("permission_requests") {
                tracing::warn!("permission_requests table not found, returning empty list");
                return Ok(Json(vec![]));
            }
            return Err(AppError::Internal(anyhow::anyhow!("Failed to list permission requests: {e}")));
        }
    };

    let results: Vec<PermissionRequestResponse> = rows
        .into_iter()
        .map(|(id, agent_id, ptype, resource, access, justification, status, reviewed_by, reviewed_at, created_at)| {
            to_permission_response(
                id,
                agent_id,
                ptype,
                resource,
                access,
                justification,
                status,
                reviewed_by,
                reviewed_at,
                created_at,
            )
        })
        .collect();

    Ok(Json(results))
}

/// POST /api/permission-requests/:id/approve — approve request
pub async fn approve_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let parsed_id =
        uuid::Uuid::parse_str(&id).map_err(|_| AppError::Internal(anyhow::anyhow!("Invalid ID format")))?;

    let result = sqlx::query(
        "UPDATE permission_requests SET status = 'approved', reviewed_by = 'operator', reviewed_at = NOW()
         WHERE id = $1 AND status = 'pending'"
    )
    .bind(parsed_id)
    .execute(state.db.inner())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to approve: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "permission_request",
            key: "id",
            value: id,
        }));
    }

    Ok(Json(serde_json::json!({"status": "approved", "id": parsed_id.to_string()})))
}

/// POST /api/permission-requests/:id/deny — deny request
pub async fn deny_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let parsed_id =
        uuid::Uuid::parse_str(&id).map_err(|_| AppError::Internal(anyhow::anyhow!("Invalid ID format")))?;

    let result = sqlx::query(
        "UPDATE permission_requests SET status = 'denied', reviewed_by = 'operator', reviewed_at = NOW()
         WHERE id = $1 AND status = 'pending'"
    )
    .bind(parsed_id)
    .execute(state.db.inner())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to deny: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "permission_request",
            key: "id",
            value: id,
        }));
    }

    Ok(Json(serde_json::json!({"status": "denied", "id": parsed_id.to_string()})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_permission_response() {
        let now = time::OffsetDateTime::now_utc();
        let id = uuid::Uuid::new_v4();
        let agent_id = uuid::Uuid::new_v4();

        let response = to_permission_response(
            id,
            agent_id,
            "filesystem".to_string(),
            "/tmp".to_string(),
            "read".to_string(),
            Some("needed for config".to_string()),
            "pending".to_string(),
            None,
            None,
            now,
        );

        assert_eq!(response.id, id.to_string());
        assert_eq!(response.agent_instance_id, agent_id.to_string());
        assert_eq!(response.permission_type, "filesystem");
        assert_eq!(response.resource, "/tmp");
        assert_eq!(response.access_level, "read");
        assert_eq!(response.status, "pending");
        assert!(response.reviewed_by.is_none());
        assert!(response.reviewed_at.is_none());
    }
}
