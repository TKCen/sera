//! Chat endpoint — routes messages to agent containers via their chat server.
#![allow(dead_code, unused_imports, unused_variables, clippy::too_many_arguments)]

use axum::{
    extract::{Path, Query, State},
    response::{sse::{Event, KeepAlive, Sse}, IntoResponse, Response},
    http::StatusCode,
    Json,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;

use crate::error::AppError;
use crate::state::AppState;
use sera_db::sessions::SessionRepository;

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
        let agent_id_clone = agent_id.clone();

        tokio::spawn(async move {
            if let Err(e) = process_chat_background(
                &state_clone,
                &chat_url_clone,
                &message_clone,
                &session_clone,
                context_clone,
                &msg_id_clone,
                &agent_id_clone,
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

/// Background task to process chat in stream mode.
/// Sends the message to the agent container, then publishes the response
/// to Centrifugo so the web UI receives it via the `tokens:{agentId}` channel.
async fn process_chat_background(
    state: &AppState,
    chat_url: &str,
    message: &str,
    session_id: &str,
    context: Vec<ChatMessage>,
    message_id: &str,
    agent_id: &str,
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

    let response_body: ContainerChatResponse = resp.json().await?;
    let reply = response_body.result.unwrap_or_else(|| "No response generated.".to_string());

    // Publish response to Centrifugo for the web UI token stream
    if let Some(centrifugo) = &state.centrifugo {
        let channel = format!("tokens:{}", agent_id);

        // Send the complete reply as a single token
        centrifugo
            .publish(
                &channel,
                serde_json::json!({
                    "token": reply,
                    "done": false,
                    "messageId": message_id,
                }),
            )
            .await
            .unwrap_or_else(|e| tracing::error!("Centrifugo publish error: {e}"));

        // Send done signal
        centrifugo
            .publish(
                &channel,
                serde_json::json!({
                    "token": "",
                    "done": true,
                    "messageId": message_id,
                }),
            )
            .await
            .unwrap_or_else(|e| tracing::error!("Centrifugo done signal error: {e}"));
    } else {
        tracing::warn!("Centrifugo client not configured — stream response not delivered");
    }

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
            // Container naming: sera-agent-{name}-{instance_id[..8]}
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

// ============================================================================
// Chat Session Message Routes
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddMessageRequest {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageResponse {
    pub id: String,
    pub role: String,
    pub content: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagesQuery {
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

/// POST /api/chat/sessions/:id/messages — add a message to a session
pub async fn add_message(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<AddMessageRequest>,
) -> Result<(StatusCode, Json<MessageResponse>), AppError> {
    let message_id = uuid::Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO chat_messages (id, session_id, role, content, metadata, created_at)
         VALUES ($1::uuid, $2::uuid, $3, $4, $5, NOW())"
    )
    .bind(&message_id)
    .bind(&session_id)
    .bind(&body.role)
    .bind(&body.content)
    .bind(&body.metadata)
    .execute(state.db.inner())
    .await
    .map_err(|e| {
        if e.to_string().contains("foreign key") {
            AppError::Db(sera_db::DbError::NotFound {
                entity: "session",
                key: "id",
                value: session_id.clone(),
            })
        } else {
            AppError::Internal(anyhow::anyhow!("Failed to insert message: {e}"))
        }
    })?;

    let row: (Option<String>,) = sqlx::query_as(
        "SELECT created_at AT TIME ZONE 'UTC' FROM chat_messages WHERE id = $1::uuid"
    )
    .bind(&message_id)
    .fetch_one(state.db.inner())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to fetch created_at: {e}")))?;

    use super::iso8601_opt;
    Ok((
        StatusCode::CREATED,
        Json(MessageResponse {
            id: message_id,
            role: body.role,
            content: Some(body.content),
            metadata: body.metadata,
            created_at: iso8601_opt(
                row.0.as_deref().and_then(|s| time::OffsetDateTime::parse(s, &time::format_description::well_known::Iso8601::DEFAULT).ok())
            ),
        }),
    ))
}

/// GET /api/chat/sessions/:id/messages — list messages with pagination
pub async fn list_messages(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(params): Query<MessagesQuery>,
) -> Result<Json<Vec<MessageResponse>>, AppError> {
    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0).max(0);

    let rows = sqlx::query_as::<_, (uuid::Uuid, uuid::Uuid, String, Option<String>, Option<serde_json::Value>, Option<time::OffsetDateTime>)>(
        "SELECT id, session_id, role, content, metadata, created_at
         FROM chat_messages WHERE session_id = $1::uuid
         ORDER BY created_at ASC
         LIMIT $2 OFFSET $3"
    )
    .bind(&session_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(state.db.inner())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to fetch messages: {e}")))?;

    use super::iso8601_opt;
    let messages = rows
        .into_iter()
        .map(|(id, _session_id, role, content, metadata, created_at)| MessageResponse {
            id: id.to_string(),
            role,
            content,
            metadata,
            created_at: iso8601_opt(created_at),
        })
        .collect();

    Ok(Json(messages))
}

// ============================================================================
// Chat Streaming & Completion Stubs
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamEventPayload {
    pub session_id: String,
    pub message_id: String,
    pub delta: Option<String>,
}

/// POST /api/chat/stream — SSE streaming stub
pub async fn stream_chat(
    State(_state): State<AppState>,
) -> Sse<impl futures_util::stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = futures_util::stream::iter(vec![
        Ok(Event::default()
            .event("message")
            .data(serde_json::to_string(&StreamEventPayload {
                session_id: uuid::Uuid::new_v4().to_string(),
                message_id: uuid::Uuid::new_v4().to_string(),
                delta: Some("This is a streaming response placeholder.".to_string()),
            }).unwrap_or_default())),
        Ok(Event::default()
            .event("done")
            .data(serde_json::json!({"status": "complete"}).to_string())),
    ]);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// POST /api/chat/completions — non-streaming completion stub
pub async fn completions(
    State(_state): State<AppState>,
    Json(_body): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, AppError> {
    Ok(Json(ChatResponse {
        reply: Some("This is a completion stub response.".to_string()),
        thought: None,
        thoughts: None,
        citations: None,
        session_id: uuid::Uuid::new_v4().to_string(),
        message_id: Some(uuid::Uuid::new_v4().to_string()),
        usage: None,
    }))
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

    #[test]
    fn add_message_request_deserializes() {
        let json = r#"{"role":"assistant","content":"test response"}"#;
        let req: AddMessageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.role, "assistant");
        assert_eq!(req.content, "test response");
        assert_eq!(req.metadata, None);
    }

    #[test]
    fn add_message_request_with_metadata() {
        let json = r#"{"role":"user","content":"hi","metadata":{"key":"value"}}"#;
        let req: AddMessageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.role, "user");
        assert_eq!(req.content, "hi");
        assert!(req.metadata.is_some());
    }

    #[test]
    fn stream_event_payload_serializes() {
        let event = StreamEventPayload {
            session_id: "session-123".to_string(),
            message_id: "msg-456".to_string(),
            delta: Some("hello".to_string()),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("sessionId"));
        assert!(json.contains("messageId"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn chat_response_serializes() {
        let resp = ChatResponse {
            reply: Some("test".to_string()),
            thought: None,
            thoughts: None,
            citations: None,
            session_id: "sess-1".to_string(),
            message_id: Some("msg-1".to_string()),
            usage: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("sess-1"));
    }
}
