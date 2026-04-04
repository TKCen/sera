//! Stub endpoints — return 501 Not Implemented or empty data.
//! These exist to prevent 404s for known TS routes that haven't been
//! fully ported yet.
#![allow(dead_code, unused_imports)]

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use crate::error::AppError;
use crate::state::AppState;

fn not_impl() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({"error": "Not yet implemented in sera-core-rs"})),
    )
}

// ── Sandbox stubs ───────────────────────────────────────────────────────────

/// POST /api/sandbox/spawn
pub async fn sandbox_spawn() -> (StatusCode, Json<serde_json::Value>) {
    not_impl()
}

/// POST /api/sandbox/exec
pub async fn sandbox_exec() -> (StatusCode, Json<serde_json::Value>) {
    not_impl()
}

// ── Intercom stubs ──────────────────────────────────────────────────────────

/// POST /api/intercom/publish
pub async fn intercom_publish() -> (StatusCode, Json<serde_json::Value>) {
    not_impl()
}

/// POST /api/intercom/dm
pub async fn intercom_dm() -> (StatusCode, Json<serde_json::Value>) {
    not_impl()
}

// ── Pipeline stubs ──────────────────────────────────────────────────────────

/// POST /api/pipelines
pub async fn create_pipeline() -> (StatusCode, Json<serde_json::Value>) {
    not_impl()
}

/// GET /api/pipelines/:id
pub async fn get_pipeline(Path(_id): Path<String>) -> (StatusCode, Json<serde_json::Value>) {
    not_impl()
}

// ── Chat stubs (full impl is sera-runtime phase) ────────────────────────────

/// POST /api/chat
pub async fn chat() -> (StatusCode, Json<serde_json::Value>) {
    not_impl()
}

/// POST /v1/chat/completions (OpenAI compat)
pub async fn openai_chat_completions() -> (StatusCode, Json<serde_json::Value>) {
    not_impl()
}

// ── Embedding stubs ─────────────────────────────────────────────────────────

/// GET /api/embedding/config
pub async fn embedding_config() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "provider": "ollama",
        "model": "nomic-embed-text",
        "dimensions": 768,
        "status": "stub"
    }))
}

/// GET /api/embedding/status
pub async fn embedding_status() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "unavailable", "message": "Embedding service not yet ported to Rust"}))
}

// ── Knowledge stubs ─────────────────────────────────────────────────────────

/// GET /api/knowledge/circles/:id/history
pub async fn knowledge_history(Path(_id): Path<String>) -> Json<Vec<serde_json::Value>> {
    Json(vec![])
}

// ── Agent sub-route stubs ───────────────────────────────────────────────────

/// GET /api/agents/:id/logs
pub async fn agent_logs(Path(_id): Path<String>) -> Json<Vec<serde_json::Value>> {
    Json(vec![])
}

/// GET /api/agents/:id/subagents
pub async fn agent_subagents(Path(_id): Path<String>) -> Json<Vec<serde_json::Value>> {
    Json(vec![])
}

/// GET /api/agents/pending-updates
pub async fn pending_updates() -> Json<Vec<serde_json::Value>> {
    Json(vec![])
}

/// GET /api/tools — list executable tools
pub async fn list_tools() -> Json<Vec<serde_json::Value>> {
    Json(vec![])
}

/// GET /api/templates — delegates to agent templates
pub async fn list_templates(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let rows = sera_db::agents::AgentRepository::list_templates(state.db.inner()).await?;
    let templates: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "name": r.name,
                "displayName": r.display_name,
                "category": r.category,
            })
        })
        .collect();
    Ok(Json(templates))
}

/// GET /api/schedules/:id — get single schedule
pub async fn get_schedule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let row = sqlx::query_as::<_, sera_db::schedules::ScheduleRow>(
        "SELECT s.id, ai.name as agent_name, s.name, s.cron, s.expression, s.type, s.source,
                s.status, s.last_run_at, s.last_run_status, s.next_run_at, s.category, s.description
         FROM schedules s
         LEFT JOIN agent_instances ai ON s.agent_instance_id = ai.id
         WHERE s.id = $1::uuid",
    )
    .bind(&id)
    .fetch_optional(state.db.inner())
    .await
    .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    match row {
        Some(r) => Ok(Json(serde_json::json!({
            "id": r.id.to_string(),
            "agentName": r.agent_name,
            "name": r.name,
            "cron": r.cron,
            "expression": r.expression,
            "type": r.r#type,
            "source": r.source,
            "status": r.status,
            "category": r.category,
            "description": r.description,
        }))),
        None => Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "schedule",
            key: "id",
            value: id,
        })),
    }
}

/// GET /api/schedules/runs — schedule run history (stub)
pub async fn schedule_runs() -> Json<Vec<serde_json::Value>> {
    Json(vec![])
}

// ── Memory advanced stubs ───────────────────────────────────────────────────

/// GET /api/memory/overview
pub async fn memory_overview(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM core_memory_blocks")
        .fetch_one(state.db.inner())
        .await
        .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    Ok(Json(serde_json::json!({
        "totalBlocks": count.0,
    })))
}

/// GET /api/memory/:agentId/core
pub async fn agent_core_memory(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let rows = sera_db::memory::MemoryRepository::list_blocks(state.db.inner(), Some(&agent_id)).await?;
    let blocks: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id.to_string(),
                "name": r.name,
                "content": r.content,
                "characterLimit": r.character_limit,
                "isReadOnly": r.is_read_only,
            })
        })
        .collect();
    Ok(Json(blocks))
}

/// PUT /api/memory/:agentId/core/:name — update core memory block by name
pub async fn update_core_memory(
    State(state): State<AppState>,
    Path((agent_id, name)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let content = body.get("content").and_then(|v| v.as_str()).unwrap_or("");

    let result = sqlx::query(
        "UPDATE core_memory_blocks SET content = $1, updated_at = NOW()
         WHERE agent_instance_id = $2::uuid AND name = $3 AND is_read_only = false",
    )
    .bind(content)
    .bind(&agent_id)
    .bind(&name)
    .execute(state.db.inner())
    .await
    .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "core_memory_block",
            key: "name",
            value: name,
        }));
    }

    Ok(Json(serde_json::json!({"success": true})))
}

/// GET /api/memory/:agentId/blocks — list agent scoped blocks
pub async fn agent_scoped_blocks(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    // Same as core memory for now
    agent_core_memory(State(state), Path(agent_id)).await
}

/// DELETE /api/memory/:agentId/blocks/:id
pub async fn delete_agent_block(
    State(state): State<AppState>,
    Path((_agent_id, block_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = sera_db::memory::MemoryRepository::delete_block(state.db.inner(), &block_id).await?;
    if !deleted {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "memory_block",
            key: "id",
            value: block_id,
        }));
    }
    Ok(Json(serde_json::json!({"success": true})))
}
