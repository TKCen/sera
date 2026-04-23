//! Memory blocks endpoints.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
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
    pub agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct SearchQuery {
    pub query: Option<String>,
    pub agent_id: Option<String>,
}

/// GET /api/memory/blocks — list memory blocks, optionally filtered by agent_id
pub async fn list_blocks(
    State(state): State<AppState>,
    Query(params): Query<BlocksQuery>,
) -> Result<Json<Vec<MemoryBlockResponse>>, AppError> {
    let rows =
        MemoryRepository::list_blocks(state.db.require_pg_pool(), params.agent_id.as_deref())
            .await?;
    Ok(Json(rows.into_iter().map(block_to_response).collect()))
}

/// GET /api/memory/blocks/{id}
pub async fn get_block(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<MemoryBlockResponse>, AppError> {
    let row = MemoryRepository::get_block(state.db.require_pg_pool(), &id).await?;
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

/// POST /api/memory/blocks — create a new memory block
pub async fn create_block(
    State(state): State<AppState>,
    Json(body): Json<CreateBlockRequest>,
) -> Result<(StatusCode, Json<MemoryBlockResponse>), AppError> {
    let row = MemoryRepository::create_block(
        state.db.require_pg_pool(),
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

/// PUT /api/memory/blocks/{id}
pub async fn update_block(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateBlockRequest>,
) -> Result<Json<MemoryBlockResponse>, AppError> {
    let row =
        MemoryRepository::update_block(state.db.require_pg_pool(), &id, &body.content).await?;
    Ok(Json(block_to_response(row)))
}

/// DELETE /api/memory/blocks/{id}
pub async fn delete_block(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let deleted = MemoryRepository::delete_block(state.db.require_pg_pool(), &id).await?;
    if !deleted {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "memory_block",
            key: "id",
            value: id,
        }));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
pub struct MemorySearchResult {
    pub results: Vec<serde_json::Value>,
}

/// POST /api/memory/search — search memory blocks (stub: returns empty results)
pub async fn search_memory(
    State(_state): State<AppState>,
    Query(_params): Query<SearchQuery>,
) -> Result<Json<MemorySearchResult>, AppError> {
    // Stub implementation: returns empty results
    Ok(Json(MemorySearchResult { results: vec![] }))
}

#[derive(Debug, Serialize)]
pub struct MemoryVersions {
    pub versions: Vec<serde_json::Value>,
}

/// GET /api/memory/versions/{agent_id} — get memory versions (stub)
pub async fn get_memory_versions(
    State(_state): State<AppState>,
    Path(_agent_id): Path<String>,
) -> Result<Json<MemoryVersions>, AppError> {
    // Stub implementation: returns empty versions list
    Ok(Json(MemoryVersions { versions: vec![] }))
}

#[derive(Debug, Serialize)]
pub struct SnapshotResponse {
    pub snapshot_id: String,
    pub created_at: String,
}

/// POST /api/memory/versions/{agent_id}/snapshot — create memory snapshot (stub)
pub async fn create_memory_snapshot(
    State(_state): State<AppState>,
    Path(_agent_id): Path<String>,
) -> Result<(StatusCode, Json<SnapshotResponse>), AppError> {
    // Stub implementation: returns a synthetic snapshot ID
    Ok((
        StatusCode::CREATED,
        Json(SnapshotResponse {
            snapshot_id: uuid::Uuid::new_v4().to_string(),
            created_at: super::iso8601(time::OffsetDateTime::now_utc()),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_block_response_serializes() {
        let block = MemoryBlockResponse {
            id: "block-1".to_string(),
            agent_instance_id: "agent-1".to_string(),
            name: "priority_context".to_string(),
            content: "Important information".to_string(),
            character_limit: 2000,
            is_read_only: false,
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["id"], "block-1");
        assert_eq!(json["agentInstanceId"], "agent-1");
        assert_eq!(json["characterLimit"], 2000);
    }

    #[test]
    fn create_block_request_deserializes() {
        let input = r#"{
            "agentInstanceId": "agent-123",
            "name": "working_notes",
            "content": "Some notes",
            "characterLimit": 5000,
            "isReadOnly": false
        }"#;

        let req: CreateBlockRequest = serde_json::from_str(input).unwrap();
        assert_eq!(req.agent_instance_id, "agent-123");
        assert_eq!(req.name, "working_notes");
        assert_eq!(req.character_limit, Some(5000));
        assert!(!req.is_read_only);
    }

    #[test]
    fn update_block_request_deserializes() {
        let input = r#"{
            "content": "Updated content"
        }"#;

        let req: UpdateBlockRequest = serde_json::from_str(input).unwrap();
        assert_eq!(req.content, "Updated content");
    }

    #[test]
    fn blocks_query_with_agent_id() {
        let input = r#"{"agentId": "agent-456"}"#;
        let query: BlocksQuery = serde_json::from_str(input).unwrap();
        assert_eq!(query.agent_id, Some("agent-456".to_string()));
    }

    #[test]
    fn memory_search_result_serializes() {
        let result = MemorySearchResult { results: vec![] };

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["results"], serde_json::json!([]));
    }

    #[test]
    fn memory_versions_serializes() {
        let versions = MemoryVersions { versions: vec![] };

        let json = serde_json::to_value(&versions).unwrap();
        assert_eq!(json["versions"], serde_json::json!([]));
    }
}
