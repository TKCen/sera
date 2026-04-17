//! LLM client — calls the sera-core LLM proxy via reqwest.
//!
//! Supports both streaming (SSE) and non-streaming OpenAI-compatible chat completions.
//! Works with LM Studio, Ollama, OpenAI, and any OpenAI-compatible API.

use async_trait::async_trait;
use sera_types::runtime::TokenUsage;

use crate::config::RuntimeConfig;
use crate::turn::{LlmProvider, ThinkError, ThinkResult};
use crate::types::{ChatMessage, ToolCall, ToolCallFunction, ToolDefinition};
use futures_util::StreamExt;
use serde::Deserialize;
use std::collections::HashMap;
use tokio::time::Duration;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Classified LLM errors for retry / circuit-breaker logic.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("context overflow: {0}")]
    ContextOverflow(String),

    #[error("rate limited: {0}")]
    RateLimited(String),

    #[error("provider unavailable: {0}")]
    ProviderUnavailable(String),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("request error: {0}")]
    RequestError(String),
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Result of a chat completion request.
pub struct LlmChatResult {
    pub message: ChatMessage,
    #[allow(dead_code)]
    pub finish_reason: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

// ---------------------------------------------------------------------------
// SSE delta deserialization types (internal)
// ---------------------------------------------------------------------------

/// A single SSE chunk from the streaming API.
#[derive(Debug, Deserialize)]
struct SseChunk {
    choices: Option<Vec<SseChoice>>,
    usage: Option<SseUsage>,
}

#[derive(Debug, Deserialize)]
struct SseChoice {
    delta: Option<SseDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SseDelta {
    content: Option<String>,
    tool_calls: Option<Vec<SseToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct SseToolCallDelta {
    index: usize,
    id: Option<String>,
    function: Option<SseFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct SseFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SseUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

/// Accumulator for tool call fragments arriving across multiple SSE chunks.
#[derive(Debug, Default)]
struct ToolCallAccumulator {
    id: String,
    function_name: String,
    arguments: String,
}

// ---------------------------------------------------------------------------
// Non-streaming response types (internal)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NonStreamingResponse {
    choices: Vec<NonStreamingChoice>,
    #[serde(default)]
    usage: Option<SseUsage>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NonStreamingChoice {
    message: NonStreamingMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NonStreamingMessage {
    content: Option<String>,
    tool_calls: Option<Vec<NonStreamingToolCall>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NonStreamingToolCall {
    id: String,
    function: NonStreamingFunction,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NonStreamingFunction {
    name: String,
    arguments: String,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// HTTP client for the LLM proxy endpoint.
pub struct LlmClient {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
    max_tokens: u32,
    timeout: Duration,
}

impl LlmClient {
    pub fn new(config: &RuntimeConfig) -> Self {
        let timeout = Duration::from_secs(300);
        Self {
            client: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .unwrap_or_default(),
            base_url: config.llm_base_url.clone(),
            model: config.llm_model.clone(),
            api_key: config.llm_api_key.clone(),
            max_tokens: config.max_tokens,
            timeout,
        }
    }

    /// Build a new client with explicit parameters (useful for testing / non-config use).
    #[allow(dead_code)]
    pub fn with_params(
        base_url: &str,
        model: &str,
        api_key: Option<&str>,
        timeout_ms: u64,
    ) -> Self {
        let timeout = Duration::from_millis(timeout_ms);
        Self {
            client: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .unwrap_or_default(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            api_key: api_key.unwrap_or_default().to_string(),
            max_tokens: 4096,
            timeout,
        }
    }

    // ------------------------------------------------------------------
    // Streaming chat (SSE)
    // ------------------------------------------------------------------

    /// Send a chat completion request with streaming SSE.
    ///
    /// Reads `data:` lines from the response body, accumulates content and tool
    /// call deltas, and returns the complete result once `data: [DONE]` arrives
    /// or the stream ends.
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<LlmChatResult, LlmError> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": self.max_tokens,
            "stream": true,
        });

        if !tools.is_empty() {
            body["tools"] =
                serde_json::to_value(tools).map_err(|e| LlmError::RequestError(e.to_string()))?;
        }

        let response = self.send_request(&body).await?;

        // Parse the streaming SSE body
        self.parse_sse_stream(response).await
    }

    // ------------------------------------------------------------------
    // Non-streaming chat
    // ------------------------------------------------------------------

    /// Send a non-streaming chat completion request.
    #[allow(dead_code)]
    pub async fn chat_non_streaming(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<LlmChatResult, LlmError> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": self.max_tokens,
        });

        if !tools.is_empty() {
            body["tools"] =
                serde_json::to_value(tools).map_err(|e| LlmError::RequestError(e.to_string()))?;
        }

        let response = self.send_request(&body).await?;

        let text = response
            .text()
            .await
            .map_err(|e| LlmError::RequestError(format!("failed to read response body: {e}")))?;

        let parsed: NonStreamingResponse = serde_json::from_str(&text)
            .map_err(|e| LlmError::RequestError(format!("failed to parse response: {e}")))?;

        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::RequestError("empty choices in response".to_string()))?;

        let tool_calls = choice
            .message
            .tool_calls
            .map(|tcs| {
                tcs.into_iter()
                    .map(|tc| ToolCall {
                        id: tc.id,
                        call_type: "function".to_string(),
                        function: ToolCallFunction {
                            name: tc.function.name,
                            arguments: tc.function.arguments,
                        },
                    })
                    .collect::<Vec<_>>()
            });

        let usage = parsed.usage.unwrap_or_default();

        Ok(LlmChatResult {
            message: ChatMessage {
                role: "assistant".to_string(),
                content: choice.message.content,
                tool_calls,
                tool_call_id: None,
                name: None,
            },
            finish_reason: choice.finish_reason.unwrap_or_else(|| "stop".to_string()),
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
        })
    }

    // ------------------------------------------------------------------
    // Internals
    // ------------------------------------------------------------------

    /// Send the HTTP POST and classify any HTTP-level error.
    async fn send_request(
        &self,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response, LlmError> {
        let url = format!("{}/chat/completions", self.base_url);

        let result = tokio::time::timeout(self.timeout, async {
            self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(body)
                .send()
                .await
        })
        .await;

        let response = match result {
            Err(_) => {
                return Err(LlmError::Timeout(format!(
                    "request timed out after {:?}",
                    self.timeout
                )));
            }
            Ok(Err(e)) => {
                if e.is_timeout() {
                    return Err(LlmError::Timeout(e.to_string()));
                }
                return Err(LlmError::RequestError(e.to_string()));
            }
            Ok(Ok(resp)) => resp,
        };

        let status = response.status();

        if status.is_success() {
            return Ok(response);
        }

        // Read body for error classification
        let error_body = response.text().await.unwrap_or_default();

        classify_http_error(status.as_u16(), &error_body)
    }

    /// Parse an SSE stream into a complete `LlmChatResult`.
    async fn parse_sse_stream(
        &self,
        response: reqwest::Response,
    ) -> Result<LlmChatResult, LlmError> {
        let mut content = String::new();
        let mut tool_calls_map: HashMap<usize, ToolCallAccumulator> = HashMap::new();
        let mut usage = SseUsage::default();
        let mut finish_reason = String::from("stop");

        // Buffer for incomplete lines (SSE can split mid-line across chunks)
        let mut line_buffer = String::new();

        let mut stream = response.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result
                .map_err(|e| LlmError::RequestError(format!("stream read error: {e}")))?;

            let chunk_str = String::from_utf8_lossy(&chunk);
            line_buffer.push_str(&chunk_str);

            // Process complete lines
            while let Some(newline_pos) = line_buffer.find('\n') {
                let line = line_buffer[..newline_pos].trim_end_matches('\r').to_string();
                line_buffer = line_buffer[newline_pos + 1..].to_string();

                if line.is_empty() || line.starts_with(':') {
                    // SSE comment or empty line (event separator)
                    continue;
                }

                let data = if let Some(stripped) = line.strip_prefix("data: ") {
                    stripped.trim()
                } else if let Some(stripped) = line.strip_prefix("data:") {
                    stripped.trim()
                } else {
                    // Not a data line (could be "event:" etc.) — skip
                    continue;
                };

                if data == "[DONE]" {
                    // Stream complete — exit both loops by building tool_calls and returning early
                    let tool_calls = if tool_calls_map.is_empty() {
                        None
                    } else {
                        let mut sorted: Vec<(usize, ToolCallAccumulator)> =
                            tool_calls_map.into_iter().collect();
                        sorted.sort_by_key(|(idx, _)| *idx);

                        Some(
                            sorted
                                .into_iter()
                                .map(|(_, acc)| ToolCall {
                                    id: acc.id,
                                    call_type: "function".to_string(),
                                    function: ToolCallFunction {
                                        name: acc.function_name,
                                        arguments: acc.arguments,
                                    },
                                })
                                .collect(),
                        )
                    };

                    return Ok(LlmChatResult {
                        message: ChatMessage {
                            role: "assistant".to_string(),
                            content: if content.is_empty() {
                                None
                            } else {
                                Some(content)
                            },
                            tool_calls,
                            tool_call_id: None,
                            name: None,
                        },
                        finish_reason,
                        prompt_tokens: usage.prompt_tokens,
                        completion_tokens: usage.completion_tokens,
                    });
                }

                // Parse the JSON chunk
                let chunk: SseChunk = match serde_json::from_str(data) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::debug!("Skipping unparseable SSE data: {e}");
                        continue;
                    }
                };

                // Accumulate usage if present (usually on the final chunk)
                if let Some(u) = chunk.usage {
                    usage = u;
                }

                // Process choices
                if let Some(choices) = chunk.choices {
                    for choice in choices {
                        if let Some(fr) = choice.finish_reason {
                            finish_reason = fr;
                        }
                        if let Some(delta) = choice.delta {
                            // Content delta
                            if let Some(c) = delta.content {
                                content.push_str(&c);
                            }
                            // Tool call deltas
                            if let Some(tc_deltas) = delta.tool_calls {
                                for tc_delta in tc_deltas {
                                    let acc = tool_calls_map
                                        .entry(tc_delta.index)
                                        .or_default();
                                    if let Some(id) = tc_delta.id {
                                        acc.id = id;
                                    }
                                    if let Some(func) = tc_delta.function {
                                        if let Some(name) = func.name {
                                            acc.function_name = name;
                                        }
                                        if let Some(args) = func.arguments {
                                            acc.arguments.push_str(&args);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Build final tool calls sorted by index
        let tool_calls = if tool_calls_map.is_empty() {
            None
        } else {
            let mut sorted: Vec<(usize, ToolCallAccumulator)> =
                tool_calls_map.into_iter().collect();
            sorted.sort_by_key(|(idx, _)| *idx);

            Some(
                sorted
                    .into_iter()
                    .map(|(_, acc)| ToolCall {
                        id: acc.id,
                        call_type: "function".to_string(),
                        function: ToolCallFunction {
                            name: acc.function_name,
                            arguments: acc.arguments,
                        },
                    })
                    .collect(),
            )
        };

        Ok(LlmChatResult {
            message: ChatMessage {
                role: "assistant".to_string(),
                content: if content.is_empty() {
                    None
                } else {
                    Some(content)
                },
                tool_calls,
                tool_call_id: None,
                name: None,
            },
            finish_reason,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
        })
    }
}

// ---------------------------------------------------------------------------
// LlmProvider implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl LlmProvider for LlmClient {
    async fn chat(
        &self,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
    ) -> Result<ThinkResult, ThinkError> {
        // Convert Value messages to ChatMessage
        let chat_messages: Vec<ChatMessage> = messages
            .iter()
            .map(|m| serde_json::from_value(m.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ThinkError::Conversion(format!("message deserialization: {e}")))?;

        // Convert Value tools to ToolDefinition
        let tool_defs: Vec<ToolDefinition> = tools
            .iter()
            .map(|t| serde_json::from_value(t.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ThinkError::Conversion(format!("tool deserialization: {e}")))?;

        // Call the LLM
        let result = LlmClient::chat(self, &chat_messages, &tool_defs)
            .await
            .map_err(|e| ThinkError::Llm(e.to_string()))?;

        // Convert tool calls to Value
        let tool_calls: Vec<serde_json::Value> = result
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| {
                serde_json::json!({
                    "id": tc.id,
                    "type": tc.call_type,
                    "function": {
                        "name": tc.function.name,
                        "arguments": tc.function.arguments,
                    }
                })
            })
            .collect();

        Ok(ThinkResult {
            response: serde_json::json!({
                "role": "assistant",
                "content": result.message.content,
            }),
            tool_calls,
            tokens: TokenUsage {
                prompt_tokens: result.prompt_tokens,
                completion_tokens: result.completion_tokens,
                total_tokens: result.prompt_tokens + result.completion_tokens,
            },
        })
    }
}

// ---------------------------------------------------------------------------
// Error classification
// ---------------------------------------------------------------------------

/// Classify an HTTP error into the appropriate `LlmError` variant.
fn classify_http_error(status: u16, body: &str) -> Result<reqwest::Response, LlmError> {
    let lower_body = body.to_lowercase();

    match status {
        429 => Err(LlmError::RateLimited(truncate_error(body))),
        503 => Err(LlmError::ProviderUnavailable(truncate_error(body))),
        400 if lower_body.contains("context_length_exceeded")
            || lower_body.contains("maximum context length")
            || lower_body.contains("context window")
            || lower_body.contains("too many tokens") =>
        {
            Err(LlmError::ContextOverflow(truncate_error(body)))
        }
        _ => Err(LlmError::RequestError(format!(
            "HTTP {status}: {}",
            truncate_error(body)
        ))),
    }
}

/// Truncate long error bodies for display.
fn truncate_error(body: &str) -> String {
    if body.len() > 500 {
        format!("{}...", &body[..500])
    } else {
        body.to_string()
    }
}

// ---------------------------------------------------------------------------
// Unit-level helpers for testing SSE parsing
// ---------------------------------------------------------------------------

/// Parse a single SSE `data:` line into an `SseChunk`. Exposed for testing.
#[cfg(test)]
fn parse_sse_data_line(line: &str) -> Option<SseChunk> {
    let data = line
        .strip_prefix("data: ")
        .or_else(|| line.strip_prefix("data:"))?
        .trim();

    if data == "[DONE]" {
        return None;
    }

    serde_json::from_str(data).ok()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ----- SSE line parsing -----

    #[test]
    fn test_parse_sse_content_delta() {
        let line = r#"data: {"choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        let chunk = parse_sse_data_line(line).expect("should parse");
        let choices = chunk.choices.unwrap();
        let delta = choices[0].delta.as_ref().unwrap();
        assert_eq!(delta.content.as_deref(), Some("Hello"));
    }

    #[test]
    fn test_parse_sse_tool_call_delta() {
        let line = r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_abc","function":{"name":"read_file","arguments":""}}]},"finish_reason":null}]}"#;
        let chunk = parse_sse_data_line(line).expect("should parse");
        let choices = chunk.choices.unwrap();
        let delta = choices[0].delta.as_ref().unwrap();
        let tc = &delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.index, 0);
        assert_eq!(tc.id.as_deref(), Some("call_abc"));
        assert_eq!(
            tc.function.as_ref().unwrap().name.as_deref(),
            Some("read_file")
        );
    }

    #[test]
    fn test_parse_sse_done() {
        let result = parse_sse_data_line("data: [DONE]");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_sse_with_usage() {
        let line = r#"data: {"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":50}}"#;
        let chunk = parse_sse_data_line(line).expect("should parse");
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
    }

    #[test]
    fn test_parse_sse_malformed_line() {
        let result = parse_sse_data_line("data: {invalid json}");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_sse_not_data_line() {
        let result = parse_sse_data_line("event: ping");
        assert!(result.is_none());
    }

    // ----- Error classification -----

    #[test]
    fn test_error_rate_limited() {
        let err = classify_http_error(429, "rate limit exceeded").unwrap_err();
        assert!(matches!(err, LlmError::RateLimited(_)));
        assert!(err.to_string().contains("rate limit exceeded"));
    }

    #[test]
    fn test_error_provider_unavailable() {
        let err = classify_http_error(503, "service unavailable").unwrap_err();
        assert!(matches!(err, LlmError::ProviderUnavailable(_)));
    }

    #[test]
    fn test_error_context_overflow_keyword1() {
        let err = classify_http_error(
            400,
            r#"{"error":{"message":"This model's maximum context length is 8192 tokens"}}"#,
        )
        .unwrap_err();
        assert!(matches!(err, LlmError::ContextOverflow(_)));
    }

    #[test]
    fn test_error_context_overflow_keyword2() {
        let err = classify_http_error(
            400,
            r#"{"error":{"code":"context_length_exceeded","message":"too long"}}"#,
        )
        .unwrap_err();
        assert!(matches!(err, LlmError::ContextOverflow(_)));
    }

    #[test]
    fn test_error_context_overflow_keyword3() {
        let err = classify_http_error(400, "too many tokens in the request").unwrap_err();
        assert!(matches!(err, LlmError::ContextOverflow(_)));
    }

    #[test]
    fn test_error_generic_400() {
        let err = classify_http_error(400, "bad request: missing field").unwrap_err();
        assert!(matches!(err, LlmError::RequestError(_)));
    }

    #[test]
    fn test_error_generic_500() {
        let err = classify_http_error(500, "internal server error").unwrap_err();
        assert!(matches!(err, LlmError::RequestError(_)));
        assert!(err.to_string().contains("HTTP 500"));
    }

    #[test]
    fn test_truncate_error_short() {
        let short = "short error";
        assert_eq!(truncate_error(short), short);
    }

    #[test]
    fn test_truncate_error_long() {
        let long = "x".repeat(600);
        let truncated = truncate_error(&long);
        assert!(truncated.len() < 600);
        assert!(truncated.ends_with("..."));
    }

    // ----- Tool call accumulation -----

    #[test]
    fn test_tool_call_accumulation() {
        // Simulate multiple SSE chunks building up a tool call
        let mut tool_calls_map: HashMap<usize, ToolCallAccumulator> = HashMap::new();

        // Chunk 1: tool call start with id and name
        let chunk1_data = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_123","function":{"name":"read_file","arguments":""}}]},"finish_reason":null}]}"#;
        let chunk1: SseChunk = serde_json::from_str(chunk1_data).unwrap();
        apply_chunk_tool_calls(&chunk1, &mut tool_calls_map);

        // Chunk 2: argument fragment 1
        let chunk2_data = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":"}}]},"finish_reason":null}]}"#;
        let chunk2: SseChunk = serde_json::from_str(chunk2_data).unwrap();
        apply_chunk_tool_calls(&chunk2, &mut tool_calls_map);

        // Chunk 3: argument fragment 2
        let chunk3_data = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"/tmp/test\"}"}}]},"finish_reason":null}]}"#;
        let chunk3: SseChunk = serde_json::from_str(chunk3_data).unwrap();
        apply_chunk_tool_calls(&chunk3, &mut tool_calls_map);

        assert_eq!(tool_calls_map.len(), 1);
        let acc = tool_calls_map.get(&0).unwrap();
        assert_eq!(acc.id, "call_123");
        assert_eq!(acc.function_name, "read_file");
        assert_eq!(acc.arguments, r#"{"path":"/tmp/test"}"#);
    }

    #[test]
    fn test_multiple_tool_calls_accumulation() {
        let mut tool_calls_map: HashMap<usize, ToolCallAccumulator> = HashMap::new();

        // Two tool calls in parallel at different indices
        let chunk_data = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"read_file","arguments":"{\"path\":\"a\"}"}},{"index":1,"id":"call_b","function":{"name":"write_file","arguments":"{\"path\":\"b\"}"}}]},"finish_reason":null}]}"#;
        let chunk: SseChunk = serde_json::from_str(chunk_data).unwrap();
        apply_chunk_tool_calls(&chunk, &mut tool_calls_map);

        assert_eq!(tool_calls_map.len(), 2);
        assert_eq!(tool_calls_map[&0].function_name, "read_file");
        assert_eq!(tool_calls_map[&1].function_name, "write_file");
    }

    #[test]
    fn test_non_streaming_response_parsing() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": "Hello world",
                    "tool_calls": null
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5
            }
        }"#;

        let parsed: NonStreamingResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.choices.len(), 1);
        assert_eq!(
            parsed.choices[0].message.content.as_deref(),
            Some("Hello world")
        );
        assert_eq!(
            parsed.choices[0].finish_reason.as_deref(),
            Some("stop")
        );
        let usage = parsed.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
    }

    #[test]
    fn test_non_streaming_response_with_tool_calls() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_xyz",
                        "type": "function",
                        "function": {
                            "name": "shell_exec",
                            "arguments": "{\"command\":\"ls\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 8
            }
        }"#;

        let parsed: NonStreamingResponse = serde_json::from_str(json).unwrap();
        let tc = &parsed.choices[0].message.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.id, "call_xyz");
        assert_eq!(tc.function.name, "shell_exec");
        assert_eq!(tc.function.arguments, r#"{"command":"ls"}"#);
    }

    // ----- LlmError Display -----

    #[test]
    fn test_error_display() {
        let err = LlmError::ContextOverflow("too big".to_string());
        assert_eq!(err.to_string(), "context overflow: too big");

        let err = LlmError::RateLimited("slow down".to_string());
        assert_eq!(err.to_string(), "rate limited: slow down");

        let err = LlmError::ProviderUnavailable("down".to_string());
        assert_eq!(err.to_string(), "provider unavailable: down");

        let err = LlmError::Timeout("30s".to_string());
        assert_eq!(err.to_string(), "timeout: 30s");

        let err = LlmError::RequestError("oops".to_string());
        assert_eq!(err.to_string(), "request error: oops");
    }

    // Helper to simulate applying tool call deltas from an SSE chunk
    fn apply_chunk_tool_calls(
        chunk: &SseChunk,
        map: &mut HashMap<usize, ToolCallAccumulator>,
    ) {
        if let Some(choices) = &chunk.choices {
            for choice in choices {
                if let Some(delta) = &choice.delta
                    && let Some(tc_deltas) = &delta.tool_calls
                {
                    for tc_delta in tc_deltas {
                        let acc = map.entry(tc_delta.index).or_default();
                        if let Some(id) = &tc_delta.id {
                            acc.id = id.clone();
                        }
                        if let Some(func) = &tc_delta.function {
                            if let Some(name) = &func.name {
                                acc.function_name = name.clone();
                            }
                            if let Some(args) = &func.arguments {
                                acc.arguments.push_str(args);
                            }
                        }
                    }
                }
            }
        }
    }
}
