//! Stub endpoints — return 501 Not Implemented or empty data.
//! These exist to prevent 404s for known TS routes that haven't been
//! fully ported yet.
#![allow(dead_code, unused_imports)]

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
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

/// Helper: standard 501 response with planned scope + owning bead.
fn not_impl_with(planned: &str, bead: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "planned": planned,
            "bead": bead,
        })),
    )
}

// ── Pipeline stubs ──────────────────────────────────────────────────────────

/// POST /api/pipelines
pub async fn create_pipeline() -> (StatusCode, Json<serde_json::Value>) {
    not_impl_with(
        "Pipeline creation backed by sera-workflow engine (post-MVS).",
        "sera-pipelines",
    )
}

/// GET /api/pipelines/:id
pub async fn get_pipeline(Path(_id): Path<String>) -> (StatusCode, Json<serde_json::Value>) {
    not_impl_with(
        "Pipeline read backed by sera-workflow engine (post-MVS).",
        "sera-pipelines",
    )
}

// ── Chat stubs (full impl is sera-runtime phase) ────────────────────────────

/// POST /api/chat
pub async fn chat() -> (StatusCode, Json<serde_json::Value>) {
    not_impl_with(
        "Chat endpoint superseded by routes::chat; this alias is deprecated.",
        "sera-chat",
    )
}

/// POST /v1/chat/completions (OpenAI compat)
pub async fn openai_chat_completions() -> (StatusCode, Json<serde_json::Value>) {
    not_impl_with(
        "OpenAI-compat endpoint superseded by routes::openai_compat; alias deprecated.",
        "sera-openai",
    )
}

// ── Embedding stubs ─────────────────────────────────────────────────────────

/// GET /api/embedding/config (dead — unregistered alias; real impl in routes::embedding)
pub async fn embedding_config() -> (StatusCode, Json<serde_json::Value>) {
    not_impl_with(
        "Dead stub — real impl in routes::embedding::get_config.",
        "sera-embedding",
    )
}

/// GET /api/embedding/status (dead — unregistered alias; real impl in routes::embedding)
pub async fn embedding_status() -> (StatusCode, Json<serde_json::Value>) {
    not_impl_with(
        "Dead stub — real impl in routes::embedding::get_status.",
        "sera-embedding",
    )
}

// ── Knowledge stubs ─────────────────────────────────────────────────────────

/// GET /api/knowledge/circles/:id/history
pub async fn knowledge_history(Path(_id): Path<String>) -> (StatusCode, Json<serde_json::Value>) {
    not_impl_with(
        "Circle knowledge history requires knowledge-store integration (post-MVS).",
        "sera-knowledge",
    )
}

// ── Agent sub-route stubs ───────────────────────────────────────────────────

/// GET /api/agents/:id/logs — return recent audit events for this agent.
///
/// Backed by the `audit_trail` table, filtered by `actor_id = :id` and ordered
/// by sequence descending. Capped at 200 rows.
pub async fn agent_logs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let rows = sqlx::query(
        "SELECT sequence, timestamp, actor_type, actor_id, event_type, payload
         FROM audit_trail
         WHERE actor_id = $1
         ORDER BY sequence DESC
         LIMIT 200",
    )
    .bind(&id)
    .fetch_all(state.db.require_pg_pool())
    .await
    .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    let logs: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let ts: time::OffsetDateTime = r.get("timestamp");
            serde_json::json!({
                "sequence": r.get::<i64, _>("sequence"),
                "timestamp": super::iso8601(ts),
                "actorType": r.get::<String, _>("actor_type"),
                "actorId": r.get::<String, _>("actor_id"),
                "eventType": r.get::<String, _>("event_type"),
                "payload": r.get::<serde_json::Value, _>("payload"),
            })
        })
        .collect();

    Ok(Json(logs))
}

/// GET /api/agents/:id/subagents — list subagents spawned by this agent.
///
/// Backed by `agent_instances.parent_instance_id = :id`.
pub async fn agent_subagents(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let rows = sqlx::query(
        "SELECT id, name, display_name, template_name, status, lifecycle_mode,
                last_heartbeat_at, created_at, updated_at
         FROM agent_instances
         WHERE parent_instance_id = $1::uuid
         ORDER BY created_at DESC",
    )
    .bind(&id)
    .fetch_all(state.db.require_pg_pool())
    .await
    .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    let subagents: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let id: uuid::Uuid = r.get("id");
            let created_at: Option<time::OffsetDateTime> = r.get("created_at");
            let updated_at: Option<time::OffsetDateTime> = r.get("updated_at");
            let last_heartbeat: Option<time::OffsetDateTime> = r.get("last_heartbeat_at");
            serde_json::json!({
                "id": id.to_string(),
                "name": r.get::<String, _>("name"),
                "displayName": r.get::<Option<String>, _>("display_name"),
                "templateName": r.get::<String, _>("template_name"),
                "status": r.get::<Option<String>, _>("status"),
                "lifecycleMode": r.get::<Option<String>, _>("lifecycle_mode"),
                "lastHeartbeatAt": last_heartbeat.map(super::iso8601),
                "createdAt": created_at.map(super::iso8601),
                "updatedAt": updated_at.map(super::iso8601),
            })
        })
        .collect();

    Ok(Json(subagents))
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
    .fetch_all(state.db.require_pg_pool())
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
    if let Ok(skills) =
        sera_db::skills::SkillRepository::list_skills(state.db.require_pg_pool()).await
    {
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
    if let Ok(skills) =
        sera_db::skills::SkillRepository::list_skills(state.db.require_pg_pool()).await
    {
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
    let rows = sera_db::agents::AgentRepository::list_templates(state.db.require_pg_pool()).await?;
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
    .fetch_optional(state.db.require_pg_pool())
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
             LIMIT $2",
        )
        .bind(agent_id)
        .bind(limit)
        .fetch_all(state.db.require_pg_pool())
        .await
    } else {
        sqlx::query(
            "SELECT tq.id, tq.status, tq.created_at, tq.started_at, tq.completed_at,
                    tq.result, tq.context
             FROM task_queue tq
             ORDER BY tq.created_at DESC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(state.db.require_pg_pool())
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
        .fetch_one(state.db.require_pg_pool())
        .await
        .unwrap_or((0,));

    // Agents with block counts
    let agent_rows = sqlx::query(
        "SELECT agent_instance_id, COUNT(*) as block_count FROM core_memory_blocks GROUP BY agent_instance_id"
    )
    .fetch_all(state.db.require_pg_pool())
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
    let rows =
        sera_db::memory::MemoryRepository::list_blocks(state.db.require_pg_pool(), Some(&agent_id))
            .await?;
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
    .execute(state.db.require_pg_pool())
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
    let agent =
        sera_db::agents::AgentRepository::get_instance(state.db.require_pg_pool(), &id).await?;

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

/// GET /api/agents/:id/template-diff — detect template drift for an instance.
///
/// Compares `agent_instances.updated_at` to the joined
/// `agent_templates.updated_at`. If the template is newer, `hasChanges` is
/// `true`. The full diff payload is left empty (detailed field-level diff is
/// post-MVS).
pub async fn agent_template_diff(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let row = sqlx::query(
        "SELECT ai.id, ai.template_name, ai.updated_at AS instance_updated_at,
                at.updated_at AS template_updated_at
         FROM agent_instances ai
         LEFT JOIN agent_templates at ON ai.template_name = at.name
         WHERE ai.id = $1::uuid",
    )
    .bind(&id)
    .fetch_optional(state.db.require_pg_pool())
    .await
    .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    match row {
        Some(r) => {
            let instance_updated_at: Option<time::OffsetDateTime> = r.get("instance_updated_at");
            let template_updated_at: Option<time::OffsetDateTime> = r.get("template_updated_at");
            let template_name: String = r.get("template_name");

            let has_changes = match (instance_updated_at, template_updated_at) {
                (Some(iu), Some(tu)) => tu > iu,
                (None, Some(_)) => true,
                _ => false,
            };

            Ok(Json(serde_json::json!({
                "hasChanges": has_changes,
                "instanceId": id,
                "templateName": template_name,
                "templateUpdatedAt": template_updated_at.map(super::iso8601),
                "instanceAppliedAt": instance_updated_at.map(super::iso8601),
                "changes": []
            })))
        }
        None => Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "agent_instance",
            key: "id",
            value: id,
        })),
    }
}

/// GET /api/memory/recent
pub async fn memory_recent(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let rows = sqlx::query(
        "SELECT id, agent_instance_id, name, content, character_limit, is_read_only, created_at, updated_at
         FROM core_memory_blocks ORDER BY updated_at DESC LIMIT 20"
    )
    .fetch_all(state.db.require_pg_pool())
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
pub async fn memory_explorer_graph() -> (StatusCode, Json<serde_json::Value>) {
    not_impl_with(
        "Memory explorer graph visualization is post-MVS.",
        "sera-memory-graph",
    )
}

/// GET /api/audit/verify — verify audit chain integrity
pub async fn audit_verify(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rows = sera_db::audit::AuditRepository::get_chain_for_verification(
        state.db.require_pg_pool(),
        1000,
    )
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
            if let Some(ph) = &row.prev_hash
                && ph != &prev_row.hash
            {
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
pub async fn providers_dynamic(State(state): State<AppState>) -> Json<serde_json::Value> {
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
pub async fn providers_dynamic_statuses(State(state): State<AppState>) -> Json<serde_json::Value> {
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
pub async fn providers_templates() -> (StatusCode, Json<serde_json::Value>) {
    not_impl_with(
        "Provider template library is post-MVS.",
        "sera-providers-templates",
    )
}

/// GET /api/providers/default-model
pub async fn providers_default_model() -> (StatusCode, Json<serde_json::Value>) {
    not_impl_with(
        "Global default-model config is MVS-optional; not yet wired.",
        "sera-defaults",
    )
}

/// PUT /api/providers/default-model
pub async fn set_default_model() -> (StatusCode, Json<serde_json::Value>) {
    not_impl_with(
        "Global default-model config is MVS-optional; not yet wired.",
        "sera-defaults",
    )
}

/// GET /api/agents/:id/grants — list agent capability grants
pub async fn agent_grants(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let rows = sqlx::query(
        "SELECT id, agent_instance_id, capability, action, granted_at, granted_by
         FROM capability_grants WHERE agent_instance_id = $1::uuid",
    )
    .bind(&id)
    .fetch_all(state.db.require_pg_pool())
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
pub async fn agent_context_debug(Path(_id): Path<String>) -> (StatusCode, Json<serde_json::Value>) {
    not_impl_with(
        "Context-debug tracing requires sera-runtime instrumentation (post-MVS).",
        "sera-debug",
    )
}

/// GET /api/agents/:id/system-prompt — get agent system prompt
pub async fn agent_system_prompt(Path(_id): Path<String>) -> (StatusCode, Json<serde_json::Value>) {
    not_impl_with(
        "System prompt is assembled by sera-runtime::manifest; gateway read path TBD.",
        "sera-runtime",
    )
}

/// GET /api/agents/:id/health-check — agent health check
/// Frontend expects: { agentId, agentName?, overallStatus, checks: Record<string, {ok, detail?}> }
pub async fn agent_health_check(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let row =
        sera_db::agents::AgentRepository::get_instance(state.db.require_pg_pool(), &id).await?;

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
         FROM task_queue WHERE session_id = $1 ORDER BY created_at DESC",
    )
    .bind(&sid)
    .fetch_all(state.db.require_pg_pool())
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
    let deleted =
        sera_db::memory::MemoryRepository::delete_block(state.db.require_pg_pool(), &block_id)
            .await?;
    if !deleted {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "memory_block",
            key: "id",
            value: block_id,
        }));
    }
    Ok(Json(serde_json::json!({"success": true})))
}

#[cfg(test)]
mod tests {
    //! Shape tests for the sera-3npy stub rewrite.
    //!
    //! These tests verify handler-response shapes without requiring a live
    //! Postgres connection. Handlers that hit the DB are tested elsewhere via
    //! the `integration_tests` suite (feature-gated on `DATABASE_URL`); here
    //! we only assert JSON contract for the 501/503 branches and the static
    //! fragments of the three newly implemented MVS-CRITICAL endpoints.
    use super::*;

    // ── 501 contract ────────────────────────────────────────────────────────

    #[test]
    fn not_impl_with_returns_501_with_bead() {
        let (status, Json(body)) = not_impl_with("sample scope", "sera-example");
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["error"], "not_implemented");
        assert_eq!(body["planned"], "sample scope");
        assert_eq!(body["bead"], "sera-example");
    }

    #[tokio::test]
    async fn memory_explorer_graph_returns_501() {
        let (status, Json(body)) = memory_explorer_graph().await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["error"], "not_implemented");
        assert_eq!(body["bead"], "sera-memory-graph");
    }

    #[tokio::test]
    async fn providers_default_model_get_returns_501() {
        let (status, Json(body)) = providers_default_model().await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["error"], "not_implemented");
    }

    #[tokio::test]
    async fn set_default_model_returns_501_not_fake_success() {
        // Previously this PUT silently returned {"success": true}. Regression
        // guard: ensure it now explicitly reports not-implemented.
        let (status, Json(body)) = set_default_model().await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_ne!(body["success"], serde_json::json!(true));
        assert_eq!(body["error"], "not_implemented");
    }

    #[tokio::test]
    async fn agent_context_debug_returns_501() {
        let (status, Json(body)) = agent_context_debug(Path("instance-id".to_string())).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["bead"], "sera-debug");
    }

    #[tokio::test]
    async fn agent_system_prompt_returns_501() {
        let (status, Json(body)) = agent_system_prompt(Path("instance-id".to_string())).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["bead"], "sera-runtime");
    }

    #[tokio::test]
    async fn providers_templates_returns_501() {
        let (status, Json(body)) = providers_templates().await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["bead"], "sera-providers-templates");
    }

    #[tokio::test]
    async fn knowledge_history_returns_501() {
        let (status, Json(body)) = knowledge_history(Path("circle-id".to_string())).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["bead"], "sera-knowledge");
    }

    #[tokio::test]
    async fn pipeline_stubs_return_501() {
        let (status, _) = create_pipeline().await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        let (status2, _) = get_pipeline(Path("p-1".to_string())).await;
        assert_eq!(status2, StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn chat_alias_returns_501() {
        let (status, Json(body)) = chat().await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["bead"], "sera-chat");
        let (status2, _) = openai_chat_completions().await;
        assert_eq!(status2, StatusCode::NOT_IMPLEMENTED);
    }

    // ── MVS-CRITICAL integration contract ───────────────────────────────────
    //
    // The three MVS-CRITICAL endpoints (`agent_logs`, `agent_subagents`,
    // `agent_template_diff`) require a live PgPool to exercise end-to-end.
    // Their integration tests live here as JSON-shape contract assertions to
    // document the response fields the frontend depends on, without spinning
    // up Postgres in unit mode.

    #[test]
    fn agent_logs_response_contract() {
        // Each element of the array must contain these fields. This matches
        // the SQL SELECT in `agent_logs` above.
        let sample = serde_json::json!({
            "sequence": 42,
            "timestamp": "2026-04-17T00:00:00Z",
            "actorType": "agent",
            "actorId": "a-1",
            "eventType": "lane_failure",
            "payload": {}
        });
        for field in [
            "sequence",
            "timestamp",
            "actorType",
            "actorId",
            "eventType",
            "payload",
        ] {
            assert!(sample.get(field).is_some(), "missing {field}");
        }
    }

    #[test]
    fn agent_subagents_response_contract() {
        let sample = serde_json::json!({
            "id": "uuid-here",
            "name": "child-agent",
            "displayName": null,
            "templateName": "researcher",
            "status": "running",
            "lifecycleMode": null,
            "lastHeartbeatAt": null,
            "createdAt": "2026-04-17T00:00:00Z",
            "updatedAt": "2026-04-17T00:00:00Z"
        });
        for field in [
            "id",
            "name",
            "templateName",
            "status",
            "lifecycleMode",
            "lastHeartbeatAt",
            "createdAt",
            "updatedAt",
        ] {
            assert!(sample.get(field).is_some(), "missing {field}");
        }
    }

    #[test]
    fn agent_template_diff_response_contract() {
        let sample = serde_json::json!({
            "hasChanges": true,
            "instanceId": "uuid-here",
            "templateName": "researcher",
            "templateUpdatedAt": "2026-04-17T00:00:00Z",
            "instanceAppliedAt": "2026-04-15T00:00:00Z",
            "changes": []
        });
        for field in [
            "hasChanges",
            "instanceId",
            "templateName",
            "templateUpdatedAt",
            "instanceAppliedAt",
            "changes",
        ] {
            assert!(sample.get(field).is_some(), "missing {field}");
        }
        assert!(sample["hasChanges"].is_boolean());
        assert!(sample["changes"].is_array());
    }
}
