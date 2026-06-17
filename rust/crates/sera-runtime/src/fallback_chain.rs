//! Ordered provider chain — try primary first, fall back to next on retryable errors.
//!
//! Used to put MiniMax (or any OpenAI-compatible endpoint) in front of a local
//! LM Studio / Qwen backup so SERA prefers the cheap/cloud endpoint while
//! preserving local continuity if the primary is unavailable. Each entry is a
//! plain [`LlmClient`] so the chain works for any OpenAI-compatible provider
//! without provider-specific adapter code.
//!
//! ## Fallback classification
//!
//! Errors from a [`LlmClient`] call are partitioned into two classes:
//!
//! - **Retryable** — [`LlmError::RateLimited`], [`LlmError::ProviderUnavailable`],
//!   [`LlmError::Timeout`]. The chain advances to the next provider.
//! - **Non-retryable** — [`LlmError::RequestError`] (4xx other than 429,
//!   malformed body, tool-schema, policy, serialization bugs) and
//!   [`LlmError::ContextOverflow`] (genuine model-side limit). These surface
//!   immediately so a real SERA bug is not silently masked by falling back.
//!
//! Transport-level failures (DNS, connection refused, TLS handshake, mid-
//! request body drops, h2 stream resets, etc.) are classified by [`LlmClient`]
//! as [`LlmError::ProviderUnavailable`] — see `classify_send_error` in
//! `llm_client.rs`, which defaults any non-timeout reqwest error from `.send()`
//! to `ProviderUnavailable` and reserves `RequestError` for the
//! `is_request()` (SERA-side request-builder bug) case alone. This is the
//! fix for the PR #1272 P1 review: a primary-down outage now falls through
//! to the next provider rather than masquerading as a SERA-side request bug.
//!
//! When the last provider in the chain fails, its error is returned verbatim
//! (whatever class it was), preserving caller-visible diagnostics.
//!
//! ## Wiring
//!
//! [`crate::llm_client::build_provider_from_config`] reads
//! `SERA_LLM_FALLBACK_BASE_URL`, `SERA_LLM_FALLBACK_MODEL`, and
//! `SERA_LLM_FALLBACK_API_KEY` and, when all three are set, builds a chain of
//! `[primary, fallback]`. Otherwise it returns the primary alone — the
//! single-provider path is byte-identical to the pre-chain behaviour.

use async_trait::async_trait;
use sera_types::runtime::TokenUsage;
use sera_types::tool::ToolUseBehavior;

use crate::llm_client::{LlmChatResult, LlmClient, LlmError};
use crate::turn::{LlmProvider, ThinkError, ThinkResult};
use crate::types::{ChatMessage, ToolDefinition};

/// Ordered chain of [`LlmClient`]s with retryable-only fallback semantics.
pub struct FallbackChain {
    providers: Vec<LlmClient>,
}

impl FallbackChain {
    /// Build a chain from an ordered list of clients. Panics on empty input —
    /// a zero-provider chain is always a configuration bug, not a runtime
    /// condition worth surfacing as an error variant.
    pub fn new(providers: Vec<LlmClient>) -> Self {
        assert!(
            !providers.is_empty(),
            "FallbackChain requires at least one provider"
        );
        Self { providers }
    }

    /// Number of providers in the chain (primary + fallbacks).
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// Always `false` — [`FallbackChain::new`] rejects an empty provider list,
    /// so a constructed chain is never empty. Defined for clippy parity with
    /// [`Self::len`].
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Streaming chat with explicit tool-use policy. Tries each provider in
    /// order, advancing only on [`is_retryable`] errors.
    pub async fn chat_typed_with_behavior(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        tool_use_behavior: &ToolUseBehavior,
    ) -> Result<LlmChatResult, LlmError> {
        let last_index = self.providers.len() - 1;
        let mut last_err: Option<LlmError> = None;
        for (idx, provider) in self.providers.iter().enumerate() {
            match provider
                .chat_typed_with_behavior(messages, tools, tool_use_behavior)
                .await
            {
                Ok(result) => return Ok(result),
                Err(err) => {
                    if !is_retryable(&err) || idx == last_index {
                        return Err(err);
                    }
                    tracing::warn!(
                        provider_index = idx,
                        error = %err,
                        "provider failed with retryable error, advancing chain"
                    );
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            LlmError::RequestError("FallbackChain exhausted with no providers".to_string())
        }))
    }

    /// Non-streaming chat with explicit tool-use policy.
    pub async fn chat_non_streaming_with_behavior(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        tool_use_behavior: &ToolUseBehavior,
    ) -> Result<LlmChatResult, LlmError> {
        let last_index = self.providers.len() - 1;
        let mut last_err: Option<LlmError> = None;
        for (idx, provider) in self.providers.iter().enumerate() {
            match provider
                .chat_non_streaming_with_behavior(messages, tools, tool_use_behavior)
                .await
            {
                Ok(result) => return Ok(result),
                Err(err) => {
                    if !is_retryable(&err) || idx == last_index {
                        return Err(err);
                    }
                    tracing::warn!(
                        provider_index = idx,
                        error = %err,
                        "provider failed with retryable error, advancing chain (non-streaming)"
                    );
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            LlmError::RequestError("FallbackChain exhausted with no providers".to_string())
        }))
    }
}

/// True when an [`LlmError`] is eligible for chain fallback.
///
/// Retryable classes are exactly those that indicate upstream-side trouble
/// rather than a SERA-side request bug. Falling back on
/// [`LlmError::RequestError`] would mask tool-schema / policy / serialization
/// bugs; falling back on [`LlmError::ContextOverflow`] would just produce the
/// same error from the next provider at best.
pub fn is_retryable(err: &LlmError) -> bool {
    matches!(
        err,
        LlmError::RateLimited { .. } | LlmError::ProviderUnavailable(_) | LlmError::Timeout(_)
    )
}

#[async_trait]
impl LlmProvider for FallbackChain {
    async fn chat(
        &self,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
    ) -> Result<ThinkResult, ThinkError> {
        <Self as LlmProvider>::chat_with_behavior(self, messages, tools, &ToolUseBehavior::Auto)
            .await
    }

    async fn chat_with_behavior(
        &self,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        tool_use_behavior: &ToolUseBehavior,
    ) -> Result<ThinkResult, ThinkError> {
        let chat_messages: Vec<ChatMessage> = messages
            .iter()
            .map(|m| serde_json::from_value(m.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ThinkError::Conversion(format!("message deserialization: {e}")))?;

        let tool_defs: Vec<ToolDefinition> = tools
            .iter()
            .map(|t| serde_json::from_value(t.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ThinkError::Conversion(format!("tool deserialization: {e}")))?;

        let result = self
            .chat_typed_with_behavior(&chat_messages, &tool_defs, tool_use_behavior)
            .await
            .map_err(|e| ThinkError::Llm(e.to_string()))?;

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

        // Mirror the direct `LlmClient` adapter (sera-xbmz): embed the assistant
        // `tool_calls` in the transcript message so the next provider request
        // carries a well-formed assistant tool-call row before its tool results.
        let mut response = serde_json::json!({
            "role": "assistant",
            "content": result.message.content,
        });
        if !tool_calls.is_empty() {
            response["tool_calls"] = serde_json::Value::Array(tool_calls.clone());
        }

        Ok(ThinkResult {
            response,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn rate_limited_is_retryable() {
        let err = LlmError::RateLimited {
            message: "slow down".into(),
            retry_after: Some(Duration::from_secs(1)),
        };
        assert!(is_retryable(&err));
    }

    #[test]
    fn provider_unavailable_is_retryable() {
        assert!(is_retryable(&LlmError::ProviderUnavailable("503".into())));
    }

    #[test]
    fn timeout_is_retryable() {
        assert!(is_retryable(&LlmError::Timeout("60s".into())));
    }

    #[test]
    fn request_error_is_not_retryable() {
        // Schema / policy / serialization bugs must NOT trigger fallback —
        // the chain would just hide a real SERA bug.
        assert!(!is_retryable(&LlmError::RequestError(
            "HTTP 400: missing tool argument".into()
        )));
    }

    #[test]
    fn context_overflow_is_not_retryable() {
        // The next provider has the same context limit at best; falling back
        // would just produce the same error after another roundtrip.
        assert!(!is_retryable(&LlmError::ContextOverflow(
            "max context exceeded".into()
        )));
    }

    #[test]
    #[should_panic(expected = "at least one provider")]
    fn empty_chain_panics() {
        let _ = FallbackChain::new(Vec::new());
    }

    #[test]
    fn single_provider_chain_reports_length_one() {
        let client = LlmClient::with_params("http://unused.invalid", "m", None, 1_000);
        let chain = FallbackChain::new(vec![client]);
        assert_eq!(chain.len(), 1);
    }

    #[tokio::test]
    async fn fallback_chain_adapter_embeds_tool_calls_in_assistant_transcript() {
        // sera-xbmz: the FallbackChain LlmProvider adapter must embed assistant
        // tool_calls in the transcript response, same as the direct LlmClient
        // adapter — otherwise the SERA_LLM_FALLBACK_* configuration recreates
        // the orphaned tool-transcript / provider-400 scenario.
        use crate::turn::LlmProvider;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"file-write\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse))
            .mount(&server)
            .await;

        let client = LlmClient::with_params(&server.uri(), "test-model", None, 512);
        let chain = FallbackChain::new(vec![client]);
        let tool = serde_json::json!({
            "type": "function",
            "function": {
                "name": "file-write",
                "description": "Tool file-write",
                "parameters": {"type":"object","properties":{}}
            }
        });
        let result = <FallbackChain as LlmProvider>::chat(
            &chain,
            &[serde_json::json!({"role":"user","content":"create a file"})],
            &[tool],
        )
        .await
        .unwrap();

        assert_eq!(result.tool_calls[0]["id"], "call_1");
        assert_eq!(result.response["role"], "assistant");
        assert_eq!(result.response["tool_calls"][0]["id"], "call_1");
        assert_eq!(result.response["tool_calls"][0]["function"]["name"], "file-write");
    }
}
