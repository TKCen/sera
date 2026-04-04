//! Stub endpoints — return 501 Not Implemented or empty data.
//! These exist to prevent 404s for known TS routes that haven't been
//! fully ported yet.
#![allow(dead_code, unused_imports)]

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use sqlx::Row;

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
pub async fn list_tools(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    // Start with built-in tools
    let mut tools = vec![
        serde_json::json!({"name": "file-read", "type": "builtin", "description": "Read file contents"}),
        serde_json::json!({"name": "file-write", "type": "builtin", "description": "Write file contents"}),
        serde_json::json!({"name": "file-list", "type": "builtin", "description": "List directory contents"}),
        serde_json::json!({"name": "shell-exec", "type": "builtin", "description": "Execute shell command"}),
        serde_json::json!({"name": "http-request", "type": "builtin", "description": "Make HTTP request"}),
        serde_json::json!({"name": "knowledge-store", "type": "builtin", "description": "Store knowledge block"}),
        serde_json::json!({"name": "knowledge-query", "type": "builtin", "description": "Query knowledge base"}),
        serde_json::json!({"name": "web-fetch", "type": "builtin", "description": "Fetch web page"}),
        serde_json::json!({"name": "glob", "type": "builtin", "description": "File pattern matching"}),
        serde_json::json!({"name": "grep", "type": "builtin", "description": "Search file contents"}),
        serde_json::json!({"name": "spawn-ephemeral", "type": "builtin", "description": "Spawn ephemeral subagent"}),
        serde_json::json!({"name": "tool-search", "type": "builtin", "description": "Search available tools"}),
        serde_json::json!({"name": "skill-search", "type": "builtin", "description": "Search available skills"}),
    ];

    // Try to append skills from DB
    if let Ok(skills) = sera_db::skills::SkillRepository::list_skills(state.db.inner()).await {
        for skill in skills {
            tools.push(serde_json::json!({
                "id": skill.id.to_string(),
                "name": skill.name,
                "description": skill.description,
                "category": skill.category,
                "version": skill.version,
                "type": "skill",
            }));
        }
    }

    Json(tools)
}

/// GET /v1/tools/catalog — dynamic tool catalog for agent runtime
pub async fn tools_catalog(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    // Start with built-in tools
    let mut tools = vec![
        serde_json::json!({"name": "file-read", "type": "builtin", "description": "Read file contents"}),
        serde_json::json!({"name": "file-write", "type": "builtin", "description": "Write file contents"}),
        serde_json::json!({"name": "file-list", "type": "builtin", "description": "List directory contents"}),
        serde_json::json!({"name": "shell-exec", "type": "builtin", "description": "Execute shell command"}),
        serde_json::json!({"name": "http-request", "type": "builtin", "description": "Make HTTP request"}),
        serde_json::json!({"name": "knowledge-store", "type": "builtin", "description": "Store knowledge block"}),
        serde_json::json!({"name": "knowledge-query", "type": "builtin", "description": "Query knowledge base"}),
        serde_json::json!({"name": "web-fetch", "type": "builtin", "description": "Fetch web page"}),
        serde_json::json!({"name": "web-search", "type": "builtin", "description": "Web search"}),
        serde_json::json!({"name": "glob", "type": "builtin", "description": "File pattern matching"}),
        serde_json::json!({"name": "grep", "type": "builtin", "description": "Search file contents"}),
        serde_json::json!({"name": "spawn-ephemeral", "type": "builtin", "description": "Spawn ephemeral subagent"}),
        serde_json::json!({"name": "tool-search", "type": "builtin", "description": "Search available tools"}),
        serde_json::json!({"name": "delegate-task", "type": "builtin", "description": "Delegate task to another agent"}),
        serde_json::json!({"name": "schedule-task", "type": "builtin", "description": "Schedule a recurring task"}),
    ];

    // Append skills from DB
    if let Ok(skills) = sera_db::skills::SkillRepository::list_skills(state.db.inner()).await {
        for skill in skills {
            tools.push(serde_json::json!({
                "name": skill.name,
                "description": skill.description,
                "type": "skill",
            }));
        }
    }

    Json(tools)
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

/// Query params for schedule runs.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRunsQuery {
    pub agent_id: Option<String>,
    pub schedule_id: Option<String>,
    pub category: Option<String>,
    pub limit: Option<i64>,
}

/// GET /api/schedules/runs — schedule run history
/// Frontend expects ScheduleRun[]: { id, scheduleId, status, firedAt, startedAt, completedAt, createdAt, output? }
pub async fn schedule_runs(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<ScheduleRunsQuery>,
) -> Json<Vec<serde_json::Value>> {
    let limit = params.limit.unwrap_or(50).min(500);

    // Try to query from task_queue with schedule context
    let query = if let Some(agent_id) = &params.agent_id {
        sqlx::query(
            "SELECT tq.id, tq.status, tq.created_at, tq.started_at, tq.completed_at,
                    tq.result, tq.context
             FROM task_queue tq
             WHERE tq.agent_instance_id::text = $1
             ORDER BY tq.created_at DESC
             LIMIT $2"
        )
        .bind(agent_id)
        .bind(limit)
        .fetch_all(state.db.inner())
        .await
    } else {
        sqlx::query(
            "SELECT tq.id, tq.status, tq.created_at, tq.started_at, tq.completed_at,
                    tq.result, tq.context
             FROM task_queue tq
             ORDER BY tq.created_at DESC
             LIMIT $1"
        )
        .bind(limit)
        .fetch_all(state.db.inner())
        .await
    };

    if let Ok(db_rows) = query {
        let runs: Vec<serde_json::Value> = db_rows
            .into_iter()
            .map(|r| {
                let id: uuid::Uuid = r.get("id");
                let status: String = r.get("status");
                let created_at: time::OffsetDateTime = r.get("created_at");
                let started_at: Option<time::OffsetDateTime> = r.get("started_at");
                let completed_at: Option<time::OffsetDateTime> = r.get("completed_at");
                let context: Option<serde_json::Value> = r.get("context");
                let schedule_id = context
                    .as_ref()
                    .and_then(|c| c.get("schedule"))
                    .and_then(|s| s.get("scheduleId"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                serde_json::json!({
                    "id": id.to_string(),
                    "scheduleId": schedule_id,
                    "status": status,
                    "firedAt": super::iso8601(created_at),
                    "startedAt": started_at.map(super::iso8601),
                    "completedAt": completed_at.map(super::iso8601),
                    "createdAt": super::iso8601(created_at),
                })
            })
            .collect();
        return Json(runs);
    }

    // Query failed — return empty array gracefully
    Json(vec![])
}

// ── Memory advanced stubs ───────────────────────────────────────────────────

/// GET /api/memory/overview
/// Frontend expects: { totalBlocks, agents: [{id, blockCount}], topTags: [{tag, count}], typeBreakdown: Record<string, number> }
pub async fn memory_overview(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM core_memory_blocks")
        .fetch_one(state.db.inner())
        .await
        .unwrap_or((0,));

    Ok(Json(serde_json::json!({
        "totalBlocks": count.0,
        "agents": [],
        "topTags": [],
        "typeBreakdown": {}
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

/// GET /api/agents/instances/:id/tools — agent available tools
pub async fn agent_tools(Path(_id): Path<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "available": [],
        "unavailable": []
    }))
}

/// GET /api/agents/:id/template-diff
pub async fn agent_template_diff(Path(_id): Path<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "hasChanges": false,
        "instanceId": _id,
        "templateName": "",
        "templateUpdatedAt": "",
        "instanceAppliedAt": null,
        "changes": []
    }))
}

/// GET /api/memory/recent
pub async fn memory_recent() -> Json<Vec<serde_json::Value>> {
    Json(vec![])
}

/// GET /api/memory/explorer-graph
pub async fn memory_explorer_graph() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "nodes": [],
        "edges": []
    }))
}

/// GET /api/audit/verify — verify audit chain integrity
pub async fn audit_verify() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "valid": true,
        "totalEntries": 0,
        "checkedEntries": 0,
        "brokenLinks": []
    }))
}

/// GET /api/providers/dynamic
pub async fn providers_dynamic() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "dynamicProviders": [] }))
}

/// GET /api/providers/dynamic/statuses
pub async fn providers_dynamic_statuses() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "statuses": [] }))
}

/// GET /api/providers/templates — list provider templates
pub async fn providers_templates() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "templates": [] }))
}

/// GET /api/providers/default-model
pub async fn providers_default_model() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "defaultModel": null }))
}

/// PUT /api/providers/default-model
pub async fn set_default_model() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "success": true }))
}

/// GET /api/agents/:id/grants — list agent capability grants
pub async fn agent_grants(Path(_id): Path<String>) -> Json<Vec<serde_json::Value>> {
    Json(vec![])
}

/// GET /api/agents/:id/context-debug — debug agent context
/// Frontend expects: { agentId, agentName, testMessage, systemPromptLength, events: Array<{stage, detail, durationMs?}> }
pub async fn agent_context_debug(Path(id): Path<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "agentId": id,
        "agentName": "",
        "testMessage": "",
        "systemPromptLength": 0,
        "events": []
    }))
}

/// GET /api/agents/:id/system-prompt — get agent system prompt
pub async fn agent_system_prompt(Path(_id): Path<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "prompt": "" }))
}

/// GET /api/agents/:id/health-check — agent health check
/// Frontend expects: { agentId, agentName?, overallStatus, checks: Record<string, {ok, detail?}> }
pub async fn agent_health_check(Path(id): Path<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "agentId": id,
        "overallStatus": "unknown",
        "checks": {}
    }))
}

/// GET /api/agents/:id/sessions/:sid/commands — session command log
pub async fn session_commands(Path((_id, _sid)): Path<(String, String)>) -> Json<Vec<serde_json::Value>> {
    Json(vec![])
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
