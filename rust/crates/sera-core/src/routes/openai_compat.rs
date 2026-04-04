//! OpenAI-compatible chat completions endpoint.
//! Maps OpenAI API format to SERA agent processing.
#![allow(dead_code, unused_imports)]

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

/// OpenAI-compatible chat completion request.
#[derive(Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<OpenAIMessage>,
    #[serde(default)]
    pub stream: bool,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Option<Vec<String>>,
    pub user: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIMessage {
    pub role: String,
    pub content: String,
}

/// OpenAI-compatible chat completion response (non-streaming).
#[derive(Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Serialize)]
pub struct Choice {
    pub index: u32,
    pub message: OpenAIMessage,
    pub finish_reason: String,
}

#[derive(Serialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Streaming chunk.
#[derive(Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<StreamChoice>,
}

#[derive(Serialize)]
pub struct StreamChoice {
    pub index: u32,
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

#[derive(Serialize)]
pub struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// POST /v1/chat/completions — OpenAI-compatible endpoint
pub async fn chat_completions(
    State(state): State<AppState>,
    Json(body): Json<ChatCompletionRequest>,
) -> Result<Response, AppError> {
    let request_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let model = body.model.clone();

    // Find agent by model name (model maps to agent template)
    let agent_id = resolve_agent_for_model(&state, &body.model).await?;

    // Build the chat message from the last user message
    let last_user_msg = body
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();

    // Get agent chat URL
    let chat_url = super::chat::get_agent_chat_url(&state, &agent_id).await;

    if body.stream {
        let req_id = request_id.clone();
        let model_name = model.clone();

        match chat_url {
            Ok(url) => {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(300))
                    .build()
                    .unwrap_or_default();

                let resp = client
                    .post(format!("{url}/chat"))
                    .json(&serde_json::json!({
                        "message": last_user_msg,
                        "stream": true,
                        "context": body.messages,
                    }))
                    .send()
                    .await
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("Agent unavailable: {e}")))?;

                let byte_stream = resp.bytes_stream();

                let sse_stream = async_stream::stream! {
                    tokio::pin!(byte_stream);

                    // Send initial role chunk
                    let initial = ChatCompletionChunk {
                        id: req_id.clone(),
                        object: "chat.completion.chunk".to_string(),
                        created,
                        model: model_name.clone(),
                        choices: vec![StreamChoice {
                            index: 0,
                            delta: Delta { role: Some("assistant".to_string()), content: None },
                            finish_reason: None,
                        }],
                    };
                    yield Ok::<_, Infallible>(Event::default().data(serde_json::to_string(&initial).unwrap_or_default()));

                    while let Some(chunk) = byte_stream.next().await {
                        match chunk {
                            Ok(bytes) => {
                                let text = String::from_utf8_lossy(&bytes);
                                // Extract content from SSE data
                                for line in text.lines() {
                                    if let Some(data) = line.strip_prefix("data: ") {
                                        if data == "[DONE]" {
                                            break;
                                        }
                                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                                            let content = parsed.get("content")
                                                .or(parsed.get("text"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or_default();

                                            if !content.is_empty() {
                                                let chunk = ChatCompletionChunk {
                                                    id: req_id.clone(),
                                                    object: "chat.completion.chunk".to_string(),
                                                    created,
                                                    model: model_name.clone(),
                                                    choices: vec![StreamChoice {
                                                        index: 0,
                                                        delta: Delta { role: None, content: Some(content.to_string()) },
                                                        finish_reason: None,
                                                    }],
                                                };
                                                yield Ok(Event::default().data(serde_json::to_string(&chunk).unwrap_or_default()));
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("Stream error: {e}");
                                break;
                            }
                        }
                    }

                    // Send final chunk with finish_reason
                    let final_chunk = ChatCompletionChunk {
                        id: req_id.clone(),
                        object: "chat.completion.chunk".to_string(),
                        created,
                        model: model_name.clone(),
                        choices: vec![StreamChoice {
                            index: 0,
                            delta: Delta { role: None, content: None },
                            finish_reason: Some("stop".to_string()),
                        }],
                    };
                    yield Ok(Event::default().data(serde_json::to_string(&final_chunk).unwrap_or_default()));
                    yield Ok(Event::default().data("[DONE]"));
                };

                Ok(Sse::new(sse_stream)
                    .keep_alive(KeepAlive::default())
                    .into_response())
            }
            Err(_) => Err(AppError::Internal(anyhow::anyhow!(
                "No agent available for model: {}",
                model
            ))),
        }
    } else {
        // Non-streaming response
        match chat_url {
            Ok(url) => {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(300))
                    .build()
                    .unwrap_or_default();

                let resp = client
                    .post(format!("{url}/chat"))
                    .json(&serde_json::json!({
                        "message": last_user_msg,
                        "stream": false,
                        "context": body.messages,
                    }))
                    .send()
                    .await
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("Agent unavailable: {e}")))?;

                let response_body: serde_json::Value =
                    resp.json().await
                        .map_err(|e| AppError::Internal(anyhow::anyhow!("Invalid response: {e}")))?;

                let content = response_body
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();

                Ok(Json(ChatCompletionResponse {
                    id: request_id,
                    object: "chat.completion".to_string(),
                    created,
                    model,
                    choices: vec![Choice {
                        index: 0,
                        message: OpenAIMessage {
                            role: "assistant".to_string(),
                            content,
                        },
                        finish_reason: "stop".to_string(),
                    }],
                    usage: Usage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                })
                .into_response())
            }
            Err(_) => Err(AppError::Internal(anyhow::anyhow!(
                "No agent available for model: {}",
                model
            ))),
        }
    }
}

/// Resolve which agent to use for a given model name.
async fn resolve_agent_for_model(state: &AppState, model: &str) -> Result<String, AppError> {
    // Try to find a running agent whose template matches this model
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT ai.id FROM agent_instances ai
         JOIN agent_templates at ON ai.template_name = at.name
         WHERE ai.status = 'running'
         ORDER BY ai.created_at DESC LIMIT 1"
    )
    .fetch_optional(state.db.inner())
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("DB error: {e}")))?;

    match row {
        Some((id,)) => Ok(id.to_string()),
        None => Err(AppError::Internal(anyhow::anyhow!(
            "No running agent for model: {model}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_message_deserializes() {
        let msg = OpenAIMessage {
            role: "user".to_string(),
            content: "test".to_string(),
        };
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "test");
    }
}
