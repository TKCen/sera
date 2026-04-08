//! Intercom messaging endpoints — publish to channels and send DMs via Centrifugo.
#![allow(dead_code, unused_imports)]

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

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
    Json(body): Json<PublishRequest>,
) -> Result<Json<PublishResponse>, AppError> {
    // TODO: Resolve manifest for agent authorization
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
    Json(body): Json<DmRequest>,
) -> Result<Json<DmResponse>, AppError> {
    // TODO: Resolve manifest for from agent authorization
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

    // TODO: Fetch from Centrifugo history API
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

    // TODO: Resolve manifest and return agent's authorized channels
    Ok(Json(ChannelsResponse {
        channels: vec![],
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

    let centrifugo = state
        .centrifugo
        .as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Centrifugo not configured")))?;

    // Generate JWT token for Centrifugo connection
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let token = centrifugo
        .generate_connection_token(&params.agent_id, now_secs + 3600)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to issue Centrifugo token: {e}")))?;

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

    let centrifugo = state
        .centrifugo
        .as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Centrifugo not configured")))?;

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // In a real scenario, we'd get the current user/agent from the auth context.
    // For now, use a generic "operator" subject for subscription tokens.
    let token = centrifugo
        .generate_subscription_token("operator", &params.channel, now_secs + 3600)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to issue subscription token: {e}")))?;

    Ok(Json(TokenResponse { token }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
