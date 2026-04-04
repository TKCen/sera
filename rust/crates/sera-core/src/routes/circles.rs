//! Circles endpoint.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx;

use sera_db::circles::CircleRepository;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CircleResponse {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
}

/// GET /api/circles
pub async fn list_circles(
    State(state): State<AppState>,
) -> Result<Json<Vec<CircleResponse>>, AppError> {
    let rows = CircleRepository::list_circles(state.db.inner()).await?;
    let circles: Vec<CircleResponse> = rows
        .into_iter()
        .map(|r| CircleResponse {
            id: r.id.to_string(),
            name: r.name,
            display_name: r.display_name,
            description: r.description,
        })
        .collect();
    Ok(Json(circles))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCircleRequest {
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
}

/// POST /api/circles
pub async fn create_circle(
    State(state): State<AppState>,
    Json(body): Json<CreateCircleRequest>,
) -> Result<(StatusCode, Json<CircleResponse>), AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    CircleRepository::create_circle(
        state.db.inner(),
        &id,
        &body.name,
        &body.display_name,
        body.description.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(CircleResponse {
        id,
        name: body.name,
        display_name: body.display_name,
        description: body.description,
    })))
}

/// GET /api/circles/:name — get a single circle by name or id.
pub async fn get_circle(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<CircleResponse>, AppError> {
    let row = CircleRepository::get_by_name(state.db.inner(), &name).await?;
    Ok(Json(CircleResponse {
        id: row.id.to_string(),
        name: row.name,
        display_name: row.display_name,
        description: row.description,
    }))
}

/// PUT /api/circles/:id — update a circle.
#[allow(dead_code)]
pub async fn update_circle(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateCircleRequest>,
) -> Result<Json<CircleResponse>, AppError> {
    // Upsert — update display_name and description
    sqlx::query(
        "UPDATE circles SET display_name = $1, description = $2, updated_at = NOW() WHERE id::text = $3 OR name = $3"
    )
    .bind(&body.display_name)
    .bind(&body.description)
    .bind(&id)
    .execute(state.db.inner())
    .await
    .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    let row = CircleRepository::get_by_name(state.db.inner(), &id).await?;
    Ok(Json(CircleResponse {
        id: row.id.to_string(),
        name: row.name,
        display_name: row.display_name,
        description: row.description,
    }))
}

/// DELETE /api/circles/:id
pub async fn delete_circle(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    CircleRepository::delete_circle(state.db.inner(), &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
