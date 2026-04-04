//! Knowledge management endpoints — git history and merge requests.
#![allow(dead_code, unused_imports, clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeCommit {
    pub sha: String,
    pub message: String,
    pub author: String,
    pub timestamp: String,
    pub files_changed: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// GET /api/knowledge/circles/:id/history — git log for circle knowledge repo
pub async fn get_history(
    State(state): State<AppState>,
    Path(circle_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<KnowledgeCommit>>, AppError> {
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);

    // Query knowledge_commits table for this circle
    let rows = sqlx::query_as::<_, (String, String, String, time::OffsetDateTime, i32)>(
        "SELECT sha, message, author, created_at, files_changed
         FROM knowledge_commits
         WHERE circle_id = $1::uuid
         ORDER BY created_at DESC
         LIMIT $2 OFFSET $3",
    )
    .bind(&circle_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(state.db.inner())
    .await;

    // If table doesn't exist yet, return empty
    match rows {
        Ok(rows) => {
            let commits = rows
                .into_iter()
                .map(|(sha, message, author, ts, files)| KnowledgeCommit {
                    sha,
                    message,
                    author,
                    timestamp: ts.to_string(),
                    files_changed: files,
                })
                .collect();
            Ok(Json(commits))
        }
        Err(_) => Ok(Json(vec![])),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeRequest {
    pub id: String,
    pub circle_id: String,
    pub title: String,
    pub status: String, // "pending", "approved", "rejected"
    pub source_agent: String,
    pub created_at: String,
}

/// GET /api/knowledge/circles/:id/merge-requests — list pending merge requests
pub async fn list_merge_requests(
    State(state): State<AppState>,
    Path(circle_id): Path<String>,
) -> Result<Json<Vec<MergeRequest>>, AppError> {
    let rows = sqlx::query_as::<_, (Uuid, String, String, String, String, time::OffsetDateTime)>(
        "SELECT id, circle_id::text, title, status, source_agent, created_at
         FROM knowledge_merge_requests
         WHERE circle_id = $1::uuid
         ORDER BY created_at DESC",
    )
    .bind(&circle_id)
    .fetch_all(state.db.inner())
    .await;

    match rows {
        Ok(rows) => {
            let requests = rows
                .into_iter()
                .map(|(id, cid, title, status, agent, ts)| MergeRequest {
                    id: id.to_string(),
                    circle_id: cid,
                    title,
                    status,
                    source_agent: agent,
                    created_at: ts.to_string(),
                })
                .collect();
            Ok(Json(requests))
        }
        Err(_) => Ok(Json(vec![])),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMergeRequestBody {
    pub title: String,
    pub source_agent: String,
    pub changes: serde_json::Value,
}

/// POST /api/knowledge/circles/:id/merge-requests — create merge request
pub async fn create_merge_request(
    State(state): State<AppState>,
    Path(circle_id): Path<String>,
    Json(body): Json<CreateMergeRequestBody>,
) -> Result<(StatusCode, Json<MergeRequest>), AppError> {
    let id = Uuid::new_v4();
    let now = time::OffsetDateTime::now_utc();

    sqlx::query(
        "INSERT INTO knowledge_merge_requests (id, circle_id, title, status, source_agent, changes, created_at)
         VALUES ($1, $2::uuid, $3, 'pending', $4, $5, $6)",
    )
    .bind(id)
    .bind(&circle_id)
    .bind(&body.title)
    .bind(&body.source_agent)
    .bind(&body.changes)
    .bind(now)
    .execute(state.db.inner())
    .await
    .map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "Failed to create merge request: {e}"
        ))
    })?;

    Ok((
        StatusCode::CREATED,
        Json(MergeRequest {
            id: id.to_string(),
            circle_id,
            title: body.title,
            status: "pending".to_string(),
            source_agent: body.source_agent,
            created_at: now.to_string(),
        }),
    ))
}

/// POST /api/knowledge/circles/:id/merge-requests/:mrId/approve
pub async fn approve_merge_request(
    State(state): State<AppState>,
    Path((circle_id, mr_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = sqlx::query(
        "UPDATE knowledge_merge_requests SET status = 'approved', updated_at = NOW() WHERE id = $1::uuid AND circle_id = $2::uuid",
    )
    .bind(&mr_id)
    .bind(&circle_id)
    .execute(state.db.inner())
    .await
    .map_err(|e| {
        AppError::Internal(anyhow::anyhow!("Failed to approve: {e}"))
    })?;

    if result.rows_affected() == 0 {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "merge_request",
            key: "id",
            value: mr_id,
        }));
    }

    Ok(Json(serde_json::json!({"status": "approved"})))
}

/// POST /api/knowledge/circles/:id/merge-requests/:mrId/reject
pub async fn reject_merge_request(
    State(state): State<AppState>,
    Path((circle_id, mr_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = sqlx::query(
        "UPDATE knowledge_merge_requests SET status = 'rejected', updated_at = NOW() WHERE id = $1::uuid AND circle_id = $2::uuid",
    )
    .bind(&mr_id)
    .bind(&circle_id)
    .execute(state.db.inner())
    .await
    .map_err(|e| {
        AppError::Internal(anyhow::anyhow!("Failed to reject: {e}"))
    })?;

    if result.rows_affected() == 0 {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "merge_request",
            key: "id",
            value: mr_id,
        }));
    }

    Ok(Json(serde_json::json!({"status": "rejected"})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_commit_serializes() {
        let commit = KnowledgeCommit {
            sha: "abc123".to_string(),
            message: "Add knowledge".to_string(),
            author: "agent-1".to_string(),
            timestamp: "2026-04-04T00:00:00Z".to_string(),
            files_changed: 3,
        };
        let json = serde_json::to_string(&commit).unwrap();
        assert!(json.contains("abc123"));
        assert!(json.contains("filesChanged"));
    }

    #[test]
    fn merge_request_has_required_fields() {
        let mr = MergeRequest {
            id: "mr-123".to_string(),
            circle_id: "circle-1".to_string(),
            title: "Add to knowledge base".to_string(),
            status: "pending".to_string(),
            source_agent: "agent-1".to_string(),
            created_at: "2026-04-04T00:00:00Z".to_string(),
        };
        assert_eq!(mr.status, "pending");
        assert_eq!(mr.source_agent, "agent-1");
    }
}
