//! Registry endpoints — advanced template and instance CRUD.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use sera_db::agents::AgentRepository;

use crate::error::AppError;
use crate::routes::agents::TemplateResponse;
use crate::state::AppState;

/// GET /api/registry/templates
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

/// GET /api/registry/templates/:name
pub async fn get_template(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<TemplateResponse>, AppError> {
    let row = AgentRepository::get_template(state.db.inner(), &name).await?;
    Ok(Json(TemplateResponse {
        name: row.name,
        display_name: row.display_name,
        builtin: row.builtin,
        category: row.category,
        spec: row.spec,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertTemplateRequest {
    pub name: String,
    pub display_name: Option<String>,
    pub category: Option<String>,
    pub spec: Value,
}

/// POST /api/registry/templates — create or upsert template.
pub async fn upsert_template(
    State(state): State<AppState>,
    Json(body): Json<UpsertTemplateRequest>,
) -> Result<(StatusCode, Json<TemplateResponse>), AppError> {
    // Upsert via INSERT ON CONFLICT
    sqlx::query(
        "INSERT INTO agent_templates (name, display_name, category, spec)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (name) DO UPDATE SET
           display_name = COALESCE($2, agent_templates.display_name),
           category = COALESCE($3, agent_templates.category),
           spec = $4,
           updated_at = NOW()",
    )
    .bind(&body.name)
    .bind(&body.display_name)
    .bind(&body.category)
    .bind(&body.spec)
    .execute(state.db.inner())
    .await
    .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    let row = AgentRepository::get_template(state.db.inner(), &body.name).await?;
    Ok((
        StatusCode::CREATED,
        Json(TemplateResponse {
            name: row.name,
            display_name: row.display_name,
            builtin: row.builtin,
            category: row.category,
            spec: row.spec,
        }),
    ))
}

/// PUT /api/registry/templates/:name
pub async fn update_template(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<UpsertTemplateRequest>,
) -> Result<Json<TemplateResponse>, AppError> {
    sqlx::query(
        "UPDATE agent_templates SET
           display_name = COALESCE($1, display_name),
           category = COALESCE($2, category),
           spec = $3,
           updated_at = NOW()
         WHERE name = $4",
    )
    .bind(&body.display_name)
    .bind(&body.category)
    .bind(&body.spec)
    .bind(&name)
    .execute(state.db.inner())
    .await
    .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    let row = AgentRepository::get_template(state.db.inner(), &name).await?;
    Ok(Json(TemplateResponse {
        name: row.name,
        display_name: row.display_name,
        builtin: row.builtin,
        category: row.category,
        spec: row.spec,
    }))
}

/// DELETE /api/registry/templates/:name
pub async fn delete_template(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    let result = sqlx::query("DELETE FROM agent_templates WHERE name = $1")
        .bind(&name)
        .execute(state.db.inner())
        .await
        .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "template",
            key: "name",
            value: name,
        }));
    }
    Ok(StatusCode::NO_CONTENT)
}

