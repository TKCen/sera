//! Agent and template read endpoints.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sera_db::agents::AgentRepository;
use sera_db::DbError;

use crate::error::AppError;
use crate::state::AppState;

// Re-import sqlx for inline queries in start/stop handlers
use sqlx;

/// Query params for listing instances.
#[derive(Debug, Deserialize)]
pub struct ListInstancesQuery {
    pub status: Option<String>,
}

/// Template response (camelCase for API compatibility).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateResponse {
    pub name: String,
    pub display_name: Option<String>,
    pub builtin: bool,
    pub category: Option<String>,
    pub spec: Value,
}

/// Instance response — snake_case to match the TypeScript core's response shape.
#[derive(Debug, Serialize)]
pub struct InstanceResponse {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub template_ref: String,
    pub circle: Option<String>,
    pub status: String,
    pub lifecycle_mode: Option<String>,
    pub parent_instance_id: Option<String>,
    pub workspace_path: Option<String>,
    pub container_id: Option<String>,
    pub sandbox_boundary: Option<String>,
    pub overrides: Option<serde_json::Value>,
    pub resolved_config: Option<serde_json::Value>,
    pub resolved_capabilities: Option<serde_json::Value>,
    pub last_heartbeat_at: Option<String>,
    pub updated_at: Option<String>,
    pub created_at: Option<String>,
}

/// GET /api/agents/templates
pub async fn list_templates(
    State(state): State<AppState>,
) -> Result<Json<Vec<TemplateResponse>>, AppError> {
    let rows = AgentRepository::list_templates(state.db.inner()).await?;
    let templates: Vec<TemplateResponse> = rows
        .into_iter()
        .map(|r| TemplateResponse {
            name: r.name,
            display_name: r.display_name,
            builtin: r.builtin,
            category: r.category,
            spec: r.spec,
        })
        .collect();
    Ok(Json(templates))
}

/// GET /api/agents
pub async fn list_instances(
    State(state): State<AppState>,
    Query(params): Query<ListInstancesQuery>,
) -> Result<Json<Vec<InstanceResponse>>, AppError> {
    let rows =
        AgentRepository::list_instances(state.db.inner(), params.status.as_deref()).await?;
    let instances: Vec<InstanceResponse> = rows.into_iter().map(instance_to_response).collect();
    Ok(Json(instances))
}

/// GET /api/agents/:id
pub async fn get_instance(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<InstanceResponse>, AppError> {
    let row = AgentRepository::get_instance(state.db.inner(), &id).await?;
    Ok(Json(instance_to_response(row)))
}

fn instance_to_response(r: sera_db::agents::InstanceRow) -> InstanceResponse {
    use super::iso8601_opt;
    InstanceResponse {
        id: r.id.to_string(),
        name: r.name,
        display_name: r.display_name,
        template_ref: r.template_ref.unwrap_or(r.template_name),
        circle: r.circle,
        status: r.status.unwrap_or_else(|| "active".to_string()),
        lifecycle_mode: r.lifecycle_mode,
        parent_instance_id: r.parent_instance_id.map(|id| id.to_string()),
        workspace_path: Some(r.workspace_path),
        container_id: r.container_id,
        sandbox_boundary: r.sandbox_boundary,
        overrides: r.overrides,
        resolved_config: r.resolved_config,
        resolved_capabilities: r.resolved_capabilities,
        last_heartbeat_at: iso8601_opt(r.last_heartbeat_at),
        updated_at: iso8601_opt(r.updated_at),
        created_at: iso8601_opt(r.created_at),
    }
}

/// Request body for creating an agent instance.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstanceRequest {
    pub template_ref: String,
    pub name: String,
    pub display_name: Option<String>,
    pub circle: Option<String>,
    pub lifecycle_mode: Option<String>,
}

/// POST /api/agents/instances
pub async fn create_instance(
    State(state): State<AppState>,
    Json(body): Json<CreateInstanceRequest>,
) -> Result<(StatusCode, Json<InstanceResponse>), AppError> {
    // Verify template exists
    AgentRepository::get_template(state.db.inner(), &body.template_ref).await?;

    // Check for duplicate name
    if AgentRepository::instance_name_exists(state.db.inner(), &body.name).await? {
        return Err(AppError::Db(DbError::Conflict(format!(
            "Agent instance with name '{}' already exists",
            body.name
        ))));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let workspace_path = format!("/workspaces/{}", body.name);

    AgentRepository::create_instance(
        state.db.inner(),
        &id,
        &body.name,
        &body.template_ref,
        &body.template_ref,
        &workspace_path,
        body.display_name.as_deref(),
        body.circle.as_deref(),
        body.lifecycle_mode.as_deref(),
    )
    .await?;

    let row = AgentRepository::get_instance(state.db.inner(), &id).await?;
    Ok((StatusCode::CREATED, Json(instance_to_response(row))))
}

/// Request body for updating an agent instance.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstanceRequest {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub circle: Option<String>,
    pub lifecycle_mode: Option<String>,
}

/// PATCH /api/agents/instances/:id
pub async fn update_instance(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateInstanceRequest>,
) -> Result<Json<InstanceResponse>, AppError> {
    AgentRepository::update_instance(
        state.db.inner(),
        &id,
        body.name.as_deref(),
        body.display_name.as_deref(),
        body.circle.as_deref(),
        body.lifecycle_mode.as_deref(),
    )
    .await?;

    let row = AgentRepository::get_instance(state.db.inner(), &id).await?;
    Ok(Json(instance_to_response(row)))
}

/// DELETE /api/agents/instances/:id
pub async fn delete_instance(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let name = AgentRepository::delete_instance(state.db.inner(), &id).await?;
    Ok(Json(serde_json::json!({
        "deleted": { "id": id, "name": name }
    })))
}

/// POST /api/agents/instances/:id/start
pub async fn start_instance(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<InstanceResponse>, AppError> {
    let instance = AgentRepository::get_instance(state.db.inner(), &id).await?;
    let template_ref = instance.template_ref.as_deref().unwrap_or(&instance.template_name);

    // Issue a JWT identity token for the agent container
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let identity_token = state.jwt.issue(sera_auth::JwtClaims {
        sub: instance.name.clone(),
        iss: "sera".to_string(),
        exp: now_secs + 86400 * 30, // 30 days
        agent_id: Some(instance.name.clone()),
        instance_id: Some(id.clone()),
    }).map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to issue agent JWT: {e}")))?;

    // ── Workspace provisioning ──────────────────────────────────────────────
    // Build the agent manifest YAML from template spec + instance overrides,
    // write it to /workspaces/<name>/AGENT.yaml so the agent-runtime can load it.
    //
    // Inside this container: /workspaces/<name>/
    // On the Docker host: $HOST_WORKSPACES_DIR/<name>/
    // The agent container bind mount must use the HOST path.
    let workspace_container_dir = format!("/workspaces/{}", instance.name);
    // For Docker bind mounts, we need the HOST path that the Docker daemon can resolve.
    // HOST_WORKSPACES_DIR should be an absolute path (e.g. D:/projects/homelab/sera/workspaces
    // on Docker Desktop for Windows, or /home/user/sera/workspaces on Linux).
    // Falls back to /workspaces which works on native Linux Docker.
    let host_workspaces = std::env::var("HOST_WORKSPACES_DIR")
        .unwrap_or_else(|_| "/workspaces".to_string());
    let workspace_host_dir = format!("{}/{}", host_workspaces, instance.name);

    // Create the directory inside our container (we have /workspaces mounted)
    std::fs::create_dir_all(&workspace_container_dir).ok();

    // Read template spec
    let template = AgentRepository::get_template(state.db.inner(), template_ref).await?;
    let mut manifest = template.spec.clone();

    // Merge instance overrides on top of template spec
    #[allow(clippy::collapsible_if)]
    if let Some(overrides) = &instance.overrides {
        if let (Some(base), Some(over)) = (manifest.as_object_mut(), overrides.as_object()) {
            for (k, v) in over {
                base.insert(k.clone(), v.clone());
            }
        }
    }

    // Add metadata block for agent-runtime
    if let Some(obj) = manifest.as_object_mut() {
        obj.insert("metadata".to_string(), serde_json::json!({
            "name": instance.name,
            "displayName": instance.display_name,
            "instanceId": id,
            "templateRef": template_ref,
        }));
    }

    // Write AGENT.yaml inside our container's mounted /workspaces
    if let Ok(yaml_str) = serde_yaml::to_string(&manifest) {
        let _ = std::fs::write(format!("{}/AGENT.yaml", workspace_container_dir), yaml_str);
    }

    // Set permissions for non-root agent user (uid 1001)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&workspace_container_dir, std::fs::Permissions::from_mode(0o777));
    }

    // Build bind mounts — use HOST path so Docker daemon can find the directory
    let binds = vec![
        format!("{}:/workspace:rw", workspace_host_dir),
        // Memory mount - per instance
        format!("{}/memory/{}:/memory:rw", host_workspaces.replace("/workspaces", ""), id),
        // Knowledge mounts
        format!("{}/knowledge/agents/{}:/knowledge/personal:ro", host_workspaces.replace("/workspaces", ""), instance.name),
        format!("{}/knowledge/shared:/knowledge/shared:ro", host_workspaces.replace("/workspaces", "")),
    ];

    let mut env_vars = std::collections::HashMap::new();
    env_vars.insert("AGENT_NAME".to_string(), instance.name.clone());
    env_vars.insert("AGENT_INSTANCE_ID".to_string(), id.clone());
    env_vars.insert("SERA_CORE_URL".to_string(), "http://sera-core:3001".to_string());
    env_vars.insert("SERA_IDENTITY_TOKEN".to_string(), identity_token);
    env_vars.insert("WORKSPACE_PATH".to_string(), "/workspace".to_string());
    env_vars.insert("AGENT_LIFECYCLE_MODE".to_string(),
        instance.lifecycle_mode.as_deref().unwrap_or("ephemeral").to_string());
    env_vars.insert("CENTRIFUGO_API_URL".to_string(),
        std::env::var("CENTRIFUGO_API_URL").unwrap_or_else(|_| "http://centrifugo:8000/api".to_string()));
    env_vars.insert("CENTRIFUGO_API_KEY".to_string(),
        std::env::var("CENTRIFUGO_API_KEY").unwrap_or_else(|_| "sera-api-key".to_string()));
    env_vars.insert("AGENT_HEARTBEAT_INTERVAL_MS".to_string(), "30000".to_string());
    env_vars.insert("SERA_LLM_PROXY_URL".to_string(), "http://sera-core:3001/v1/llm".to_string());
    env_vars.insert("AGENT_CHAT_PORT".to_string(), "3100".to_string());

    let container_id = state
        .docker
        .start_container(
            &id,
            &instance.name,
            template_ref,
            "sera-agent-worker:latest",
            "sera_net",
            env_vars,
            Some(binds),
            None,
        )
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Docker error: {e}")))?;

    // Update status and container_id
    AgentRepository::update_status(state.db.inner(), &id, "running").await?;
    sqlx::query("UPDATE agent_instances SET container_id = $1, updated_at = NOW() WHERE id::text = $2")
        .bind(&container_id)
        .bind(&id)
        .execute(state.db.inner())
        .await
        .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    let row = AgentRepository::get_instance(state.db.inner(), &id).await?;
    Ok(Json(instance_to_response(row)))
}

/// POST /api/agents/instances/:id/stop
pub async fn stop_instance(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<InstanceResponse>, AppError> {
    let instance = AgentRepository::get_instance(state.db.inner(), &id).await?;

    if let Some(container_id) = &instance.container_id {
        state
            .docker
            .stop_container(container_id)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("Docker error: {e}")))?;
    }

    // Update status and clear container_id
    AgentRepository::update_status(state.db.inner(), &id, "stopped").await?;
    sqlx::query("UPDATE agent_instances SET container_id = NULL, updated_at = NOW() WHERE id::text = $1")
        .bind(&id)
        .execute(state.db.inner())
        .await
        .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    let row = AgentRepository::get_instance(state.db.inner(), &id).await?;
    Ok(Json(instance_to_response(row)))
}
