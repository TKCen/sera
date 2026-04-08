//! Auth endpoints — API key management and session info.

use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Argon2,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use sera_db::api_keys::ApiKeyRepository;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyResponse {
    pub id: String,
    pub name: String,
    pub roles: Vec<String>,
    pub created_at: Option<String>,
    pub expires_at: Option<String>,
    pub last_used_at: Option<String>,
}

/// GET /api/auth/me — return current operator info (from API key).
pub async fn get_me() -> Json<serde_json::Value> {
    // In the full impl, this would extract from JWT/auth context.
    // For now, return the bootstrap operator.
    Json(serde_json::json!({
        "sub": "bootstrap-operator",
        "roles": ["admin", "operator"],
        "authenticated": true
    }))
}

/// GET /api/auth/api-keys
pub async fn list_api_keys(
    State(state): State<AppState>,
) -> Result<Json<Vec<ApiKeyResponse>>, AppError> {
    let rows = ApiKeyRepository::list(state.db.inner(), None).await?;
    let keys: Vec<ApiKeyResponse> = rows
        .into_iter()
        .map(|r| ApiKeyResponse {
            id: r.id.to_string(),
            name: r.name,
            roles: r.roles,
            created_at: r.created_at.map(super::iso8601),
            expires_at: r.expires_at.map(super::iso8601),
            last_used_at: r.last_used_at.map(super::iso8601),
        })
        .collect();
    Ok(Json(keys))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub roles: Option<Vec<String>>,
}

/// POST /api/auth/api-keys
pub async fn create_api_key(
    State(state): State<AppState>,
    Json(body): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    // Generate a random key
    let raw_key = format!("sera_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));

    // Hash with Argon2 instead of Sha256 for hardening
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let key_hash = argon2
        .hash_password(raw_key.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Hashing failed: {e}")))?
        .to_string();

    let roles = body.roles.unwrap_or_else(|| vec!["operator".to_string()]);

    let row = ApiKeyRepository::create(
        state.db.inner(),
        &body.name,
        &key_hash,
        "bootstrap-operator",
        &roles,
    )
    .await?;

    // Return the raw key only on creation (it's hashed in DB)
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": row.id.to_string(),
            "name": row.name,
            "key": raw_key,
            "roles": row.roles,
        })),
    ))
}

/// DELETE /api/auth/api-keys/:id
pub async fn delete_api_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let revoked = ApiKeyRepository::revoke(state.db.inner(), &id).await?;
    if !revoked {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "api_key",
            key: "id",
            value: id,
        }));
    }
    Ok(Json(serde_json::json!({"success": true})))
}
