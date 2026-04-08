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
pub async fn pending_updates(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let rows = sqlx::query(
        "SELECT ai.id, ai.name, ai.template_ref, ai.updated_at, at.updated_at as template_updated_at
         FROM agent_instances ai
         JOIN agent_templates at ON ai.template_name = at.name
         WHERE at.updated_at > ai.updated_at OR at.updated_at IS NULL"
    )
    .fetch_all(state.db.inner())
    .await
    .unwrap_or_default();

    let updates: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let id: uuid::Uuid = r.get("id");
            let updated_at: Option<time::OffsetDateTime> = r.get("updated_at");
            let template_updated_at: Option<time::OffsetDateTime> = r.get("template_updated_at");
            serde_json::json!({
                "agentId": id.to_string(),
                "agentName": r.get::<String, _>("name"),
                "templateRef": r.get::<Option<String>, _>("template_ref"),
                "instanceUpdatedAt": updated_at.map(super::iso8601),
                "templateUpdatedAt": template_updated_at.map(super::iso8601)
            })
        })
        .collect();

    Ok(Json(updates))
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
    // Total block count
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM core_memory_blocks")
        .fetch_one(state.db.inner())
        .await
        .unwrap_or((0,));

    // Agents with block counts
    let agent_rows = sqlx::query(
        "SELECT agent_instance_id, COUNT(*) as block_count FROM core_memory_blocks GROUP BY agent_instance_id"
    )
    .fetch_all(state.db.inner())
    .await
    .unwrap_or_default();

    let agents: Vec<serde_json::Value> = agent_rows
        .iter()
        .map(|r| {
            let agent_id: uuid::Uuid = r.get("agent_instance_id");
            let block_count: i64 = r.get("block_count");
            serde_json::json!({
                "id": agent_id.to_string(),
                "blockCount": block_count
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "totalBlocks": count.0,
        "agents": agents,
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
pub async fn agent_tools(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Get agent instance to find resolved capabilities
    let agent = sera_db::agents::AgentRepository::get_instance(state.db.inner(), &id).await?;

    let available = agent
        .resolved_capabilities
        .as_ref()
        .and_then(|caps| caps.get("tools"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(Json(serde_json::json!({
        "available": available,
        "unavailable": []
    })))
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
pub async fn memory_recent(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let rows = sqlx::query(
        "SELECT id, agent_instance_id, name, content, character_limit, is_read_only, created_at, updated_at
         FROM core_memory_blocks ORDER BY updated_at DESC LIMIT 20"
    )
    .fetch_all(state.db.inner())
    .await
    .unwrap_or_default();

    let blocks: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let id: uuid::Uuid = r.get("id");
            let agent_id: uuid::Uuid = r.get("agent_instance_id");
            let updated_at: time::OffsetDateTime = r.get("updated_at");
            serde_json::json!({
                "id": id.to_string(),
                "agentInstanceId": agent_id.to_string(),
                "name": r.get::<String, _>("name"),
                "content": r.get::<String, _>("content"),
                "characterLimit": r.get::<i32, _>("character_limit"),
                "isReadOnly": r.get::<bool, _>("is_read_only"),
                "updatedAt": super::iso8601(updated_at)
            })
        })
        .collect();

    Ok(Json(blocks))
}

/// GET /api/memory/explorer-graph
pub async fn memory_explorer_graph() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "nodes": [],
        "edges": []
    }))
}

/// GET /api/audit/verify — verify audit chain integrity
pub async fn audit_verify(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rows = sera_db::audit::AuditRepository::get_chain_for_verification(state.db.inner(), 1000)
        .await
        .unwrap_or_default();

    let total = rows.len() as i64;
    let mut checked = 0i64;
    let mut broken_links = vec![];

    for (idx, row) in rows.iter().enumerate() {
        checked += 1;

        // Verify prev_hash matches previous entry's hash
        if idx > 0 {
            let prev_row = &rows[idx - 1];
            if let Some(ph) = &row.prev_hash && ph != &prev_row.hash {
                broken_links.push(serde_json::json!({
                    "sequence": row.sequence,
                    "expected": prev_row.hash,
                    "actual": ph
                }));
            }
        }
    }

    Ok(Json(serde_json::json!({
        "valid": broken_links.is_empty(),
        "totalEntries": total,
        "checkedEntries": checked,
        "brokenLinks": broken_links
    })))
}

/// GET /api/providers/dynamic
pub async fn providers_dynamic(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let providers = state.providers.read().await;
    let dynamic_providers = providers
        .providers
        .iter()
        .filter(|p| p.dynamic_provider_id.is_some())
        .map(|p| {
            serde_json::json!({
                "modelName": p.model_name,
                "provider": p.provider,
                "api": p.api,
                "baseUrl": p.base_url,
                "dynamicProviderId": p.dynamic_provider_id
            })
        })
        .collect::<Vec<_>>();

    Json(serde_json::json!({ "dynamicProviders": dynamic_providers }))
}

/// GET /api/providers/dynamic/statuses
pub async fn providers_dynamic_statuses(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let providers = state.providers.read().await;
    let statuses: Vec<serde_json::Value> = providers
        .providers
        .iter()
        .filter(|p| p.dynamic_provider_id.is_some())
        .map(|p| {
            serde_json::json!({
                "modelName": p.model_name,
                "status": "configured",
                "lastCheckedAt": None::<String>,
                "errors": None::<Vec<String>>
            })
        })
        .collect();

    Json(serde_json::json!({ "statuses": statuses }))
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
pub async fn agent_grants(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let rows = sqlx::query(
        "SELECT id, agent_instance_id, capability, action, granted_at, granted_by
         FROM capability_grants WHERE agent_instance_id = $1::uuid"
    )
    .bind(&id)
    .fetch_all(state.db.inner())
    .await
    .unwrap_or_default();

    let grants: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let id: uuid::Uuid = r.get("id");
            let granted_at: Option<time::OffsetDateTime> = r.get("granted_at");
            serde_json::json!({
                "id": id.to_string(),
                "agentInstanceId": r.get::<uuid::Uuid, _>("agent_instance_id").to_string(),
                "capability": r.get::<String, _>("capability"),
                "action": r.get::<String, _>("action"),
                "grantedAt": granted_at.map(super::iso8601),
                "grantedBy": r.get::<Option<String>, _>("granted_by")
            })
        })
        .collect();

    Ok(Json(grants))
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
pub async fn agent_health_check(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let row = sera_db::agents::AgentRepository::get_instance(state.db.inner(), &id).await?;

    // Check heartbeat — extract OffsetDateTime from the row
    let heartbeat_ok = if let Some(hb) = row.last_heartbeat_at {
        let now = time::OffsetDateTime::now_utc();
        let age_secs = (now - hb).whole_seconds();
        age_secs < 60
    } else {
        false
    };

    let status_str = row.status.as_deref().unwrap_or("unknown");
    let overall_status = match status_str {
        "running" if heartbeat_ok => "healthy",
        "running" => "stale",
        "stopped" => "stopped",
        _ => "unknown",
    };

    Ok(Json(serde_json::json!({
        "agentId": id,
        "agentName": row.name,
        "overallStatus": overall_status,
        "checks": {
            "heartbeat": {
                "ok": heartbeat_ok,
                "detail": row.last_heartbeat_at.map(super::iso8601)
            },
            "status": {
                "ok": status_str == "running",
                "detail": status_str
            }
        }
    })))
}

/// GET /api/agents/:id/sessions/:sid/commands — session command log
pub async fn session_commands(
    State(state): State<AppState>,
    Path((_id, sid)): Path<(String, String)>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let rows = sqlx::query(
        "SELECT id, session_id, task, status, result, error, created_at, started_at, completed_at
         FROM task_queue WHERE session_id = $1 ORDER BY created_at DESC"
    )
    .bind(&sid)
    .fetch_all(state.db.inner())
    .await
    .unwrap_or_default();

    let commands: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let id: uuid::Uuid = r.get("id");
            let created_at: time::OffsetDateTime = r.get("created_at");
            serde_json::json!({
                "id": id.to_string(),
                "sessionId": r.get::<String, _>("session_id"),
                "task": r.get::<String, _>("task"),
                "status": r.get::<String, _>("status"),
                "result": r.get::<Option<String>, _>("result"),
                "error": r.get::<Option<String>, _>("error"),
                "createdAt": super::iso8601(created_at),
                "startedAt": r.get::<Option<time::OffsetDateTime>, _>("started_at").map(super::iso8601),
                "completedAt": r.get::<Option<time::OffsetDateTime>, _>("completed_at").map(super::iso8601)
            })
        })
        .collect();

    Ok(Json(commands))
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
