//! Model provider trait and configuration.
//!
//! The [`ModelProvider`] trait abstracts over different LLM providers,
//! allowing SERA to use OpenAI, Anthropic, local models, or any
//! provider that implements this interface.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::ModelError;
use crate::response::ModelResponse;
use sera_types::model::ModelRequest;

/// Configuration for a model provider.
///
/// Each variant represents a different provider type with its
/// specific configuration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum ProviderConfig {
    /// OpenAI-compatible API provider.
    OpenAi {
        api_key: String,
        model: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        base_url: Option<String>,
    },
    /// Anthropic API provider.
    Anthropic {
        api_key: String,
        model: String,
    },
    /// Local model via OAI-compatible endpoint.
    Local {
        model: String,
        base_url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
    },
    /// Google AI (Gemini) provider.
    GoogleAi {
        api_key: String,
        model: String,
    },
    /// AWS Bedrock provider.
    AwsBedrock {
        region: String,
        model: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        aws_access_key: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        aws_secret_key: Option<String>,
    },
    /// Generic OAI-compatible provider.
    OaiCompatible {
        model: String,
        base_url: String,
        api_key: Option<String>,
    },
}

impl ProviderConfig {
    /// Get the base URL for this provider.
    pub fn base_url(&self) -> Option<&str> {
        match self {
            ProviderConfig::OpenAi { base_url, .. } => base_url.as_deref(),
            ProviderConfig::Local { base_url, .. } => Some(base_url),
            ProviderConfig::OaiCompatible { base_url, .. } => Some(base_url),
            _ => None,
        }
    }

    /// Get the model name for this provider.
    pub fn model(&self) -> &str {
        match self {
            ProviderConfig::OpenAi { model, .. } => model,
            ProviderConfig::Anthropic { model, .. } => model,
            ProviderConfig::Local { model, .. } => model,
            ProviderConfig::GoogleAi { model, .. } => model,
            ProviderConfig::AwsBedrock { model, .. } => model,
            ProviderConfig::OaiCompatible { model, .. } => model,
        }
    }
}

/// A model provider that can handle LLM requests.
///
/// Implement this trait to add support for new model providers.
/// Each implementation handles the provider-specific details of:
/// - Authentication
/// - Request serialization
/// - Response parsing
/// - Error handling
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Send a chat completion request to the model.
    async fn chat(&self, request: ModelRequest) -> Result<ModelResponse, ModelError>;

    /// Get the provider configuration.
    fn config(&self) -> &ProviderConfig;

    /// Check if the provider is available and healthy.
    async fn health_check(&self) -> Result<(), ModelError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response::{FinishReason, Usage};
    use async_trait::async_trait;
    use serde_json::json;

    // ── ProviderConfig::base_url() ────────────────────────────────────────────

    #[test]
    fn openai_base_url_none_when_not_set() {
        let cfg = ProviderConfig::OpenAi {
            api_key: "sk-test".into(),
            model: "gpt-4o".into(),
            base_url: None,
        };
        assert_eq!(cfg.base_url(), None);
    }

    #[test]
    fn openai_base_url_some_when_set() {
        let cfg = ProviderConfig::OpenAi {
            api_key: "sk-test".into(),
            model: "gpt-4o".into(),
            base_url: Some("https://proxy.example.com/v1".into()),
        };
        assert_eq!(cfg.base_url(), Some("https://proxy.example.com/v1"));
    }

    #[test]
    fn local_base_url_always_present() {
        let cfg = ProviderConfig::Local {
            model: "llama-3".into(),
            base_url: "http://localhost:11434/v1".into(),
            api_key: None,
        };
        assert_eq!(cfg.base_url(), Some("http://localhost:11434/v1"));
    }

    #[test]
    fn oai_compatible_base_url_present() {
        let cfg = ProviderConfig::OaiCompatible {
            model: "mistral".into(),
            base_url: "https://api.together.xyz/v1".into(),
            api_key: Some("tok-abc".into()),
        };
        assert_eq!(cfg.base_url(), Some("https://api.together.xyz/v1"));
    }

    #[test]
    fn anthropic_base_url_is_none() {
        let cfg = ProviderConfig::Anthropic {
            api_key: "anthro-key".into(),
            model: "claude-3-5-sonnet".into(),
        };
        assert_eq!(cfg.base_url(), None);
    }

    #[test]
    fn google_ai_base_url_is_none() {
        let cfg = ProviderConfig::GoogleAi {
            api_key: "gemini-key".into(),
            model: "gemini-2.0-flash".into(),
        };
        assert_eq!(cfg.base_url(), None);
    }

    #[test]
    fn aws_bedrock_base_url_is_none() {
        let cfg = ProviderConfig::AwsBedrock {
            region: "us-east-1".into(),
            model: "anthropic.claude-3-sonnet".into(),
            aws_access_key: None,
            aws_secret_key: None,
        };
        assert_eq!(cfg.base_url(), None);
    }

    // ── ProviderConfig::model() ───────────────────────────────────────────────

    #[test]
    fn model_returns_correct_name_for_each_variant() {
        let cases: Vec<(ProviderConfig, &str)> = vec![
            (
                ProviderConfig::OpenAi {
                    api_key: "k".into(),
                    model: "gpt-4o".into(),
                    base_url: None,
                },
                "gpt-4o",
            ),
            (
                ProviderConfig::Anthropic {
                    api_key: "k".into(),
                    model: "claude-3-5-sonnet".into(),
                },
                "claude-3-5-sonnet",
            ),
            (
                ProviderConfig::Local {
                    model: "llama-3-8b".into(),
                    base_url: "http://localhost:11434/v1".into(),
                    api_key: None,
                },
                "llama-3-8b",
            ),
            (
                ProviderConfig::GoogleAi {
                    api_key: "k".into(),
                    model: "gemini-2.0-flash".into(),
                },
                "gemini-2.0-flash",
            ),
            (
                ProviderConfig::AwsBedrock {
                    region: "us-east-1".into(),
                    model: "anthropic.claude-3-sonnet".into(),
                    aws_access_key: None,
                    aws_secret_key: None,
                },
                "anthropic.claude-3-sonnet",
            ),
            (
                ProviderConfig::OaiCompatible {
                    model: "mistral-7b".into(),
                    base_url: "https://api.together.xyz/v1".into(),
                    api_key: None,
                },
                "mistral-7b",
            ),
        ];

        for (cfg, expected) in cases {
            assert_eq!(
                cfg.model(),
                expected,
                "model() mismatch for variant"
            );
        }
    }

    // ── ProviderConfig serde round-trips ──────────────────────────────────────

    #[test]
    fn openai_config_serde_roundtrip() {
        let cfg = ProviderConfig::OpenAi {
            api_key: "sk-secret".into(),
            model: "gpt-4o-mini".into(),
            base_url: Some("https://custom.openai.example/v1".into()),
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let parsed: ProviderConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.model(), "gpt-4o-mini");
        assert_eq!(parsed.base_url(), Some("https://custom.openai.example/v1"));
    }

    #[test]
    fn openai_config_omits_base_url_when_none() {
        let cfg = ProviderConfig::OpenAi {
            api_key: "sk-secret".into(),
            model: "gpt-4o".into(),
            base_url: None,
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        assert!(
            !json.contains("base_url"),
            "base_url should be omitted when None, got: {json}"
        );
    }

    #[test]
    fn anthropic_config_serde_roundtrip() {
        let cfg = ProviderConfig::Anthropic {
            api_key: "anthro-secret".into(),
            model: "claude-3-opus".into(),
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        assert!(json.contains("\"provider\":\"anthropic\""), "tag missing: {json}");
        let parsed: ProviderConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.model(), "claude-3-opus");
    }

    #[test]
    fn local_config_serde_roundtrip() {
        let cfg = ProviderConfig::Local {
            model: "llama-3-8b".into(),
            base_url: "http://localhost:11434/v1".into(),
            api_key: Some("ollama-key".into()),
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let parsed: ProviderConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.model(), "llama-3-8b");
        assert_eq!(parsed.base_url(), Some("http://localhost:11434/v1"));
    }

    #[test]
    fn local_config_omits_api_key_when_none() {
        let cfg = ProviderConfig::Local {
            model: "llama-3".into(),
            base_url: "http://localhost:11434/v1".into(),
            api_key: None,
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        assert!(
            !json.contains("api_key"),
            "api_key should be omitted when None, got: {json}"
        );
    }

    #[test]
    fn google_ai_config_serde_roundtrip() {
        let cfg = ProviderConfig::GoogleAi {
            api_key: "gem-key".into(),
            model: "gemini-2.0-flash".into(),
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let parsed: ProviderConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.model(), "gemini-2.0-flash");
    }

    #[test]
    fn aws_bedrock_config_serde_roundtrip_with_keys() {
        let cfg = ProviderConfig::AwsBedrock {
            region: "eu-west-1".into(),
            model: "amazon.titan-text".into(),
            aws_access_key: Some("AKIAIOSFODNN7EXAMPLE".into()),
            aws_secret_key: Some("secret".into()),
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let parsed: ProviderConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.model(), "amazon.titan-text");
    }

    #[test]
    fn aws_bedrock_config_omits_keys_when_none() {
        let cfg = ProviderConfig::AwsBedrock {
            region: "us-east-1".into(),
            model: "model".into(),
            aws_access_key: None,
            aws_secret_key: None,
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        assert!(
            !json.contains("aws_access_key"),
            "aws_access_key should be omitted when None, got: {json}"
        );
        assert!(
            !json.contains("aws_secret_key"),
            "aws_secret_key should be omitted when None, got: {json}"
        );
    }

    #[test]
    fn oai_compatible_config_serde_roundtrip() {
        let cfg = ProviderConfig::OaiCompatible {
            model: "qwen-72b".into(),
            base_url: "https://api.qwen.example/v1".into(),
            api_key: None,
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        assert!(
            json.contains("\"provider\":\"oai_compatible\""),
            "tag missing: {json}"
        );
        let parsed: ProviderConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.model(), "qwen-72b");
    }

    #[test]
    fn provider_config_serde_rejects_unknown_tag() {
        let bad_json = json!({"provider": "nonexistent_llm", "model": "x", "api_key": "y"});
        let result: Result<ProviderConfig, _> = serde_json::from_value(bad_json);
        assert!(
            result.is_err(),
            "expected error for unknown provider tag"
        );
    }

    // ── Mock ModelProvider: trait + default health_check ─────────────────────

    /// Minimal mock that always returns a fixed response.
    struct MockProvider {
        config: ProviderConfig,
        response: ModelResponse,
    }

    #[async_trait]
    impl ModelProvider for MockProvider {
        async fn chat(&self, _request: ModelRequest) -> Result<ModelResponse, ModelError> {
            Ok(self.response.clone())
        }

        fn config(&self) -> &ProviderConfig {
            &self.config
        }
    }

    fn make_mock_request() -> ModelRequest {
        ModelRequest {
            messages: vec![json!({"role": "user", "content": "ping"})],
            tools: None,
            temperature: Some(0.0),
            max_tokens: Some(16),
            stop_sequences: None,
            response_format: None,
        }
    }

    fn make_mock_response() -> ModelResponse {
        ModelResponse {
            content: "pong".into(),
            finish_reason: FinishReason::Stop,
            usage: Usage {
                prompt_tokens: 5,
                completion_tokens: 1,
                total_tokens: 6,
            },
            tool_calls: None,
        }
    }

    #[tokio::test]
    async fn mock_provider_chat_returns_response() {
        let provider = MockProvider {
            config: ProviderConfig::OpenAi {
                api_key: "sk-test".into(),
                model: "gpt-4o".into(),
                base_url: None,
            },
            response: make_mock_response(),
        };

        let result = provider.chat(make_mock_request()).await
            .expect("mock chat should succeed");
        assert_eq!(result.content, "pong");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.total_tokens, 6);
    }

    #[tokio::test]
    async fn mock_provider_config_returns_correct_variant() {
        let provider = MockProvider {
            config: ProviderConfig::Anthropic {
                api_key: "anthro-k".into(),
                model: "claude-3-5-haiku".into(),
            },
            response: make_mock_response(),
        };

        assert_eq!(provider.config().model(), "claude-3-5-haiku");
    }

    #[tokio::test]
    async fn default_health_check_returns_ok() {
        let provider = MockProvider {
            config: ProviderConfig::Local {
                model: "llama-3".into(),
                base_url: "http://localhost:11434/v1".into(),
                api_key: None,
            },
            response: make_mock_response(),
        };

        provider
            .health_check()
            .await
            .expect("default health_check should return Ok");
    }

    /// Mock that always returns an error — used to test error propagation.
    struct FailingProvider {
        config: ProviderConfig,
        error: fn() -> ModelError,
    }

    #[async_trait]
    impl ModelProvider for FailingProvider {
        async fn chat(&self, _request: ModelRequest) -> Result<ModelResponse, ModelError> {
            Err((self.error)())
        }

        fn config(&self) -> &ProviderConfig {
            &self.config
        }
    }

    #[tokio::test]
    async fn failing_provider_propagates_authentication_error() {
        let provider = FailingProvider {
            config: ProviderConfig::OpenAi {
                api_key: "bad-key".into(),
                model: "gpt-4o".into(),
                base_url: None,
            },
            error: || ModelError::Authentication("401 Unauthorized".into()),
        };

        let err = provider.chat(make_mock_request()).await
            .expect_err("should return error");
        assert!(
            err.to_string().contains("authentication failed"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn failing_provider_propagates_rate_limit() {
        let provider = FailingProvider {
            config: ProviderConfig::OpenAi {
                api_key: "sk-test".into(),
                model: "gpt-4o".into(),
                base_url: None,
            },
            error: || ModelError::RateLimit,
        };

        let err = provider.chat(make_mock_request()).await
            .expect_err("should return error");
        assert_eq!(err.to_string(), "rate limit exceeded");
    }

    #[tokio::test]
    async fn failing_provider_propagates_context_length_exceeded() {
        let provider = FailingProvider {
            config: ProviderConfig::Anthropic {
                api_key: "k".into(),
                model: "claude-3-5-sonnet".into(),
            },
            error: || ModelError::ContextLengthExceeded,
        };

        let err = provider.chat(make_mock_request()).await
            .expect_err("should return error");
        assert_eq!(err.to_string(), "context length exceeded");
    }

    #[tokio::test]
    async fn failing_provider_propagates_not_available() {
        let provider = FailingProvider {
            config: ProviderConfig::Local {
                model: "llama-3".into(),
                base_url: "http://localhost:11434/v1".into(),
                api_key: None,
            },
            error: || ModelError::NotAvailable("connection refused".into()),
        };

        let err = provider.chat(make_mock_request()).await
            .expect_err("should return error");
        assert!(
            err.to_string().contains("provider not available"),
            "unexpected: {err}"
        );
    }

    #[tokio::test]
    async fn failing_provider_propagates_timeout() {
        let provider = FailingProvider {
            config: ProviderConfig::GoogleAi {
                api_key: "gem-k".into(),
                model: "gemini-2.0-flash".into(),
            },
            error: || ModelError::Timeout,
        };

        let err = provider.chat(make_mock_request()).await
            .expect_err("should return error");
        assert_eq!(err.to_string(), "timeout waiting for response");
    }
}
