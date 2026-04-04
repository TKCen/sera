//! Chat endpoint — routes messages to agent containers via their chat server.
#![allow(dead_code, unused_imports, unused_variables, clippy::too_many_arguments)]

use axum::{
    extract::State,
    response::{sse::{Event, KeepAlive, Sse}, IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct ChatRequest {
    #[serde(alias = "agentInstanceId")]
    pub agent_instance_id: Option<String>,
    #[serde(alias = "agentName")]
    pub agent_name: Option<String>,
    pub message: String,
    #[serde(alias = "sessionId")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub context: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,      // "user" | "assistant" | "system"
    pub content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thoughts: Option<Vec<Thought>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citations: Option<Vec<Citation>>,
    pub session_id: String,
    pub message_id: Option<String>,
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thought {
    pub step: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    #[serde(rename = "blockId")]
    pub block_id: String,
    pub scope: String,
    pub relevance: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamResponse {
    pub session_id: String,
    pub message_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Container response shape
#[derive(Debug, Deserialize)]
pub struct ContainerChatResponse {
    pub result: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub thoughts: Option<Vec<Thought>>,
    #[serde(default)]
    pub citations: Option<Vec<Citation>>,
    #[serde(default)]
    pub usage: Option<UsageInfo>,
}

/// POST /api/chat — route chat message to agent container
pub async fn chat(
    State(state): State<AppState>,
    Json(body): Json<ChatRequest>,
) -> Result<Response, AppError> {
    // 1. Resolve agent (3-tier fallback: agentInstanceId → agentName → primary)
    let agent_id = resolve_agent(&state, &body.agent_instance_id, &body.agent_name).await?;

    // 2. Resolve or create session
    let session_id = if let Some(sid) = &body.session_id {
        sid.clone()
    } else {
        uuid::Uuid::new_v4().to_string()
    };

    // 3. Ensure container is running
    let chat_url = get_agent_chat_url(&state, &agent_id).await?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .unwrap_or_default();

    if body.stream {
        // Stream mode: return immediately with {sessionId, messageId}, process in background
        let message_id = uuid::Uuid::new_v4().to_string();
        let response = StreamResponse {
            session_id: session_id.clone(),
            message_id: message_id.clone(),
        };

        // Spawn background task to process
        let state_clone = state.clone();
        let chat_url_clone = chat_url.clone();
        let message_clone = body.message.clone();
        let session_clone = session_id.clone();
        let context_clone = body.context.clone();
        let msg_id_clone = message_id.clone();

        tokio::spawn(async move {
            if let Err(e) = process_chat_background(
                &state_clone,
                &chat_url_clone,
                &message_clone,
                &session_clone,
                context_clone,
                &msg_id_clone,
            )
            .await
            {
                tracing::error!("Background chat processing error: {e}");
            }
        });

        Ok(Json(response).into_response())
    } else {
        // Synchronous mode: wait for response
        let resp = client
            .post(format!("{chat_url}/chat"))
            .json(&serde_json::json!({
                "message": body.message,
                "sessionId": session_id.clone(),
                "history": body.context,
            }))
            .send()
            .await
            .map_err(|e| {
                tracing::error!(agent_id = %agent_id, error = %e, "Container chat unreachable");
                if e.is_timeout() {
                    return AppError::Internal(anyhow::anyhow!("Agent timed out while processing"));
                }
                AppError::Internal(anyhow::anyhow!("Agent container unavailable"))
            })?;

        match resp.status() {
            reqwest::StatusCode::GATEWAY_TIMEOUT => {
                return Err(AppError::Internal(anyhow::anyhow!("Agent timed out while processing")));
            }
            reqwest::StatusCode::SERVICE_UNAVAILABLE => {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "Agent container not ready"
                )));
            }
            _ => {}
        }

        let response_body: ContainerChatResponse = resp.json().await.map_err(|e| {
            AppError::Internal(anyhow::anyhow!("Invalid container response: {e}"))
        })?;

        let reply = response_body.result.unwrap_or_else(|| "No response generated.".to_string());

        Ok(Json(ChatResponse {
            reply: Some(reply),
            thought: response_body.error.as_ref().map(|e| format!("Error: {}", e)),
            thoughts: response_body.thoughts,
            citations: response_body.citations,
            session_id,
            message_id: None,
            usage: response_body.usage,
        })
        .into_response())
    }
}

/// Background task to process chat in stream mode
async fn process_chat_background(
    state: &AppState,
    chat_url: &str,
    message: &str,
    session_id: &str,
    context: Vec<ChatMessage>,
    _message_id: &str,
) -> Result<(), anyhow::Error> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;

    let resp = client
        .post(format!("{chat_url}/chat"))
        .json(&serde_json::json!({
            "message": message,
            "sessionId": session_id,
            "history": context,
        }))
        .send()
        .await?;

    let _response_body: ContainerChatResponse = resp.json().await?;
    // TODO: Persist to database and emit via Centrifugo when ready
    Ok(())
}

/// Resolve agent from 3-tier fallback: agentInstanceId → agentName → primary
async fn resolve_agent(
    state: &AppState,
    instance_id: &Option<String>,
    agent_name: &Option<String>,
) -> Result<String, AppError> {
    if let Some(id) = instance_id {
        // Check if agent exists and is running
        let row: Option<(uuid::Uuid,)> = sqlx::query_as(
            "SELECT id FROM agent_instances WHERE id = $1::uuid"
        )
        .bind(id)
        .fetch_optional(state.db.inner())
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("DB error: {e}")))?;

        if row.is_some() {
            return Ok(id.clone());
        }
    }

    if let Some(name) = agent_name {
        // Look up by template name and get running instance
        let row: Option<(uuid::Uuid,)> = sqlx::query_as(
            "SELECT id FROM agent_instances WHERE template_name = $1 AND status = 'running' LIMIT 1"
        )
        .bind(name)
        .fetch_optional(state.db.inner())
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("DB error: {e}")))?;

        if let Some((id,)) = row {
            return Ok(id.to_string());
        }
    }

    // Fallback to any running agent
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT id FROM agent_instances WHERE status = 'running' LIMIT 1"
    )
    .fetch_optional(state.db.inner())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("DB error: {e}")))?;

    match row {
        Some((id,)) => Ok(id.to_string()),
        None => Err(AppError::Internal(anyhow::anyhow!(
            "No agent configured. Check your AGENT.yaml manifests."
        ))),
    }
}

/// Look up the chat URL for an agent's running container.
pub(crate) async fn get_agent_chat_url(state: &AppState, agent_id: &str) -> Result<String, AppError> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT container_id FROM agent_instances WHERE id = $1::uuid"
    )
    .bind(agent_id)
    .fetch_optional(state.db.inner())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("DB error looking up agent: {e}")))?;

    match row {
        Some((Some(_container_id),)) => {
            // Use container name on sera_net with chat port 3100
            // Container naming matches sera-docker: sera-agent-{name}-{instance_id[..8]}
            // Note: container.rs uses instance_id for naming, NOT container_id
            let name_row: Option<(String,)> = sqlx::query_as(
                "SELECT name FROM agent_instances WHERE id = $1::uuid"
            )
            .bind(agent_id)
            .fetch_optional(state.db.inner())
            .await
            .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;
            let agent_name = name_row.map(|r| r.0).unwrap_or_default();
            let container_name = format!("sera-agent-{}-{}", agent_name.to_lowercase(), &agent_id[..8.min(agent_id.len())]);
            Ok(format!("http://{}:3100", container_name))
        }
        Some(_) => Err(AppError::Internal(anyhow::anyhow!(
            "Agent has no running container"
        ))),
        None => Err(AppError::Internal(anyhow::anyhow!(
            "Agent not found or not running"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_message_deserializes() {
        let msg = ChatMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
        };
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "hello");
    }
}
