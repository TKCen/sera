//! Permission request endpoints for filesystem/network access grants.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

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
        reviewed_at: reviewed_at.map(|t| t.to_string()),
        created_at: created_at.to_string(),
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
pub async fn list_requests(
    State(state): State<AppState>,
    Query(query): Query<ListPermissionRequestsQuery>,
) -> Result<Json<Vec<PermissionRequestResponse>>, AppError> {
    let status_filter = query.status.unwrap_or_else(|| "pending".to_string());

    let rows: Vec<(uuid::Uuid, uuid::Uuid, String, String, String, Option<String>, String, Option<String>, Option<time::OffsetDateTime>, time::OffsetDateTime)> =
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
        }
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to list permission requests: {e}")))?;

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
