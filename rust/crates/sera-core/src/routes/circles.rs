//! Circles endpoint.

use axum::{extract::State, Json};
use serde::Serialize;

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
