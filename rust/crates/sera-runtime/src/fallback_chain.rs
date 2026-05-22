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
}
