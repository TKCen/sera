//! Circle CRUD HTTP routes (sera-3g1n).
//!
//! Routes:
//!   POST   /api/circles              — create a circle
//!   GET    /api/circles              — list all circles
//!   GET    /api/circles/{id}         — get a single circle by id or name
//!   PUT    /api/circles/{id}         — update a circle's display_name / description
//!   DELETE /api/circles/{id}         — delete a circle
//!
//! Enforcement note: the `CircleRepository` is Postgres-only. When the
//! gateway runs against a SQLite backend (MVS), these routes return 503
//! Service Unavailable. Full enforcement of capability-policy per circle
//! member is tracked in the follow-up bead (see TODO at bottom of file).
//!
//! The handlers are generic over [`CirclesAppState`] so unit tests can drive
//! them without a live Postgres instance.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use sera_db::circles::CircleRepository;

// ── AppState abstraction ──────────────────────────────────────────────────────

/// Abstraction over AppState for circle CRUD handlers.
///
/// Returns `Some(&PgPool)` in Postgres deployments, `None` in SQLite (MVS)
/// deployments. Handlers respond 503 when the pool is unavailable.
pub trait CirclesAppState: Send + Sync + 'static {
    fn api_key(&self) -> &Option<String>;
    fn pg_pool(&self) -> Option<&sqlx::PgPool>;
}

// ── Auth ──────────────────────────────────────────────────────────────────────

fn check_auth(api_key: &Option<String>, headers: &HeaderMap) -> Result<(), StatusCode> {
    let expected = match api_key {
        None => return Ok(()),
        Some(k) => k,
    };
    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match provided {
        Some(k) if k == expected => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

// ── Request / response types ──────────────────────────────────────────────────

/// Request body for `POST /api/circles`.
#[derive(Debug, Deserialize)]
pub struct CreateCircleRequest {
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Request body for `PUT /api/circles/{id}`.
#[derive(Debug, Deserialize)]
pub struct UpdateCircleRequest {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// JSON projection of a circle row returned by list/get routes.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct CircleView {
    pub id: String,
    pub name: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constitution: Option<String>,
}

impl From<sera_db::circles::CircleRow> for CircleView {
    fn from(row: sera_db::circles::CircleRow) -> Self {
        Self {
            id: row.id.to_string(),
            name: row.name,
            display_name: row.display_name,
            description: row.description,
            constitution: row.constitution,
        }
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `POST /api/circles` — create a new circle.
pub async fn create_circle<S: CirclesAppState>(
    State(state): State<std::sync::Arc<S>>,
    headers: HeaderMap,
    Json(body): Json<CreateCircleRequest>,
) -> Result<(StatusCode, Json<CircleView>), StatusCode> {
    check_auth(state.api_key(), &headers)?;
    let pool = state.pg_pool().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let id = Uuid::new_v4().to_string();
    CircleRepository::create_circle(
        pool,
        &id,
        &body.name,
        &body.display_name,
        body.description.as_deref(),
    )
    .await
    .map_err(|e| {
        tracing::error!("create_circle db error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let row = CircleRepository::get_by_name(pool, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((StatusCode::CREATED, Json(CircleView::from(row))))
}

/// `GET /api/circles` — list all circles.
pub async fn list_circles<S: CirclesAppState>(
    State(state): State<std::sync::Arc<S>>,
    headers: HeaderMap,
) -> Result<Json<Vec<CircleView>>, StatusCode> {
    check_auth(state.api_key(), &headers)?;
    let pool = state.pg_pool().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let rows = CircleRepository::list_circles(pool)
        .await
        .map_err(|e| {
            tracing::error!("list_circles db error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(rows.into_iter().map(CircleView::from).collect()))
}

/// `GET /api/circles/{id}` — get a circle by id or name.
pub async fn get_circle<S: CirclesAppState>(
    State(state): State<std::sync::Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<CircleView>, StatusCode> {
    check_auth(state.api_key(), &headers)?;
    let pool = state.pg_pool().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let row = CircleRepository::get_by_name(pool, &id)
        .await
        .map_err(|e| match e {
            sera_db::DbError::NotFound { .. } => StatusCode::NOT_FOUND,
            other => {
                tracing::error!("get_circle db error: {other}");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        })?;

    Ok(Json(CircleView::from(row)))
}

/// `PUT /api/circles/{id}` — update display_name and/or description.
///
/// At least one field must be provided; returns 400 if both are absent.
pub async fn update_circle<S: CirclesAppState>(
    State(state): State<std::sync::Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateCircleRequest>,
) -> Result<Json<CircleView>, StatusCode> {
    check_auth(state.api_key(), &headers)?;

    if body.display_name.is_none() && body.description.is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let pool = state.pg_pool().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Fetch existing row first to apply partial update.
    let existing = CircleRepository::get_by_name(pool, &id)
        .await
        .map_err(|e| match e {
            sera_db::DbError::NotFound { .. } => StatusCode::NOT_FOUND,
            other => {
                tracing::error!("update_circle fetch error: {other}");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        })?;

    let new_display_name = body.display_name.as_deref().unwrap_or(&existing.display_name);
    let new_description = body.description.as_deref().or(existing.description.as_deref());

    sqlx::query(
        "UPDATE circles SET display_name = $1, description = $2, updated_at = NOW()
         WHERE id::text = $3 OR name = $3",
    )
    .bind(new_display_name)
    .bind(new_description)
    .bind(&id)
    .execute(pool)
    .await
    .map_err(|e| {
        tracing::error!("update_circle db error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let updated = CircleRepository::get_by_name(pool, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(CircleView::from(updated)))
}

/// `DELETE /api/circles/{id}` — delete a circle.
pub async fn delete_circle<S: CirclesAppState>(
    State(state): State<std::sync::Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    check_auth(state.api_key(), &headers)?;
    let pool = state.pg_pool().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    CircleRepository::delete_circle(pool, &id)
        .await
        .map_err(|e| match e {
            sera_db::DbError::NotFound { .. } => StatusCode::NOT_FOUND,
            other => {
                tracing::error!("delete_circle db error: {other}");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::{get, post};
    use tower::ServiceExt;

    use super::*;

    // ── Minimal test state ────────────────────────────────────────────────────

    /// SQLite-backed test state — pg_pool() returns None, so CRUD routes
    /// return 503. Used to verify that auth and route registration work
    /// independently of the DB backend.
    struct NoPoolState {
        api_key: Option<String>,
    }

    impl CirclesAppState for NoPoolState {
        fn api_key(&self) -> &Option<String> {
            &self.api_key
        }
        fn pg_pool(&self) -> Option<&sqlx::PgPool> {
            None
        }
    }

    fn router_no_pool(api_key: Option<&str>) -> Router {
        let state = Arc::new(NoPoolState {
            api_key: api_key.map(str::to_owned),
        });
        Router::new()
            .route("/api/circles", post(create_circle::<NoPoolState>).get(list_circles::<NoPoolState>))
            .route(
                "/api/circles/{id}",
                get(get_circle::<NoPoolState>)
                    .put(update_circle::<NoPoolState>)
                    .delete(delete_circle::<NoPoolState>),
            )
            .with_state(state)
    }

    // ── Auth enforcement ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_requires_auth_when_key_set() {
        let app = router_no_pool(Some("secret"));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/circles")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn list_returns_503_when_no_pg_pool_but_auth_passes() {
        let app = router_no_pool(None);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/circles")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // No api_key → auth passes; no pg_pool → 503
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn create_requires_auth_when_key_set() {
        let app = router_no_pool(Some("secret"));
        let body = serde_json::json!({"name": "eng", "display_name": "Engineering"});
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/circles")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn create_returns_503_when_no_pg_pool() {
        let app = router_no_pool(None);
        let body = serde_json::json!({"name": "eng", "display_name": "Engineering"});
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/circles")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn get_requires_auth_when_key_set() {
        let app = router_no_pool(Some("secret"));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/circles/some-id")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn delete_requires_auth_when_key_set() {
        let app = router_no_pool(Some("secret"));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/circles/some-id")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn update_returns_400_when_no_fields() {
        // No pool, no auth — check the 400 validation fires before DB access.
        let app = router_no_pool(None);
        let body = serde_json::json!({});
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/circles/some-id")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // TODO(sera-3g1n-follow): add Postgres integration tests exercising
    // actual CRUD round-trips (create → list → get → update → delete) once
    // the `#[cfg(feature = "integration")]` test harness is wired up.
    // See `rust/CLAUDE.md` for the DATABASE_URL requirement.
}

// TODO(sera-3g1n-follow): circle-based capability-policy enforcement.
//
// Full enforcement (Scenario 4.6: agent in circle X uses tool T; agent
// outside X cannot) requires plugging into the tool-dispatch hook chain
// in `sera-runtime` — specifically `agent_tool_registry` and the
// `ToolContext` builder. That work is a separate bead scoped against
// `sera_runtime::dispatch`. The CRUD API above is the prerequisite.
