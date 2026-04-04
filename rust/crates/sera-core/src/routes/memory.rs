//! Memory blocks endpoints.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use sera_db::memory::MemoryRepository;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBlockResponse {
    pub id: String,
    pub agent_instance_id: String,
    pub name: String,
    pub content: String,
    pub character_limit: i32,
    pub is_read_only: bool,
    pub created_at: String,
    pub updated_at: String,
}

fn block_to_response(r: sera_db::memory::MemoryBlockRow) -> MemoryBlockResponse {
    MemoryBlockResponse {
        id: r.id.to_string(),
        agent_instance_id: r.agent_instance_id.to_string(),
        name: r.name,
        content: r.content,
        character_limit: r.character_limit,
        is_read_only: r.is_read_only,
        created_at: super::iso8601(r.created_at),
        updated_at: super::iso8601(r.updated_at),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlocksQuery {
    pub agent_instance_id: Option<String>,
}

/// GET /api/memory/blocks
pub async fn list_blocks(
    State(state): State<AppState>,
    Query(params): Query<BlocksQuery>,
) -> Result<Json<Vec<MemoryBlockResponse>>, AppError> {
    let rows =
        MemoryRepository::list_blocks(state.db.inner(), params.agent_instance_id.as_deref())
            .await?;
    Ok(Json(rows.into_iter().map(block_to_response).collect()))
}

/// GET /api/memory/entries/:id
pub async fn get_block(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<MemoryBlockResponse>, AppError> {
    let row = MemoryRepository::get_block(state.db.inner(), &id).await?;
    Ok(Json(block_to_response(row)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBlockRequest {
    pub agent_instance_id: String,
    pub name: String,
    pub content: Option<String>,
    pub character_limit: Option<i32>,
    #[serde(default)]
    pub is_read_only: bool,
}

/// POST /api/memory/blocks
pub async fn create_block(
    State(state): State<AppState>,
    Json(body): Json<CreateBlockRequest>,
) -> Result<(StatusCode, Json<MemoryBlockResponse>), AppError> {
    let row = MemoryRepository::create_block(
        state.db.inner(),
        &body.agent_instance_id,
        &body.name,
        body.content.as_deref().unwrap_or(""),
        body.character_limit,
        body.is_read_only,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(block_to_response(row))))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBlockRequest {
    pub content: String,
}

/// PUT /api/memory/entries/:id
pub async fn update_block(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateBlockRequest>,
) -> Result<Json<MemoryBlockResponse>, AppError> {
    let row = MemoryRepository::update_block(state.db.inner(), &id, &body.content).await?;
    Ok(Json(block_to_response(row)))
}

/// DELETE /api/memory/entries/:id
pub async fn delete_block(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = MemoryRepository::delete_block(state.db.inner(), &id).await?;
    if !deleted {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "memory_block",
            key: "id",
            value: id,
        }));
    }
    Ok(Json(serde_json::json!({"success": true})))
}
