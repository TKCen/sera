//! Service identity endpoints for agent delegation.
#![allow(dead_code, unused_imports, clippy::type_complexity)]

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, error};

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceIdentity {
    pub id: String,
    pub agent_instance_id: String,
    pub service_name: String,
    pub key_id: String,
    pub status: String,
    pub created_at: String,
    pub rotated_at: Option<String>,
}

fn to_service_identity(
    id: uuid::Uuid,
    agent_instance_id: uuid::Uuid,
    service_name: String,
    key_id: String,
    status: String,
    created_at: time::OffsetDateTime,
    rotated_at: Option<time::OffsetDateTime>,
) -> ServiceIdentity {
    ServiceIdentity {
        id: id.to_string(),
        agent_instance_id: agent_instance_id.to_string(),
        service_name,
        key_id,
        status,
        created_at: super::iso8601(created_at),
        rotated_at: rotated_at.map(super::iso8601),
    }
}

/// GET /api/agents/:agentId/service-identities — list identities
pub async fn list_identities(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<Vec<ServiceIdentity>>, AppError> {
    let parsed_id =
        uuid::Uuid::parse_str(&agent_id).map_err(|_| AppError::Internal(anyhow::anyhow!("Invalid agent ID format")))?;

    let rows: Vec<(uuid::Uuid, uuid::Uuid, String, String, String, time::OffsetDateTime, Option<time::OffsetDateTime>)> = sqlx::query_as(
        "SELECT id, agent_instance_id, service_name, key_id, status, created_at, rotated_at
         FROM service_identities WHERE agent_instance_id = $1 ORDER BY created_at DESC"
    )
    .bind(parsed_id)
    .fetch_all(state.db.inner())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to list identities: {e}")))?;

    let identities = rows
        .into_iter()
        .map(|(id, aid, sname, kid, status, created, rotated)| {
            to_service_identity(id, aid, sname, kid, status, created, rotated)
        })
        .collect();

    Ok(Json(identities))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIdentityRequest {
    pub service_name: String,
}

/// POST /api/agents/:agentId/service-identities — create identity
pub async fn create_identity(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(body): Json<CreateIdentityRequest>,
) -> Result<(StatusCode, Json<ServiceIdentity>), AppError> {
    let parsed_id =
        uuid::Uuid::parse_str(&agent_id).map_err(|_| AppError::Internal(anyhow::anyhow!("Invalid agent ID format")))?;

    let id = uuid::Uuid::new_v4();
    let key_id = format!("sk-{}", &uuid::Uuid::new_v4().to_string()[..12]);
    let now = time::OffsetDateTime::now_utc();

    // Hash key using SHA-256 (never store raw key)
    let key_hash = format!("{:x}", Sha256::digest(key_id.as_bytes()));

    sqlx::query(
        "INSERT INTO service_identities (id, agent_instance_id, service_name, key_id, key_hash, status, created_at)
         VALUES ($1, $2, $3, $4, $5, 'active', $6)"
    )
    .bind(id)
    .bind(parsed_id)
    .bind(&body.service_name)
    .bind(&key_id)
    .bind(&key_hash)
    .bind(now)
    .execute(state.db.inner())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to create identity: {e}")))?;

    Ok((
        StatusCode::CREATED,
        Json(to_service_identity(
            id,
            parsed_id,
            body.service_name,
            key_id,
            "active".to_string(),
            now,
            None,
        )),
    ))
}

/// DELETE /api/agents/:agentId/service-identities/:identityId — delete identity
pub async fn delete_identity(
    State(state): State<AppState>,
    Path((agent_id, identity_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _parsed_agent_id = uuid::Uuid::parse_str(&agent_id)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Invalid agent ID format")))?;
    let parsed_identity_id = uuid::Uuid::parse_str(&identity_id)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Invalid identity ID format")))?;

    let result = sqlx::query("DELETE FROM service_identities WHERE id = $1")
        .bind(parsed_identity_id)
        .execute(state.db.inner())
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to delete identity: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "service_identity",
            key: "id",
            value: identity_id,
        }));
    }

    Ok(Json(serde_json::json!({"deleted": true})))
}

/// POST /api/agents/:agentId/service-identities/:identityId/rotate — rotate key
pub async fn rotate_key(
    State(state): State<AppState>,
    Path((agent_id, identity_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _parsed_agent_id = uuid::Uuid::parse_str(&agent_id)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Invalid agent ID format")))?;
    let parsed_identity_id = uuid::Uuid::parse_str(&identity_id)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Invalid identity ID format")))?;

    let new_key_id = format!("sk-{}", &uuid::Uuid::new_v4().to_string()[..12]);
    let new_key_hash = format!("{:x}", Sha256::digest(new_key_id.as_bytes()));
    let now = time::OffsetDateTime::now_utc();

    let result = sqlx::query(
        "UPDATE service_identities SET key_id = $1, key_hash = $2, rotated_at = $3
         WHERE id = $4"
    )
    .bind(&new_key_id)
    .bind(&new_key_hash)
    .bind(now)
    .bind(parsed_identity_id)
    .execute(state.db.inner())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to rotate key: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "service_identity",
            key: "id",
            value: identity_id,
        }));
    }

    // Log key rotation event for audit trail
    info!(
        agent_id = %_parsed_agent_id,
        service_identity_id = %parsed_identity_id,
        new_key_id = %new_key_id,
        rotated_at = %now,
        "Service identity key rotated"
    );

    Ok(Json(serde_json::json!({
        "id": parsed_identity_id.to_string(),
        "newKeyId": new_key_id,
        "rotatedAt": now.to_string(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_service_identity() {
        let now = time::OffsetDateTime::now_utc();
        let id = uuid::Uuid::new_v4();
        let agent_id = uuid::Uuid::new_v4();

        let identity = to_service_identity(
            id,
            agent_id,
            "test-service".to_string(),
            "sk-abc123".to_string(),
            "active".to_string(),
            now,
            None,
        );

        assert_eq!(identity.id, id.to_string());
        assert_eq!(identity.agent_instance_id, agent_id.to_string());
        assert_eq!(identity.service_name, "test-service");
        assert_eq!(identity.key_id, "sk-abc123");
        assert_eq!(identity.status, "active");
        assert!(identity.rotated_at.is_none());
    }
}
