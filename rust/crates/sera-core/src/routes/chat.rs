//! Chat endpoint — routes messages to agent containers via their chat server.

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
    pub agent_id: String,
    pub message: String,
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
pub struct ChatResponse {
    pub message: ChatMessage,
    pub session_id: String,
    pub usage: Option<UsageInfo>,
}

#[derive(Serialize)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// POST /api/chat — route chat message to agent container
pub async fn chat(
    State(state): State<AppState>,
    Json(body): Json<ChatRequest>,
) -> Result<Response, AppError> {
    // Look up agent's container chat URL from DB
    let chat_url = get_agent_chat_url(&state, &body.agent_id).await?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_default();

    if body.stream {
        // Stream SSE from container to client
        let resp = client
            .post(&format!("{chat_url}/chat"))
            .json(&serde_json::json!({
                "message": body.message,
                "sessionId": body.session_id,
                "stream": true,
                "context": body.context,
            }))
            .send()
            .await
            .map_err(|e| {
                tracing::error!(agent_id = %body.agent_id, error = %e, "Container chat unreachable");
                AppError::Internal(anyhow::anyhow!("Agent container unavailable"))
            })?;

        if resp.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            return Err(AppError::Internal(anyhow::anyhow!(
                "Agent container not ready"
            )));
        }

        let byte_stream = resp.bytes_stream();

        let sse_stream = async_stream::stream! {
            let mut buffer = String::new();
            tokio::pin!(byte_stream);

            while let Some(chunk) = byte_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));

                        // Parse SSE events from buffer
                        while let Some(pos) = buffer.find("\n\n") {
                            let event_str = buffer[..pos].to_string();
                            buffer = buffer[pos + 2..].to_string();

                            // Forward the SSE event
                            if let Some(data) = event_str.strip_prefix("data: ") {
                                yield Ok::<_, Infallible>(Event::default().data(data.to_string()));
                            } else {
                                yield Ok::<_, Infallible>(Event::default().data(event_str));
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Stream error: {e}");
                        yield Ok(Event::default().data(
                            serde_json::json!({"error": e.to_string()}).to_string()
                        ));
                        break;
                    }
                }
            }

            // Send done event
            yield Ok(Event::default().data("[DONE]"));
        };

        Ok(Sse::new(sse_stream)
            .keep_alive(KeepAlive::default())
            .into_response())
    } else {
        // Non-streaming: proxy request and return full response
        let resp = client
            .post(&format!("{chat_url}/chat"))
            .json(&serde_json::json!({
                "message": body.message,
                "sessionId": body.session_id,
                "stream": false,
                "context": body.context,
            }))
            .send()
            .await
            .map_err(|e| {
                tracing::error!(agent_id = %body.agent_id, error = %e, "Container chat unreachable");
                AppError::Internal(anyhow::anyhow!("Agent container unavailable"))
            })?;

        if resp.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            return Err(AppError::Internal(anyhow::anyhow!(
                "Agent container not ready"
            )));
        }

        let response_body: serde_json::Value = resp.json().await.map_err(|e| {
            AppError::Internal(anyhow::anyhow!("Invalid container response: {e}"))
        })?;

        Ok(Json(response_body).into_response())
    }
}

/// Look up the chat URL for an agent's running container.
pub(crate) async fn get_agent_chat_url(state: &AppState, agent_id: &str) -> Result<String, AppError> {
    let row: Option<(Option<String>, Option<i32>)> = sqlx::query_as(
        "SELECT container_id, chat_port FROM agent_instances WHERE id = $1::uuid AND status = 'running'"
    )
    .bind(agent_id)
    .fetch_optional(state.db.inner())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("DB error looking up agent: {e}")))?;

    match row {
        Some((Some(container_id), Some(port))) => {
            // Use container name/id on sera_net
            let container_name = format!("sera-agent-{}", &container_id[..8.min(container_id.len())]);
            Ok(format!("http://{}:{}", container_name, port))
        }
        Some(_) => Err(AppError::Internal(anyhow::anyhow!(
            "Agent has no running container"
        ))),
        None => {
            // Return 503 for unavailable agents
            Err(AppError::Internal(anyhow::anyhow!(
                "Agent not found or not running"
            )))
        }
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
