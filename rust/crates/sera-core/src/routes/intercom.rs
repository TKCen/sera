//! Intercom messaging endpoints — publish to channels and send DMs via Centrifugo.

use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct PublishRequest {
    pub channel: String,
    pub data: serde_json::Value,
}

/// POST /api/intercom/publish — publish message to a Centrifugo channel
pub async fn publish(
    State(state): State<AppState>,
    Json(body): Json<PublishRequest>,
) -> Result<StatusCode, AppError> {
    let centrifugo = state
        .centrifugo
        .as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Centrifugo not configured")))?;

    centrifugo
        .publish(&body.channel, body.data)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Centrifugo publish failed: {e}")))?;

    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct DmRequest {
    pub from_agent_id: String,
    pub to_agent_id: String,
    pub data: serde_json::Value,
}

/// POST /api/intercom/dm — send direct message between agents
pub async fn dm(
    State(state): State<AppState>,
    Json(body): Json<DmRequest>,
) -> Result<StatusCode, AppError> {
    let centrifugo = state
        .centrifugo
        .as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Centrifugo not configured")))?;

    // DM channel format: agent:{to_agent_id}
    let channel = format!("agent:{}", body.to_agent_id);
    let message = serde_json::json!({
        "type": "dm",
        "from": body.from_agent_id,
        "to": body.to_agent_id,
        "data": body.data,
    });

    centrifugo
        .publish(&channel, message)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Centrifugo DM failed: {e}")))?;

    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_request_deserializes() {
        let json = serde_json::json!({
            "channel": "notifications",
            "data": {"message": "hello"}
        });
        let req: PublishRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.channel, "notifications");
        assert_eq!(req.data["message"], "hello");
    }

    #[test]
    fn dm_request_deserializes() {
        let json = serde_json::json!({
            "fromAgentId": "agent-1",
            "toAgentId": "agent-2",
            "data": {"text": "hello"}
        });
        let req: DmRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.from_agent_id, "agent-1");
        assert_eq!(req.to_agent_id, "agent-2");
        assert_eq!(req.data["text"], "hello");
    }
}
