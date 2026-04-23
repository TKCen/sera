//! Operator request endpoints.

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use sera_auth::ActingContext;
use sera_db::operator_requests::OperatorRequestRepository;

use crate::error::AppError;
use crate::state::AppState;

/// Verify that the authenticated caller may speak for `claimed_agent_id`.
///
/// An agent-scoped caller (ActingContext with `agent_id = Some(x)`) may only
/// submit operator requests where the payload's `agent_id` matches `x`.
/// An operator-scoped caller (no `agent_id` set) may submit requests for
/// any agent — this covers operator UI flows that route through this
/// endpoint on behalf of a user.
///
/// Returns `AppError::Forbidden` (→ HTTP 403) on mismatch. This is also the
/// hook point that an `operator.request` MCP tool invocation should call
/// before writing to the database, so the ownership check lives in a pure
/// helper rather than on the axum handler signature.
pub fn verify_agent_ownership(ctx: &ActingContext, claimed_agent_id: &str) -> Result<(), AppError> {
    match &ctx.agent_id {
        Some(caller_agent_id) if caller_agent_id == claimed_agent_id => Ok(()),
        Some(caller_agent_id) => Err(AppError::Forbidden(format!(
            "agent '{caller_agent_id}' may not submit operator requests for agent '{claimed_agent_id}'"
        ))),
        None => {
            // Operator-scoped callers may proxy on behalf of any agent.
            Ok(())
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRequestResponse {
    pub id: String,
    pub agent_id: String,
    pub agent_name: Option<String>,
    pub r#type: String,
    pub title: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub response: Option<serde_json::Value>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

fn to_response(r: sera_db::operator_requests::OperatorRequestRow) -> OperatorRequestResponse {
    OperatorRequestResponse {
        id: r.id.to_string(),
        agent_id: r.agent_id,
        agent_name: r.agent_name,
        r#type: r.r#type,
        title: r.title,
        payload: r.payload,
        status: r.status,
        response: r.response,
        created_at: super::iso8601(r.created_at),
        resolved_at: r.resolved_at.map(super::iso8601),
    }
}

/// GET /api/operator-requests/pending/count
pub async fn pending_count(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let count = OperatorRequestRepository::count_pending(state.db.require_pg_pool()).await?;
    Ok(Json(serde_json::json!({"count": count})))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRequestsQuery {
    pub status: Option<String>,
    pub agent_id: Option<String>,
    pub limit: Option<i64>,
}

/// GET /api/operator-requests
pub async fn list_requests(
    State(state): State<AppState>,
    Query(params): Query<ListRequestsQuery>,
) -> Result<Json<Vec<OperatorRequestResponse>>, AppError> {
    let limit = params.limit.unwrap_or(50).min(500);
    let rows = OperatorRequestRepository::list(
        state.db.require_pg_pool(),
        params.status.as_deref(),
        params.agent_id.as_deref(),
        limit,
    )
    .await?;
    Ok(Json(rows.into_iter().map(to_response).collect()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOperatorRequestBody {
    pub agent_id: String,
    pub agent_name: Option<String>,
    pub r#type: String,
    pub title: String,
    pub payload: serde_json::Value,
}

/// POST /api/operator-requests — agent creates a new operator request.
///
/// Enforces that the caller's `agent_id` in the auth context matches the
/// `agent_id` claimed in the request body. Operator-scoped callers (no
/// agent_id) may proxy for any agent.
pub async fn create_request(
    State(state): State<AppState>,
    Extension(ctx): Extension<ActingContext>,
    Json(body): Json<CreateOperatorRequestBody>,
) -> Result<(StatusCode, Json<OperatorRequestResponse>), AppError> {
    verify_agent_ownership(&ctx, &body.agent_id)?;

    let id = uuid::Uuid::new_v4();
    let now = time::OffsetDateTime::now_utc();

    sqlx::query(
        "INSERT INTO operator_requests (id, agent_id, agent_name, type, title, payload, status, created_at)
         VALUES ($1::uuid, $2, $3, $4, $5, $6, 'pending', $7)"
    )
    .bind(id)
    .bind(&body.agent_id)
    .bind(&body.agent_name)
    .bind(&body.r#type)
    .bind(&body.title)
    .bind(&body.payload)
    .bind(now)
    .execute(state.db.require_pg_pool())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to create operator request: {e}")))?;

    Ok((
        StatusCode::CREATED,
        Json(OperatorRequestResponse {
            id: id.to_string(),
            agent_id: body.agent_id,
            agent_name: body.agent_name,
            r#type: body.r#type,
            title: body.title,
            payload: body.payload,
            status: "pending".to_string(),
            response: None,
            created_at: super::iso8601(now),
            resolved_at: None,
        }),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RespondRequest {
    pub status: String,
    pub response: Option<serde_json::Value>,
}

/// POST /api/operator-requests/:id/respond
pub async fn respond_to_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RespondRequest>,
) -> Result<Json<OperatorRequestResponse>, AppError> {
    let row = OperatorRequestRepository::respond(
        state.db.require_pg_pool(),
        &id,
        &body.status,
        body.response.as_ref(),
    )
    .await?;
    Ok(Json(to_response(row)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_auth::types::AuthMethod;

    fn ctx_for_agent(agent_id: &str) -> ActingContext {
        ActingContext {
            operator_id: None,
            agent_id: Some(agent_id.to_string()),
            instance_id: None,
            api_key_id: None,
            auth_method: AuthMethod::Jwt,
        }
    }

    fn ctx_for_operator() -> ActingContext {
        ActingContext {
            operator_id: Some("op-1".to_string()),
            agent_id: None,
            instance_id: None,
            api_key_id: None,
            auth_method: AuthMethod::Jwt,
        }
    }

    #[test]
    fn ownership_match_is_ok() {
        let ctx = ctx_for_agent("agent-42");
        assert!(verify_agent_ownership(&ctx, "agent-42").is_ok());
    }

    #[test]
    fn ownership_mismatch_is_forbidden() {
        let ctx = ctx_for_agent("agent-42");
        let err = verify_agent_ownership(&ctx, "agent-99").unwrap_err();
        match err {
            AppError::Forbidden(msg) => {
                assert!(msg.contains("agent-42"), "msg should name caller: {msg}");
                assert!(msg.contains("agent-99"), "msg should name target: {msg}");
            }
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }

    #[test]
    fn operator_caller_may_proxy_for_any_agent() {
        let ctx = ctx_for_operator();
        assert!(verify_agent_ownership(&ctx, "agent-anyone").is_ok());
    }
}
