//! Secrets vault endpoints.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use sera_db::secrets::SecretsRepository;

use crate::error::AppError;
use crate::state::AppState;

/// Secret metadata response (no value exposed).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretMetadataResponse {
    pub key: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub allowed_agents: Vec<String>,
    pub exposure: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Secret response with decrypted value.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretValueResponse {
    pub key: String,
    pub value: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub exposure: String,
}

/// GET /api/secrets
pub async fn list_secrets(
    State(state): State<AppState>,
) -> Result<Json<Vec<SecretMetadataResponse>>, AppError> {
    let rows = SecretsRepository::list(state.db.inner()).await?;
    let secrets: Vec<SecretMetadataResponse> = rows
        .into_iter()
        .map(|r| SecretMetadataResponse {
            key: r.name,
            description: r.description,
            tags: r.tags,
            allowed_agents: r.allowed_agents,
            exposure: r.exposure,
            created_at: r.created_at.map(|t| t.to_string()),
            updated_at: r.updated_at.map(|t| t.to_string()),
        })
        .collect();
    Ok(Json(secrets))
}

/// GET /api/secrets/:key
pub async fn get_secret(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<SecretValueResponse>, AppError> {
    let row = SecretsRepository::get_by_name(state.db.inner(), &key)
        .await?
        .ok_or_else(|| {
            AppError::Db(sera_db::DbError::NotFound {
                entity: "secret",
                key: "name",
                value: key.clone(),
            })
        })?;

    let encryption_key = &state.config.secrets_master_key;
    let value = SecretsRepository::decrypt(&row.encrypted_value, &row.iv, encryption_key)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?;

    Ok(Json(SecretValueResponse {
        key: row.name,
        value,
        description: row.description,
        tags: row.tags,
        exposure: row.exposure,
    }))
}

/// Request body for creating/updating a secret.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSecretRequest {
    pub key: String,
    pub value: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub allowed_agents: Option<Vec<String>>,
    pub exposure: Option<String>,
}

/// POST /api/secrets
pub async fn create_secret(
    State(state): State<AppState>,
    Json(body): Json<CreateSecretRequest>,
) -> Result<StatusCode, AppError> {
    let encryption_key = &state.config.secrets_master_key;
    let (encrypted_value, iv) = SecretsRepository::encrypt(&body.value, encryption_key)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?;

    let tags = body.tags.unwrap_or_default();
    let allowed_agents = body.allowed_agents.unwrap_or_default();
    let exposure = body.exposure.as_deref().unwrap_or("agent-env");

    SecretsRepository::upsert(
        state.db.inner(),
        &body.key,
        &encrypted_value,
        &iv,
        body.description.as_deref(),
        &tags,
        &allowed_agents,
        exposure,
        None, // created_by — would come from auth context
    )
    .await?;

    Ok(StatusCode::CREATED)
}

/// DELETE /api/secrets/:key
pub async fn delete_secret(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = SecretsRepository::delete(state.db.inner(), &key).await?;
    if !deleted {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "secret",
            key: "name",
            value: key,
        }));
    }
    Ok(Json(serde_json::json!({"message": "Secret deleted"})))
}
