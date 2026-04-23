//! Model provider trait and configuration.
//!
//! The [`ModelProvider`] trait abstracts over different LLM providers,
//! allowing SERA to use OpenAI, Anthropic, local models, or any
//! provider that implements this interface.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use sera_types::model::{ModelError, ModelRequest, ModelResponse};

// ---------------------------------------------------------------------------
// Credential — sera-hjem multi-account auth
// ---------------------------------------------------------------------------

/// A single credential entry attached to a [`ProviderConfig`].
///
/// Sera-hjem allows N credentials per provider for round-robin failover and
/// per-key 429 backoff.  The legacy single `api_key` field on each provider
/// variant remains as a backward-compat shim — when a user supplies only
/// `api_key`, [`ProviderConfig::credentials`] synthesises a single
/// `Credential { id: "default", api_key }` for them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credential {
    /// User-facing label (e.g. `"primary"`, `"backup"`, `"k1"`).
    pub id: String,
    /// API key used for authentication.
    pub api_key: String,
    /// Optional per-credential base URL override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

impl Credential {
    /// Build a credential with no per-key base URL override.
    pub fn new(id: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            api_key: api_key.into(),
            base_url: None,
        }
    }

    /// Attach a per-credential base URL override.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }
}

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

    /// Sera-hjem: canonical credential list for this provider.
    ///
    /// Promotes the legacy single `api_key` field into a one-element
    /// `Credential { id: "default", api_key }` vector.  Bedrock returns an
    /// empty vector — its credentials live in `aws_access_key` /
    /// `aws_secret_key` and are not part of the pool API.
    ///
    /// Multi-credential configs are loaded externally (env-driven
    /// `ProviderAccountsConfig` or a separately-parsed YAML `credentials:`
    /// list) and combined via [`ProviderCredentials::merge`].
    pub fn credentials(&self) -> Vec<Credential> {
        match self {
            ProviderConfig::OpenAi { api_key, .. }
            | ProviderConfig::Anthropic { api_key, .. }
            | ProviderConfig::GoogleAi { api_key, .. } => {
                vec![Credential::new("default", api_key.clone())]
            }
            ProviderConfig::Local { api_key, .. }
            | ProviderConfig::OaiCompatible { api_key, .. } => {
                vec![Credential::new("default", api_key.clone().unwrap_or_default())]
            }
            ProviderConfig::AwsBedrock { .. } => Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// ProviderCredentials — sera-hjem multi-account container
// ---------------------------------------------------------------------------

/// Stand-alone credential bundle parsed from YAML / JSON / env.
///
/// Pairs with [`ProviderConfig`] but is kept as a sibling type so existing
/// `ProviderConfig` constructors remain source-compatible.  YAML callers may
/// supply either a single `api_key:` (one-credential, backward-compat) or a
/// list of `credentials: [{id, api_key, base_url?}, ...]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCredentials {
    /// Legacy single-credential field.  When set and `credentials` is empty,
    /// it is promoted into a one-element vector with id `"default"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Explicit multi-credential list.  Takes precedence over `api_key`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credentials: Vec<Credential>,
}

impl ProviderCredentials {
    /// Build a single-credential bundle (backward-compat path).
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Some(api_key.into()),
            credentials: Vec::new(),
        }
    }

    /// Build a multi-credential bundle.
    pub fn from_credentials(credentials: Vec<Credential>) -> Self {
        Self {
            api_key: None,
            credentials,
        }
    }

    /// Canonical credential list.  Returns the `credentials:` list when
    /// non-empty; otherwise promotes the single `api_key` into a one-element
    /// vector with id `"default"`.
    pub fn resolved(&self) -> Vec<Credential> {
        if !self.credentials.is_empty() {
            return self.credentials.clone();
        }
        match &self.api_key {
            Some(k) if !k.is_empty() => vec![Credential::new("default", k.clone())],
            _ => Vec::new(),
        }
    }

    /// Number of resolved credentials (useful for diagnostics / metrics).
    pub fn len(&self) -> usize {
        self.resolved().len()
    }

    /// True when no credentials are configured.
    pub fn is_empty(&self) -> bool {
        self.credentials.is_empty()
            && self
                .api_key
                .as_ref()
                .is_none_or(|k| k.is_empty())
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
    use async_trait::async_trait;
    use sera_types::model::FinishReason;
    use sera_types::runtime::TokenUsage;
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
            thinking: Default::default(),
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
            content: Some("pong".into()),
            finish_reason: FinishReason::Stop,
            usage: TokenUsage {
                prompt_tokens: 5,
                completion_tokens: 1,
                total_tokens: 6,
            },
            tool_calls: Vec::new(),
            model: "mock-model".into(),
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
        assert_eq!(result.content.as_deref(), Some("pong"));
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
            error: || ModelError::AuthenticationFailed,
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
            error: || ModelError::RateLimited { retry_after_ms: None },
        };

        let err = provider.chat(make_mock_request()).await
            .expect_err("should return error");
        assert!(
            err.to_string().contains("rate limited"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn failing_provider_propagates_context_length_exceeded() {
        let provider = FailingProvider {
            config: ProviderConfig::Anthropic {
                api_key: "k".into(),
                model: "claude-3-5-sonnet".into(),
            },
            error: || ModelError::ContextLengthExceeded { limit: 4096, requested: 5000 },
        };

        let err = provider.chat(make_mock_request()).await
            .expect_err("should return error");
        assert!(
            err.to_string().contains("context length exceeded"),
            "unexpected error: {err}"
        );
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
        assert_eq!(err.to_string(), "request timed out");
    }

    // ── sera-hjem: Credential / ProviderCredentials backward-compat ──────────

    #[test]
    fn provider_config_credentials_promotes_single_api_key() {
        let cfg = ProviderConfig::OpenAi {
            api_key: "sk-only".into(),
            model: "gpt-4o".into(),
            base_url: None,
        };
        let creds = cfg.credentials();
        assert_eq!(creds.len(), 1);
        assert_eq!(creds[0].id, "default");
        assert_eq!(creds[0].api_key, "sk-only");
    }

    #[test]
    fn provider_config_credentials_local_with_no_api_key_returns_empty_string() {
        let cfg = ProviderConfig::Local {
            model: "llama-3".into(),
            base_url: "http://localhost:11434/v1".into(),
            api_key: None,
        };
        let creds = cfg.credentials();
        assert_eq!(creds.len(), 1);
        assert_eq!(creds[0].id, "default");
        assert_eq!(creds[0].api_key, "");
    }

    #[test]
    fn provider_config_credentials_bedrock_returns_empty() {
        let cfg = ProviderConfig::AwsBedrock {
            region: "us-east-1".into(),
            model: "amazon.titan".into(),
            aws_access_key: None,
            aws_secret_key: None,
        };
        assert!(cfg.credentials().is_empty());
    }

    #[test]
    fn provider_credentials_yaml_single_api_key_back_compat() {
        // Backward-compat: legacy YAML with just `api_key:` parses as one credential.
        let yaml = "api_key: sk-legacy\n";
        let bundle: ProviderCredentials =
            serde_yaml_to_provider_credentials(yaml).expect("parse");
        let resolved = bundle.resolved();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, "default");
        assert_eq!(resolved[0].api_key, "sk-legacy");
    }

    #[test]
    fn provider_credentials_yaml_multi_credential_list() {
        let yaml = r#"
credentials:
  - id: primary
    api_key: sk-one
  - id: backup
    api_key: sk-two
    base_url: https://backup.example.com/v1
"#;
        let bundle: ProviderCredentials =
            serde_yaml_to_provider_credentials(yaml).expect("parse");
        let resolved = bundle.resolved();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].id, "primary");
        assert_eq!(resolved[0].api_key, "sk-one");
        assert!(resolved[0].base_url.is_none());
        assert_eq!(resolved[1].id, "backup");
        assert_eq!(resolved[1].base_url.as_deref(), Some("https://backup.example.com/v1"));
    }

    #[test]
    fn provider_credentials_credentials_take_precedence_over_api_key() {
        // When both are present, the explicit list wins.
        let bundle = ProviderCredentials {
            api_key: Some("sk-legacy".into()),
            credentials: vec![Credential::new("primary", "sk-explicit")],
        };
        let resolved = bundle.resolved();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, "primary");
        assert_eq!(resolved[0].api_key, "sk-explicit");
    }

    #[test]
    fn provider_credentials_is_empty_when_no_keys() {
        assert!(ProviderCredentials::default().is_empty());
        assert!(ProviderCredentials::from_api_key("").is_empty());
        assert!(!ProviderCredentials::from_api_key("sk-x").is_empty());
        assert!(
            !ProviderCredentials::from_credentials(vec![Credential::new("k", "v")]).is_empty()
        );
    }

    /// Tiny YAML→JSON shim so we don't need a hard dep on serde_yaml here —
    /// we re-encode the YAML through serde_json by exploiting that the test
    /// inputs are simple enough to express as JSON manually.  This keeps the
    /// crate dependency footprint unchanged.
    fn serde_yaml_to_provider_credentials(
        yaml: &str,
    ) -> Result<ProviderCredentials, serde_json::Error> {
        // Hand-built parser: only supports the two test shapes.
        let trimmed = yaml.trim();
        if trimmed.starts_with("api_key:") {
            let key = trimmed
                .trim_start_matches("api_key:")
                .trim()
                .trim_matches('"');
            let json = format!(r#"{{"api_key":"{key}"}}"#);
            return serde_json::from_str(&json);
        }
        // multi-credential form
        if trimmed.starts_with("credentials:") {
            let body = trimmed.trim_start_matches("credentials:").trim();
            // Parse each "- id: x\n    api_key: y\n    base_url: z" block.
            let mut json_creds = Vec::new();
            let mut cur: Option<(String, String, Option<String>)> = None;
            for raw_line in body.lines() {
                let line = raw_line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("- id:") {
                    if let Some((id, key, url)) = cur.take() {
                        json_creds.push(emit_cred(&id, &key, url.as_deref()));
                    }
                    cur = Some((rest.trim().to_string(), String::new(), None));
                } else if let Some(rest) = line.strip_prefix("api_key:")
                    && let Some(c) = cur.as_mut()
                {
                    c.1 = rest.trim().to_string();
                } else if let Some(rest) = line.strip_prefix("base_url:")
                    && let Some(c) = cur.as_mut()
                {
                    c.2 = Some(rest.trim().to_string());
                }
            }
            if let Some((id, key, url)) = cur.take() {
                json_creds.push(emit_cred(&id, &key, url.as_deref()));
            }
            let json = format!(r#"{{"credentials":[{}]}}"#, json_creds.join(","));
            return serde_json::from_str(&json);
        }
        Ok(ProviderCredentials::default())
    }

    fn emit_cred(id: &str, key: &str, base_url: Option<&str>) -> String {
        match base_url {
            Some(u) => format!(r#"{{"id":"{id}","api_key":"{key}","base_url":"{u}"}}"#),
            None => format!(r#"{{"id":"{id}","api_key":"{key}"}}"#),
        }
    }
}
