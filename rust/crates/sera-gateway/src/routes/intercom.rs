//! Intercom messaging endpoints — publish to channels and send DMs via Centrifugo.
#![allow(dead_code, unused_imports)]

use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use sera_auth::ActingContext;

use crate::error::AppError;
use crate::state::AppState;

/// Verify that the authenticated caller may speak for `claimed_agent_id`.
///
/// Mirrors [`operator_requests::verify_agent_ownership`]: agent-scoped
/// callers (ActingContext with `agent_id = Some(x)`) may only publish or DM
/// as themselves. Operator-scoped callers (bootstrap API key, operator JWTs)
/// may proxy for any agent — they're used by the dashboard and admin tools.
fn verify_agent_ownership(
    ctx: &ActingContext,
    claimed_agent_id: &str,
) -> Result<(), AppError> {
    match &ctx.agent_id {
        Some(caller) if caller == claimed_agent_id => Ok(()),
        Some(caller) => Err(AppError::Forbidden(format!(
            "agent '{caller}' may not act as agent '{claimed_agent_id}'"
        ))),
        None => Ok(()), // operator-scoped — allowed to proxy
    }
}

#[derive(Deserialize)]
pub struct PublishRequest {
    pub agent: String,
    pub channel: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub payload: serde_json::Value,
}

#[derive(Serialize)]
pub struct PublishResponse {
    pub success: bool,
    pub message: Option<serde_json::Value>,
}

/// POST /api/intercom/publish — publish message to a Centrifugo channel
pub async fn publish(
    State(state): State<AppState>,
    Extension(ctx): Extension<ActingContext>,
    Json(body): Json<PublishRequest>,
) -> Result<Json<PublishResponse>, AppError> {
    // Agent-scoped callers may only publish as themselves. Operator callers
    // (dashboard, bootstrap API key) may publish for any agent.
    verify_agent_ownership(&ctx, &body.agent)?;
    tracing::debug!(agent = %body.agent, channel = %body.channel, "intercom publish authorized");
    let centrifugo = state
        .centrifugo
        .as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Centrifugo not configured")))?;

    let message = serde_json::json!({
        "type": body.message_type,
        "payload": body.payload,
    });

    centrifugo
        .publish(&body.channel, message.clone())
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Centrifugo publish failed: {e}")))?;

    Ok(Json(PublishResponse {
        success: true,
        message: Some(message),
    }))
}

#[derive(Deserialize)]
pub struct DmRequest {
    pub from: String,
    pub to: String,
    pub payload: serde_json::Value,
}

#[derive(Serialize)]
pub struct DmResponse {
    pub success: bool,
    pub message: Option<serde_json::Value>,
}

/// POST /api/intercom/dm — send direct message between agents
pub async fn dm(
    State(state): State<AppState>,
    Extension(ctx): Extension<ActingContext>,
    Json(body): Json<DmRequest>,
) -> Result<Json<DmResponse>, AppError> {
    // Enforce that the sender claimed in `from` matches the authenticated
    // agent. Operator callers may impersonate any sender.
    verify_agent_ownership(&ctx, &body.from)?;
    tracing::debug!(from = %body.from, to = %body.to, "intercom dm authorized");
    let centrifugo = state
        .centrifugo
        .as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Centrifugo not configured")))?;

    // DM channel format: agent:{to_agent_id}
    let channel = format!("agent:{}", body.to);
    let message = serde_json::json!({
        "type": "dm",
        "from": body.from,
        "to": body.to,
        "payload": body.payload,
    });

    centrifugo
        .publish(&channel, message.clone())
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Centrifugo DM failed: {e}")))?;

    Ok(Json(DmResponse {
        success: true,
        message: Some(message),
    }))
}

/// Query params for history endpoint
#[derive(Deserialize)]
pub struct HistoryQuery {
    pub channel: String,
    pub limit: Option<u32>,
}

#[derive(Serialize)]
pub struct HistoryResponse {
    pub channel: String,
    pub messages: Vec<serde_json::Value>,
}

/// GET /api/intercom/history — retrieve channel message history
pub async fn get_history(
    State(_state): State<AppState>,
    Query(params): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, AppError> {
    if params.channel.is_empty() {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Query param 'channel' is required"
        )));
    }

    // Centrifugo history API not yet integrated — CentrifugoClient lacks a history method.
    // Returns empty results until the history endpoint is added to sera-events.
    tracing::info!(channel = %params.channel, "intercom history: Centrifugo history API not yet integrated");
    Ok(Json(HistoryResponse {
        channel: params.channel,
        messages: vec![],
    }))
}

/// Query params for channels endpoint
#[derive(Deserialize)]
pub struct ChannelsQuery {
    pub agent: String,
}

#[derive(Serialize)]
pub struct ChannelsResponse {
    pub channels: Vec<String>,
}

/// GET /api/intercom/channels — list channels for an agent
pub async fn get_channels(
    State(_state): State<AppState>,
    Query(params): Query<ChannelsQuery>,
) -> Result<Json<ChannelsResponse>, AppError> {
    if params.agent.is_empty() {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Query param 'agent' is required"
        )));
    }

    // Default channel set — manifest-based channel authorization deferred to sera-auth
    Ok(Json(ChannelsResponse {
        channels: vec![format!("agent:{}", params.agent), "broadcast".to_string()],
    }))
}

#[derive(Deserialize)]
pub struct BridgeReceiveRequest {
    pub channel: String,
    pub message: serde_json::Value,
}

/// POST /api/intercom/bridge/receive — receive bridged message from remote instance
pub async fn bridge_receive(
    State(state): State<AppState>,
    Json(body): Json<BridgeReceiveRequest>,
) -> Result<StatusCode, AppError> {
    let centrifugo = state
        .centrifugo
        .as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Centrifugo not configured")))?;

    centrifugo
        .publish(&body.channel, body.message)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Bridge receive failed: {e}")))?;

    Ok(StatusCode::OK)
}

/// Query params for Centrifugo token endpoint
#[derive(Deserialize)]
pub struct TokenQuery {
    #[serde(rename = "agentId")]
    pub agent_id: String,
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub token: String,
}

/// GET /api/intercom/centrifugo/token — get connection token for agent
pub async fn get_connection_token(
    State(state): State<AppState>,
    Query(params): Query<TokenQuery>,
) -> Result<Json<TokenResponse>, AppError> {
    if params.agent_id.is_empty() {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Query param 'agentId' is required"
        )));
    }

    // Generate JWT token for Centrifugo connection
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let token = state.jwt.issue(sera_auth::JwtClaims {
        sub: params.agent_id.clone(),
        iss: "sera".to_string(),
        exp: now_secs + 3600, // 1 hour
        agent_id: Some(params.agent_id.clone()),
        instance_id: None,
    }).map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to issue Centrifugo token: {e}")))?;

    Ok(Json(TokenResponse { token }))
}

/// Query params for subscription token endpoint
#[derive(Deserialize)]
pub struct SubscriptionQuery {
    pub channel: String,
}

/// GET /api/intercom/centrifugo/subscription — get subscription token for channel
pub async fn get_subscription_token(
    State(state): State<AppState>,
    Query(params): Query<SubscriptionQuery>,
) -> Result<Json<TokenResponse>, AppError> {
    if params.channel.is_empty() {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Query param 'channel' is required"
        )));
    }

    // Generate JWT token for Centrifugo channel subscription
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let token = state.jwt.issue(sera_auth::JwtClaims {
        sub: params.channel.clone(),
        iss: "sera".to_string(),
        exp: now_secs + 3600,
        agent_id: None,
        instance_id: None,
    }).map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to issue subscription token: {e}")))?;

    Ok(Json(TokenResponse { token }))
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

    fn ctx_for_bootstrap_api_key() -> ActingContext {
        ActingContext {
            operator_id: Some("bootstrap".to_string()),
            agent_id: None,
            instance_id: None,
            api_key_id: Some("bootstrap".to_string()),
            auth_method: AuthMethod::ApiKey,
        }
    }

    #[test]
    fn verify_agent_ownership_allows_matching_agent() {
        let ctx = ctx_for_agent("agent-a");
        assert!(verify_agent_ownership(&ctx, "agent-a").is_ok());
    }

    #[test]
    fn verify_agent_ownership_rejects_cross_agent_impersonation() {
        let ctx = ctx_for_agent("agent-a");
        let err = verify_agent_ownership(&ctx, "agent-b").unwrap_err();
        match err {
            AppError::Forbidden(msg) => {
                assert!(msg.contains("agent-a"), "msg must name caller: {msg}");
                assert!(msg.contains("agent-b"), "msg must name target: {msg}");
            }
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }

    #[test]
    fn verify_agent_ownership_allows_operator_proxy() {
        let ctx = ctx_for_operator();
        assert!(verify_agent_ownership(&ctx, "any-agent").is_ok());
    }

    #[test]
    fn verify_agent_ownership_allows_bootstrap_api_key_proxy() {
        // Dashboard + admin tooling use the bootstrap API key and must be able
        // to publish/DM as any agent.
        let ctx = ctx_for_bootstrap_api_key();
        assert!(verify_agent_ownership(&ctx, "any-agent").is_ok());
    }

    #[test]
    fn publish_request_deserializes() {
        let json = serde_json::json!({
            "agent": "agent-1",
            "channel": "notifications",
            "type": "message",
            "payload": {"text": "hello"}
        });
        let req: PublishRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.agent, "agent-1");
        assert_eq!(req.channel, "notifications");
        assert_eq!(req.message_type, "message");
    }

    #[test]
    fn dm_request_deserializes() {
        let json = serde_json::json!({
            "from": "agent-1",
            "to": "agent-2",
            "payload": {"text": "hello"}
        });
        let req: DmRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.from, "agent-1");
        assert_eq!(req.to, "agent-2");
    }
}
