//! LLM client — calls the sera-core LLM proxy via reqwest.
//!
//! Supports both streaming (SSE) and non-streaming OpenAI-compatible chat completions.
//! Works with LM Studio, Ollama, OpenAI, and any OpenAI-compatible API.

use async_trait::async_trait;
use sera_models::{AccountPool, ProviderKind, ReasoningLevel, ThinkingConfig};
use sera_telemetry::{record_credential_outcome, CredentialOutcome};
use sera_types::llm::ThinkingLevel;
use sera_types::runtime::TokenUsage;
use sera_types::tool::ToolUseBehavior;

use crate::config::RuntimeConfig;
use crate::turn::{LlmProvider, ThinkError, ThinkResult};
use crate::types::{ChatMessage, ToolCall, ToolCallFunction, ToolDefinition};
use futures_util::StreamExt;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::Duration;

// ---------------------------------------------------------------------------
// Module-level constants
// ---------------------------------------------------------------------------

/// Default LLM request timeout (5 minutes), shared across client, delegation, and HITL.
pub const DEFAULT_LLM_TIMEOUT_SECS: u64 = 300;

/// Default max-tokens for LLM responses when none is specified by the caller.
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Classified LLM errors for retry / circuit-breaker logic.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("context overflow: {0}")]
    ContextOverflow(String),

    /// HTTP 429 / pool exhaustion. `retry_after` carries either a server
    /// `Retry-After` header (for direct 429s) or the soonest cooldown
    /// expiry across the credential pool.
    #[error("rate limited: {message}")]
    RateLimited {
        message: String,
        retry_after: Option<Duration>,
    },

    #[error("provider unavailable: {0}")]
    ProviderUnavailable(String),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("request error: {0}")]
    RequestError(String),
}

impl LlmError {
    /// Convenience constructor for a 429 with no `Retry-After`.
    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::RateLimited {
            message: message.into(),
            retry_after: None,
        }
    }
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
// Fields are populated by serde deserialization and read selectively.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NonStreamingResponse {
    choices: Vec<NonStreamingChoice>,
    #[serde(default)]
    usage: Option<SseUsage>,
}

#[derive(Debug, Deserialize)]
struct NonStreamingChoice {
    message: NonStreamingMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NonStreamingMessage {
    content: Option<String>,
    tool_calls: Option<Vec<NonStreamingToolCall>>,
}

#[derive(Debug, Deserialize)]
struct NonStreamingToolCall {
    id: String,
    function: NonStreamingFunction,
}

#[derive(Debug, Deserialize)]
struct NonStreamingFunction {
    name: String,
    arguments: String,
}

// ---------------------------------------------------------------------------
// ThinkingLevel → ThinkingConfig bridge (sera-1rv8)
// ---------------------------------------------------------------------------

/// Convert a [`ThinkingLevel`] from `sera-types` into the wire-layer
/// [`ThinkingConfig`] used by [`LlmClient`].
///
/// `ThinkingLevel::XHigh` maps to `ReasoningLevel::High` (the highest level
/// `ReasoningLevel` supports) while keeping the Anthropic/Gemini budget at
/// the XHigh token ceiling via `budget_tokens`.
pub fn thinking_config_from_level(level: Option<ThinkingLevel>) -> ThinkingConfig {
    match level {
        None | Some(ThinkingLevel::None) => ThinkingConfig::OFF,
        Some(ThinkingLevel::Low) => ThinkingConfig::new(ReasoningLevel::Low),
        Some(ThinkingLevel::Medium) => ThinkingConfig::new(ReasoningLevel::Medium),
        Some(ThinkingLevel::High) => ThinkingConfig::new(ReasoningLevel::High),
        // XHigh: use High effort string but extend the token budget to 32 768.
        Some(ThinkingLevel::XHigh) => {
            ThinkingConfig::new(ReasoningLevel::High).with_budget(32_768)
        }
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// HTTP client for the LLM proxy endpoint.
///
/// Supports three LLM-auth modes:
/// 1. **Single-account (default)** — uses `base_url` + `api_key` directly.
/// 2. **Account pool (sera-jvi)** — when `account_pool` is set, every request
///    acquires an account from the pool.  Rate-limit / unavailability errors
///    flip the account into cooldown; exhaustion returns
///    [`LlmError::ProviderUnavailable`].
///
/// Optional provider-agnostic reasoning config (sera-48v) is threaded through
/// `thinking` + `provider_kind` so each request emits the correct native
/// parameter (`reasoning.effort` / `enable_thinking` / `thinking` block).
pub struct LlmClient {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
    max_tokens: u32,
    timeout: Duration,
    /// When `Some`, each request acquires an account from the pool and
    /// overrides `base_url` + `api_key` for that request only.  When `None`,
    /// falls back to the single-account env-driven auth.
    account_pool: Option<Arc<AccountPool>>,
    /// Provider-agnostic reasoning config applied to every outgoing request.
    thinking: ThinkingConfig,
    /// Provider kind used to map `thinking` to a native request field.
    provider_kind: ProviderKind,
}

impl LlmClient {
    pub fn new(config: &RuntimeConfig) -> Self {
        let timeout = Duration::from_secs(DEFAULT_LLM_TIMEOUT_SECS);
        // sera-1rv8: translate RuntimeConfig.thinking_level (ThinkingLevel from
        // sera-types) into a ThinkingConfig + ProviderKind for the wire layer.
        let thinking = thinking_config_from_level(config.thinking_level);
        let provider_kind = ProviderKind::infer(&config.llm_model);
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
            account_pool: None,
            thinking,
            provider_kind,
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
            max_tokens: DEFAULT_MAX_TOKENS,
            timeout,
            account_pool: None,
            thinking: ThinkingConfig::default(),
            provider_kind: ProviderKind::Generic,
        }
    }

    /// Attach an [`AccountPool`] — every subsequent request will acquire an
    /// account from the pool, falling back to cooldown-aware failover.
    #[must_use]
    pub fn with_account_pool(mut self, pool: Arc<AccountPool>) -> Self {
        self.account_pool = Some(pool);
        self
    }

    /// Apply a unified [`ThinkingConfig`].  Pair with `with_provider_kind` so
    /// the client knows which native parameter to emit.
    #[must_use]
    pub fn with_thinking(mut self, thinking: ThinkingConfig) -> Self {
        self.thinking = thinking;
        self
    }

    /// Set the [`ProviderKind`] that maps [`ThinkingConfig`] onto native
    /// request fields.
    #[must_use]
    pub fn with_provider_kind(mut self, provider_kind: ProviderKind) -> Self {
        self.provider_kind = provider_kind;
        self
    }

    /// Access the active thinking config (mostly for diagnostics / tests).
    pub fn thinking(&self) -> &ThinkingConfig {
        &self.thinking
    }

    /// Access the configured provider kind.
    pub fn provider_kind(&self) -> ProviderKind {
        self.provider_kind
    }

    /// True when an account pool is attached.
    pub fn has_account_pool(&self) -> bool {
        self.account_pool.is_some()
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
        self.chat_typed_with_behavior(messages, tools, &ToolUseBehavior::Auto).await
    }

    /// Send a chat completion request with streaming SSE and an explicit tool-use policy.
    ///
    /// The `tool_use_behavior` is translated to an OpenAI-compatible `tool_choice`
    /// field and included in the request body when tools are present.
    pub async fn chat_typed_with_behavior(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        tool_use_behavior: &ToolUseBehavior,
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
            // Only include tool_choice when the behavior is non-default, so that
            // providers that don't support the field are not broken by unnecessary noise.
            if !matches!(tool_use_behavior, ToolUseBehavior::Auto) {
                body["tool_choice"] = tool_use_behavior.to_openai_tool_choice();
            }
        } else if tool_use_behavior.forbids_tools() {
            // Explicitly tell the model not to call tools even when none are listed.
            body["tool_choice"] = tool_use_behavior.to_openai_tool_choice();
        }

        // sera-48v: thread the unified reasoning config into the native
        // provider parameter (no-op for Off + Generic providers).
        self.thinking.apply_to_body(&mut body, self.provider_kind);

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
        self.chat_non_streaming_with_behavior(messages, tools, &ToolUseBehavior::Auto).await
    }

    /// Send a non-streaming chat completion request with an explicit tool-use policy.
    #[allow(dead_code)]
    pub async fn chat_non_streaming_with_behavior(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        tool_use_behavior: &ToolUseBehavior,
    ) -> Result<LlmChatResult, LlmError> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": self.max_tokens,
        });

        if !tools.is_empty() {
            body["tools"] =
                serde_json::to_value(tools).map_err(|e| LlmError::RequestError(e.to_string()))?;
            if !matches!(tool_use_behavior, ToolUseBehavior::Auto) {
                body["tool_choice"] = tool_use_behavior.to_openai_tool_choice();
            }
        } else if tool_use_behavior.forbids_tools() {
            body["tool_choice"] = tool_use_behavior.to_openai_tool_choice();
        }

        // sera-48v: mirror streaming path — apply reasoning config.
        self.thinking.apply_to_body(&mut body, self.provider_kind);

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

        // Guard: reject assistant messages that carry neither content nor tool
        // calls — they would otherwise produce silent empty StreamingDelta /
        // TurnCompleted events and a gateway 502 (sera-8h23).
        let content_empty = choice
            .message
            .content
            .as_deref()
            .is_none_or(|s| s.is_empty());
        let tool_calls_empty = tool_calls
            .as_deref()
            .is_none_or(|tc| tc.is_empty());
        if content_empty && tool_calls_empty {
            return Err(LlmError::RequestError(
                "provider returned assistant message with neither content nor tool_calls"
                    .to_string(),
            ));
        }

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
    ///
    /// When an [`AccountPool`] is attached (sera-jvi), this acquires an
    /// account for the request, substitutes its `api_key` (and per-account
    /// `base_url` override, if any), and reports success / rate-limit /
    /// unavailability back to the pool so cooldown state evolves.
    async fn send_request(
        &self,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response, LlmError> {
        // Acquire an account up-front if a pool is attached.  Otherwise fall
        // back to the single-account env path.
        let guard = match &self.account_pool {
            Some(pool) => {
                match pool.acquire() {
                    Ok(g) => Some(g),
                    Err(sera_models::PoolError::NoAccountsAvailable {
                        provider_id,
                        total,
                        min_cooldown_remaining,
                    }) => {
                        // Sera-hjem: surface as RateLimited with the soonest
                        // cooldown expiry so callers can sleep instead of
                        // burning CPU on retries.
                        return Err(LlmError::RateLimited {
                            message: format!(
                                "all {total} account(s) for provider '{provider_id}' are rate-limited or unavailable"
                            ),
                            retry_after: min_cooldown_remaining,
                        });
                    }
                    Err(sera_models::PoolError::EmptyPool(provider_id)) => {
                        return Err(LlmError::RequestError(format!(
                            "account pool for provider '{provider_id}' is empty"
                        )));
                    }
                }
            }
            None => None,
        };

        let (url, api_key) = match &guard {
            Some(g) => {
                let base = g
                    .effective_base_url()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| self.base_url.clone());
                let key = g.account().api_key.clone();
                (format!("{base}/chat/completions"), key)
            }
            None => (
                format!("{}/chat/completions", self.base_url),
                self.api_key.clone(),
            ),
        };

        let result = tokio::time::timeout(self.timeout, async {
            self.client
                .post(&url)
                .header("Authorization", format!("Bearer {api_key}"))
                .header("Content-Type", "application/json")
                .json(body)
                .send()
                .await
        })
        .await;

        let response = match result {
            Err(_) => {
                // Treat outer timeout as transient unavailability of the
                // currently-selected account.
                if let Some(g) = guard {
                    g.mark_unavailable();
                }
                return Err(LlmError::Timeout(format!(
                    "request timed out after {:?}",
                    self.timeout
                )));
            }
            Ok(Err(e)) => {
                let is_timeout = e.is_timeout();
                if let Some(g) = guard {
                    // Network-level failure → treat as provider unavailable so
                    // the pool tries the next account next time.
                    g.mark_unavailable();
                }
                if is_timeout {
                    return Err(LlmError::Timeout(e.to_string()));
                }
                return Err(LlmError::RequestError(e.to_string()));
            }
            Ok(Ok(resp)) => resp,
        };

        let status = response.status();

        if status.is_success() {
            if let Some(g) = guard {
                let provider = self.account_pool_provider_label();
                let credential_id = g.credential_id().to_string();
                record_credential_outcome(&provider, &credential_id, CredentialOutcome::Success);
                g.mark_success();
            }
            return Ok(response);
        }

        // Non-success → grab Retry-After before consuming the body, then
        // classify and inform the pool about the failure kind.
        let retry_after = parse_retry_after(response.headers());
        let error_body = response.text().await.unwrap_or_default();
        let classified = classify_http_error(status.as_u16(), &error_body, retry_after);
        if let Some(g) = guard {
            let provider = self.account_pool_provider_label();
            let credential_id = g.credential_id().to_string();
            match &classified {
                Err(LlmError::RateLimited { retry_after, .. }) => {
                    record_credential_outcome(
                        &provider,
                        &credential_id,
                        CredentialOutcome::RateLimited,
                    );
                    g.record_429(*retry_after);
                }
                Err(LlmError::ProviderUnavailable(_)) => {
                    record_credential_outcome(
                        &provider,
                        &credential_id,
                        CredentialOutcome::Error5xx,
                    );
                    g.mark_unavailable();
                }
                Err(LlmError::RequestError(_)) if is_non_retryable_status(status.as_u16()) => {
                    // Sera-hjem: 4xx other than 429 is the credential's fault
                    // — disable it so round-robin skips it permanently.
                    record_credential_outcome(
                        &provider,
                        &credential_id,
                        CredentialOutcome::Error4xx,
                    );
                    g.record_non_retryable_error();
                }
                _ if (500..=599).contains(&status.as_u16()) => {
                    record_credential_outcome(
                        &provider,
                        &credential_id,
                        CredentialOutcome::Error5xx,
                    );
                    g.mark_success();
                }
                // Context overflow / unclassified — not the credential's
                // fault, leave state untouched and don't taint counters.
                _ => {
                    g.mark_success();
                }
            }
        }
        classified
    }

    /// Provider label for telemetry counters.  Falls back to the configured
    /// model name when no pool is attached.
    fn account_pool_provider_label(&self) -> String {
        self.account_pool
            .as_ref()
            .map(|p| p.provider_id().to_string())
            .unwrap_or_else(|| self.model.clone())
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

                    // Guard: reject assistant messages that carry neither content
                    // nor tool calls — they would produce silent empty
                    // StreamingDelta / TurnCompleted events (sera-8h23).
                    if content.is_empty() && tool_calls.is_none() {
                        return Err(LlmError::RequestError(
                            "provider returned assistant message with neither content nor tool_calls"
                                .to_string(),
                        ));
                    }

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

        // Guard: reject assistant messages that carry neither content nor tool
        // calls — they would produce silent empty StreamingDelta /
        // TurnCompleted events (sera-8h23).
        if content.is_empty() && tool_calls.is_none() {
            return Err(LlmError::RequestError(
                "provider returned assistant message with neither content nor tool_calls"
                    .to_string(),
            ));
        }

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
        <Self as LlmProvider>::chat_with_behavior(self, messages, tools, &ToolUseBehavior::Auto).await
    }

    async fn chat_with_behavior(
        &self,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        tool_use_behavior: &ToolUseBehavior,
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

        // Call the LLM with the tool-use policy applied.
        let result = self
            .chat_typed_with_behavior(&chat_messages, &tool_defs, tool_use_behavior)
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
            plan: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Error classification
// ---------------------------------------------------------------------------

/// Classify an HTTP error into the appropriate `LlmError` variant.
fn classify_http_error(
    status: u16,
    body: &str,
    retry_after: Option<Duration>,
) -> Result<reqwest::Response, LlmError> {
    let lower_body = body.to_lowercase();

    match status {
        429 => Err(LlmError::RateLimited {
            message: truncate_error(body),
            retry_after,
        }),
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

/// True when the HTTP status indicates a credential-level fault that should
/// disable the credential (4xx except 429).  5xx is treated as transient via
/// the existing unavailable path.
fn is_non_retryable_status(status: u16) -> bool {
    matches!(status, 400..=499) && status != 429
}

/// Parse `Retry-After` from response headers.
///
/// Honours both numeric-seconds form (`Retry-After: 30`) and HTTP-date form
/// (`Retry-After: Wed, 21 Oct 2026 07:28:00 GMT`).  Returns `None` when the
/// header is absent or unparseable.
fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let raw = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())?
        .trim();
    if let Ok(secs) = raw.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    // HTTP-date form: convert to seconds from now via httpdate when available.
    // We avoid pulling in an extra dep — a misformatted header just falls
    // through to the default backoff.
    None
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

// ---------------------------------------------------------------------------
// Test helpers — request body building (mirrors chat_typed_with_behavior logic)
// ---------------------------------------------------------------------------

/// Build the streaming request body exactly as `chat_typed_with_behavior` would,
/// without making any network call.  Exposed only under `#[cfg(test)]`.
#[cfg(test)]
fn build_streaming_body(
    model: &str,
    max_tokens: u32,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
    tool_use_behavior: &ToolUseBehavior,
) -> Result<serde_json::Value, LlmError> {
    build_streaming_body_with_thinking(
        model,
        max_tokens,
        messages,
        tools,
        tool_use_behavior,
        &ThinkingConfig::default(),
        ProviderKind::Generic,
    )
}

#[cfg(test)]
fn build_streaming_body_with_thinking(
    model: &str,
    max_tokens: u32,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
    tool_use_behavior: &ToolUseBehavior,
    thinking: &ThinkingConfig,
    provider_kind: ProviderKind,
) -> Result<serde_json::Value, LlmError> {
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "max_tokens": max_tokens,
        "stream": true,
    });

    if !tools.is_empty() {
        body["tools"] =
            serde_json::to_value(tools).map_err(|e| LlmError::RequestError(e.to_string()))?;
        if !matches!(tool_use_behavior, ToolUseBehavior::Auto) {
            body["tool_choice"] = tool_use_behavior.to_openai_tool_choice();
        }
    } else if tool_use_behavior.forbids_tools() {
        body["tool_choice"] = tool_use_behavior.to_openai_tool_choice();
    }

    thinking.apply_to_body(&mut body, provider_kind);

    Ok(body)
}

/// Build the non-streaming request body exactly as `chat_non_streaming_with_behavior` would.
#[cfg(test)]
fn build_non_streaming_body(
    model: &str,
    max_tokens: u32,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
    tool_use_behavior: &ToolUseBehavior,
) -> Result<serde_json::Value, LlmError> {
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "max_tokens": max_tokens,
    });

    if !tools.is_empty() {
        body["tools"] =
            serde_json::to_value(tools).map_err(|e| LlmError::RequestError(e.to_string()))?;
        if !matches!(tool_use_behavior, ToolUseBehavior::Auto) {
            body["tool_choice"] = tool_use_behavior.to_openai_tool_choice();
        }
    } else if tool_use_behavior.forbids_tools() {
        body["tool_choice"] = tool_use_behavior.to_openai_tool_choice();
    }

    Ok(body)
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
        let err = classify_http_error(429, "rate limit exceeded", None).unwrap_err();
        assert!(matches!(err, LlmError::RateLimited { .. }));
        assert!(err.to_string().contains("rate limit exceeded"));
    }

    #[test]
    fn test_error_provider_unavailable() {
        let err = classify_http_error(503, "service unavailable", None).unwrap_err();
        assert!(matches!(err, LlmError::ProviderUnavailable(_)));
    }

    #[test]
    fn test_error_context_overflow_keyword1() {
        let err = classify_http_error(
            400,
            r#"{"error":{"message":"This model's maximum context length is 8192 tokens"}}"#,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, LlmError::ContextOverflow(_)));
    }

    #[test]
    fn test_error_context_overflow_keyword2() {
        let err = classify_http_error(
            400,
            r#"{"error":{"code":"context_length_exceeded","message":"too long"}}"#,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, LlmError::ContextOverflow(_)));
    }

    #[test]
    fn test_error_context_overflow_keyword3() {
        let err = classify_http_error(400, "too many tokens in the request", None).unwrap_err();
        assert!(matches!(err, LlmError::ContextOverflow(_)));
    }

    #[test]
    fn test_error_generic_400() {
        let err = classify_http_error(400, "bad request: missing field", None).unwrap_err();
        assert!(matches!(err, LlmError::RequestError(_)));
    }

    #[test]
    fn test_error_generic_500() {
        let err = classify_http_error(500, "internal server error", None).unwrap_err();
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

        let err = LlmError::rate_limited("slow down");
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

    // =========================================================================
    // Request body shaping tests
    // =========================================================================

    fn make_tool(name: &str) -> ToolDefinition {
        use crate::types::{FunctionDefinition, ToolDefinition};
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: name.to_string(),
                description: format!("Tool {name}"),
                parameters: serde_json::json!({"type":"object","properties":{}}),
            },
        }
    }

    fn make_user_msg(content: &str) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    // --- streaming body ---

    #[test]
    fn request_body_streaming_flag_is_true() {
        let body = build_streaming_body(
            "gpt-4",
            512,
            &[make_user_msg("hi")],
            &[],
            &ToolUseBehavior::Auto,
        )
        .unwrap();
        assert_eq!(body["stream"], serde_json::json!(true));
    }

    #[test]
    fn request_body_model_and_max_tokens_forwarded() {
        let body = build_streaming_body(
            "mistral-7b",
            2048,
            &[make_user_msg("ping")],
            &[],
            &ToolUseBehavior::Auto,
        )
        .unwrap();
        assert_eq!(body["model"], serde_json::json!("mistral-7b"));
        assert_eq!(body["max_tokens"], serde_json::json!(2048));
    }

    #[test]
    fn request_body_no_tool_choice_for_auto_with_tools() {
        // Auto + tools present → tools array included, tool_choice omitted
        let body = build_streaming_body(
            "gpt-4",
            512,
            &[make_user_msg("hi")],
            &[make_tool("read_file")],
            &ToolUseBehavior::Auto,
        )
        .unwrap();
        assert!(body["tools"].is_array());
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn request_body_tool_choice_required_with_tools() {
        let body = build_streaming_body(
            "gpt-4",
            512,
            &[make_user_msg("hi")],
            &[make_tool("read_file")],
            &ToolUseBehavior::Required,
        )
        .unwrap();
        assert_eq!(body["tool_choice"], serde_json::json!("required"));
    }

    #[test]
    fn request_body_tool_choice_none_with_tools() {
        let body = build_streaming_body(
            "gpt-4",
            512,
            &[make_user_msg("hi")],
            &[make_tool("read_file")],
            &ToolUseBehavior::None,
        )
        .unwrap();
        assert_eq!(body["tool_choice"], serde_json::json!("none"));
    }

    #[test]
    fn request_body_tool_choice_specific_with_tools() {
        let body = build_streaming_body(
            "gpt-4",
            512,
            &[make_user_msg("hi")],
            &[make_tool("read_file")],
            &ToolUseBehavior::Specific { name: "read_file".to_string() },
        )
        .unwrap();
        assert_eq!(body["tool_choice"]["type"], "function");
        assert_eq!(body["tool_choice"]["function"]["name"], "read_file");
    }

    #[test]
    fn request_body_no_tools_array_when_empty() {
        // No tools → tools key must not be present
        let body = build_streaming_body(
            "gpt-4",
            512,
            &[make_user_msg("hi")],
            &[],
            &ToolUseBehavior::Auto,
        )
        .unwrap();
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn request_body_tool_choice_none_no_tools_forces_field() {
        // None behavior + no tools → tool_choice still set to "none"
        let body = build_streaming_body(
            "gpt-4",
            512,
            &[make_user_msg("hi")],
            &[],
            &ToolUseBehavior::None,
        )
        .unwrap();
        assert_eq!(body["tool_choice"], serde_json::json!("none"));
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn request_body_required_no_tools_no_spurious_tool_choice() {
        // Required + no tools → Required is not None so forbids_tools()=false
        // → tool_choice must NOT be injected (the LLM call itself would fail
        //   upstream via validate(), but the body builder must not panic)
        let body = build_streaming_body(
            "gpt-4",
            512,
            &[make_user_msg("hi")],
            &[],
            &ToolUseBehavior::Required,
        )
        .unwrap();
        assert!(body.get("tool_choice").is_none());
    }

    // --- non-streaming body mirrors same rules ---

    #[test]
    fn non_streaming_body_has_no_stream_field() {
        let body = build_non_streaming_body(
            "gpt-4",
            512,
            &[make_user_msg("hi")],
            &[],
            &ToolUseBehavior::Auto,
        )
        .unwrap();
        assert!(body.get("stream").is_none());
    }

    #[test]
    fn non_streaming_body_tool_choice_required() {
        let body = build_non_streaming_body(
            "gpt-4",
            512,
            &[make_user_msg("hi")],
            &[make_tool("write_file")],
            &ToolUseBehavior::Required,
        )
        .unwrap();
        assert_eq!(body["tool_choice"], serde_json::json!("required"));
    }

    #[test]
    fn non_streaming_body_no_tool_choice_for_auto() {
        let body = build_non_streaming_body(
            "gpt-4",
            512,
            &[make_user_msg("hi")],
            &[make_tool("write_file")],
            &ToolUseBehavior::Auto,
        )
        .unwrap();
        assert!(body.get("tool_choice").is_none());
    }

    // =========================================================================
    // Response parsing tests (NonStreamingResponse)
    // =========================================================================

    #[test]
    fn non_streaming_missing_usage_defaults_to_zero() {
        let json = r#"{
            "choices": [{
                "message": { "content": "ok", "tool_calls": null },
                "finish_reason": "stop"
            }]
        }"#;
        let parsed: NonStreamingResponse = serde_json::from_str(json).unwrap();
        let usage = parsed.usage.unwrap_or_default();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
    }

    // sera-8h23: both content=null and tool_calls absent → LlmError::RequestError
    #[tokio::test]
    async fn non_streaming_null_content_and_tool_calls_both_absent_is_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": null}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 5, "completion_tokens": 0}
            })))
            .mount(&server)
            .await;

        let client = LlmClient::with_params(&server.uri(), "test-model", None, 512);
        let result = client.chat(&[], &[]).await;
        assert!(
            matches!(result, Err(LlmError::RequestError(ref msg)) if msg.contains("neither content nor tool_calls")),
            "expected RequestError for null content + no tool_calls"
        );
    }

    // sera-8h23: content="" (empty string) with no tool_calls → also rejected
    #[tokio::test]
    async fn non_streaming_empty_string_content_and_no_tool_calls_is_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": ""}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 5, "completion_tokens": 0}
            })))
            .mount(&server)
            .await;

        let client = LlmClient::with_params(&server.uri(), "test-model", None, 512);
        let result = client.chat(&[], &[]).await;
        assert!(
            matches!(result, Err(LlmError::RequestError(ref msg)) if msg.contains("neither content nor tool_calls")),
            "expected RequestError for empty string content + no tool_calls"
        );
    }

    #[test]
    fn non_streaming_finish_reason_tool_calls() {
        let json = r#"{
            "choices": [{
                "message": { "content": null, "tool_calls": [
                    {"id":"c1","type":"function","function":{"name":"f","arguments":"{}"}}
                ]},
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 3 }
        }"#;
        let parsed: NonStreamingResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(parsed.choices[0].message.tool_calls.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn non_streaming_only_first_choice_consumed() {
        // OpenAI may return n>1 choices; wire format accepts multiple but we only
        // use index 0.  Verify the struct at least deserializes all of them.
        let json = r#"{
            "choices": [
                {"message":{"content":"first"},"finish_reason":"stop"},
                {"message":{"content":"second"},"finish_reason":"stop"}
            ],
            "usage": {"prompt_tokens":1,"completion_tokens":1}
        }"#;
        let parsed: NonStreamingResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.choices.len(), 2);
        assert_eq!(parsed.choices[0].message.content.as_deref(), Some("first"));
    }

    // =========================================================================
    // Error classification edge cases
    // =========================================================================

    #[test]
    fn error_401_maps_to_request_error() {
        // 401 is not specially handled — falls through to generic RequestError
        let err = classify_http_error(401, "unauthorized", None).unwrap_err();
        assert!(matches!(err, LlmError::RequestError(_)));
        assert!(err.to_string().contains("HTTP 401"));
    }

    #[test]
    fn error_404_maps_to_request_error() {
        let err = classify_http_error(404, "not found", None).unwrap_err();
        assert!(matches!(err, LlmError::RequestError(_)));
        assert!(err.to_string().contains("HTTP 404"));
    }

    #[test]
    fn error_502_maps_to_request_error() {
        let err = classify_http_error(502, "bad gateway", None).unwrap_err();
        assert!(matches!(err, LlmError::RequestError(_)));
        assert!(err.to_string().contains("HTTP 502"));
    }

    #[test]
    fn error_context_overflow_context_window_keyword() {
        // "context window" keyword (distinct from "context_length_exceeded")
        let err = classify_http_error(400, "exceeded the context window limit", None).unwrap_err();
        assert!(matches!(err, LlmError::ContextOverflow(_)));
    }

    #[test]
    fn error_400_non_overflow_body_is_request_error() {
        // 400 with body that doesn't match any overflow keyword
        let err = classify_http_error(400, "invalid model specified", None).unwrap_err();
        assert!(matches!(err, LlmError::RequestError(_)));
        assert!(err.to_string().contains("HTTP 400"));
    }

    #[test]
    fn error_body_exactly_500_chars_not_truncated() {
        let body = "x".repeat(500);
        let result = truncate_error(&body);
        // Exactly 500 chars → not truncated (condition is > 500)
        assert_eq!(result.len(), 500);
        assert!(!result.ends_with("..."));
    }

    #[test]
    fn error_body_501_chars_truncated() {
        let body = "x".repeat(501);
        let result = truncate_error(&body);
        assert!(result.ends_with("..."));
        assert!(result.len() < 510); // 500 chars + "..."
    }

    #[test]
    fn error_empty_body_classifies_cleanly() {
        let err = classify_http_error(429, "", None).unwrap_err();
        assert!(matches!(err, LlmError::RateLimited { .. }));
    }

    // =========================================================================
    // SSE parsing edge cases
    // =========================================================================

    #[test]
    fn sse_data_prefix_no_space_is_parsed() {
        // "data:{...}" (no space after colon) must also be recognised
        let line = r#"data:{"choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":null}]}"#;
        let chunk = parse_sse_data_line(line).expect("should parse data: without space");
        let choices = chunk.choices.unwrap();
        let delta = choices[0].delta.as_ref().unwrap();
        assert_eq!(delta.content.as_deref(), Some("hi"));
    }

    #[test]
    fn sse_comment_line_returns_none() {
        // SSE comment (starts with ':') — not a data line, parse_sse_data_line returns None
        let result = parse_sse_data_line(": keep-alive");
        assert!(result.is_none());
    }

    #[test]
    fn sse_usage_chunk_with_empty_choices() {
        let line = r#"data: {"choices":[],"usage":{"prompt_tokens":42,"completion_tokens":17}}"#;
        let chunk = parse_sse_data_line(line).expect("should parse");
        assert!(chunk.choices.as_ref().unwrap().is_empty());
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 42);
        assert_eq!(usage.completion_tokens, 17);
    }

    #[test]
    fn sse_content_and_tool_calls_in_same_delta() {
        // Some models send content + tool_calls in a single delta
        let line = r#"data: {"choices":[{"index":0,"delta":{"content":"thinking...","tool_calls":[{"index":0,"id":"c1","function":{"name":"fn","arguments":""}}]},"finish_reason":null}]}"#;
        let chunk = parse_sse_data_line(line).expect("should parse");
        let choices = chunk.choices.unwrap();
        let delta = choices[0].delta.as_ref().unwrap();
        assert_eq!(delta.content.as_deref(), Some("thinking..."));
        assert!(delta.tool_calls.is_some());
    }

    #[test]
    fn sse_tool_call_chunk_without_id_on_continuation() {
        // Subsequent argument chunks omit the id field — accumulator must not overwrite
        let mut map: HashMap<usize, ToolCallAccumulator> = HashMap::new();

        // First chunk sets id + name
        let c1 = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_x","function":{"name":"do_thing","arguments":""}}]},"finish_reason":null}]}"#;
        let chunk1: SseChunk = serde_json::from_str(c1).unwrap();
        apply_chunk_tool_calls(&chunk1, &mut map);

        // Continuation chunk has no id
        let c2 = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"x\":1}"}}]},"finish_reason":null}]}"#;
        let chunk2: SseChunk = serde_json::from_str(c2).unwrap();
        apply_chunk_tool_calls(&chunk2, &mut map);

        let acc = map.get(&0).unwrap();
        assert_eq!(acc.id, "call_x"); // not overwritten
        assert_eq!(acc.function_name, "do_thing");
        assert_eq!(acc.arguments, r#"{"x":1}"#);
    }

    #[test]
    fn sse_finish_reason_length_captured() {
        let line = r#"data: {"choices":[{"index":0,"delta":{},"finish_reason":"length"}]}"#;
        let chunk = parse_sse_data_line(line).expect("should parse");
        assert_eq!(
            chunk.choices.unwrap()[0].finish_reason.as_deref(),
            Some("length")
        );
    }

    #[test]
    fn sse_null_choices_field_handled() {
        // Some providers send {"choices":null} on the usage-only final chunk
        let line = r#"data: {"usage":{"prompt_tokens":5,"completion_tokens":2}}"#;
        let chunk = parse_sse_data_line(line).expect("should parse");
        assert!(chunk.choices.is_none());
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 5);
    }

    // =========================================================================
    // sera-48v — ThinkingConfig wired through body builder
    // =========================================================================

    #[test]
    fn streaming_body_applies_openai_reasoning_effort() {
        let cfg = ThinkingConfig::new(sera_models::ReasoningLevel::Medium);
        let body = build_streaming_body_with_thinking(
            "o1",
            512,
            &[make_user_msg("hi")],
            &[],
            &ToolUseBehavior::Auto,
            &cfg,
            ProviderKind::OpenAi,
        )
        .unwrap();
        assert_eq!(body["reasoning"]["effort"], "medium");
    }

    #[test]
    fn streaming_body_applies_qwen_enable_thinking() {
        let cfg = ThinkingConfig::new(sera_models::ReasoningLevel::High);
        let body = build_streaming_body_with_thinking(
            "qwen-max",
            512,
            &[make_user_msg("hi")],
            &[],
            &ToolUseBehavior::Auto,
            &cfg,
            ProviderKind::Qwen,
        )
        .unwrap();
        assert_eq!(body["enable_thinking"], true);
    }

    #[test]
    fn streaming_body_off_omits_reasoning_for_openai() {
        let body = build_streaming_body_with_thinking(
            "o1",
            512,
            &[make_user_msg("hi")],
            &[],
            &ToolUseBehavior::Auto,
            &ThinkingConfig::default(),
            ProviderKind::OpenAi,
        )
        .unwrap();
        assert!(body.get("reasoning").is_none());
    }

    #[test]
    fn streaming_body_off_still_sets_qwen_false() {
        let body = build_streaming_body_with_thinking(
            "qwen",
            512,
            &[make_user_msg("hi")],
            &[],
            &ToolUseBehavior::Auto,
            &ThinkingConfig::default(),
            ProviderKind::Qwen,
        )
        .unwrap();
        assert_eq!(body["enable_thinking"], false);
    }

    // =========================================================================
    // sera-jvi — LlmClient builder / pool attachment
    // =========================================================================

    #[test]
    fn new_client_has_no_account_pool_by_default() {
        let c = LlmClient::with_params("http://x", "m", None, 1000);
        assert!(!c.has_account_pool());
        assert_eq!(c.provider_kind(), ProviderKind::Generic);
        assert!(c.thinking().is_off());
    }

    #[test]
    fn client_with_account_pool_reports_true() {
        use sera_models::{AccountPool, CooldownConfig, ProviderAccount};
        let pool = Arc::new(AccountPool::new(
            "openai",
            vec![ProviderAccount::new("k0", "sk-0", None)],
            CooldownConfig::default(),
        ));
        let c = LlmClient::with_params("http://x", "m", None, 1000)
            .with_account_pool(pool);
        assert!(c.has_account_pool());
    }

    #[test]
    fn client_with_thinking_stores_config() {
        let c = LlmClient::with_params("http://x", "m", None, 1000)
            .with_thinking(ThinkingConfig::new(sera_models::ReasoningLevel::High))
            .with_provider_kind(ProviderKind::Anthropic);
        assert_eq!(c.thinking().level, sera_models::ReasoningLevel::High);
        assert_eq!(c.provider_kind(), ProviderKind::Anthropic);
    }

    // =========================================================================
    // sera-1rv8 — thinking_config_from_level conversion
    // =========================================================================

    #[test]
    fn thinking_config_from_none_is_off() {
        let cfg = thinking_config_from_level(None);
        assert!(cfg.is_off());
        assert!(cfg.budget_tokens.is_none());
    }

    #[test]
    fn thinking_config_from_thinking_level_none_is_off() {
        let cfg = thinking_config_from_level(Some(ThinkingLevel::None));
        assert!(cfg.is_off());
    }

    #[test]
    fn thinking_config_from_low_maps_to_reasoning_low() {
        let cfg = thinking_config_from_level(Some(ThinkingLevel::Low));
        assert_eq!(cfg.level, sera_models::ReasoningLevel::Low);
        assert!(cfg.budget_tokens.is_none());
    }

    #[test]
    fn thinking_config_from_medium_maps_to_reasoning_medium() {
        let cfg = thinking_config_from_level(Some(ThinkingLevel::Medium));
        assert_eq!(cfg.level, sera_models::ReasoningLevel::Medium);
        assert!(cfg.budget_tokens.is_none());
    }

    #[test]
    fn thinking_config_from_high_maps_to_reasoning_high() {
        let cfg = thinking_config_from_level(Some(ThinkingLevel::High));
        assert_eq!(cfg.level, sera_models::ReasoningLevel::High);
        assert!(cfg.budget_tokens.is_none());
    }

    #[test]
    fn thinking_config_from_xhigh_maps_to_high_with_32768_budget() {
        let cfg = thinking_config_from_level(Some(ThinkingLevel::XHigh));
        assert_eq!(cfg.level, sera_models::ReasoningLevel::High);
        assert_eq!(cfg.budget_tokens, Some(32_768));
    }

    #[test]
    fn xhigh_anthropic_body_uses_32768_budget() {
        let cfg = thinking_config_from_level(Some(ThinkingLevel::XHigh));
        let mut body = serde_json::json!({});
        cfg.apply_to_body(&mut body, ProviderKind::Anthropic);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], serde_json::json!(32_768u32));
    }

    #[test]
    fn xhigh_openai_maps_to_high_effort() {
        let cfg = thinking_config_from_level(Some(ThinkingLevel::XHigh));
        let mut body = serde_json::json!({});
        cfg.apply_to_body(&mut body, ProviderKind::OpenAi);
        assert_eq!(body["reasoning"]["effort"], "high");
    }

    // =========================================================================
    // sera-1rv8 — RuntimeConfig.thinking_level propagates into LlmClient
    // =========================================================================

    fn make_runtime_config_with_thinking(level: Option<ThinkingLevel>) -> crate::config::RuntimeConfig {
        crate::config::RuntimeConfig {
            llm_base_url: "http://localhost:1234/v1".into(),
            llm_model: "claude-3-5-sonnet".into(),
            llm_api_key: "test-key".into(),
            chat_port: 8080,
            agent_id: "test-agent".into(),
            lifecycle_mode: "task".into(),
            core_url: "http://localhost:3001".into(),
            api_key: "test-api-key".into(),
            context_window: 128_000,
            compaction_strategy: "summarize".into(),
            max_tokens: 4096,
            circle_activity_enabled: false,
            semantic_enrichment_enabled: false,
            semantic_top_k: 3,
            semantic_similarity_threshold: None,
            semantic_enrichment_timeout_ms: 150,
            hierarchical_scopes_enabled: false,
            tool_authz_enabled: false,
            tool_authz_roles: None,
            thinking_level: level,
        }
    }

    #[test]
    fn runtime_config_thinking_none_gives_off_client() {
        let config = make_runtime_config_with_thinking(None);
        let client = LlmClient::new(&config);
        assert!(client.thinking().is_off());
    }

    #[test]
    fn runtime_config_thinking_medium_propagates_to_client() {
        let config = make_runtime_config_with_thinking(Some(ThinkingLevel::Medium));
        let client = LlmClient::new(&config);
        assert_eq!(client.thinking().level, sera_models::ReasoningLevel::Medium);
    }

    #[test]
    fn runtime_config_thinking_xhigh_propagates_budget_to_client() {
        let config = make_runtime_config_with_thinking(Some(ThinkingLevel::XHigh));
        let client = LlmClient::new(&config);
        assert_eq!(client.thinking().level, sera_models::ReasoningLevel::High);
        assert_eq!(client.thinking().budget_tokens, Some(32_768));
    }

    #[test]
    fn runtime_config_model_name_infers_provider_kind() {
        // "claude-3-5-sonnet" → Anthropic
        let config = make_runtime_config_with_thinking(None);
        let client = LlmClient::new(&config);
        assert_eq!(client.provider_kind(), ProviderKind::Anthropic);
    }
}
