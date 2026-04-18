//! LLM Router — multi-provider routing with failover and format normalization.

use std::time::Duration;

/// Supported LLM provider types.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    OpenAI,
    Anthropic,
    Ollama,
    Custom,
}

/// Configuration for an LLM provider.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub provider_type: ProviderType,
    pub api_url: String,
    pub api_key: Option<String>,
    pub default_model: String,
    pub priority: u32,
    pub enabled: bool,
    pub max_retries: u32,
    pub timeout_ms: u64,
}

/// A chat completion request (normalized format).
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stream: bool,
}

/// A chat message.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// A completion response (normalized format).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompletionResponse {
    pub content: String,
    pub model: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub provider: String,
}

/// Router health state for a provider.
struct ProviderHealth {
    failures: u32,
    last_failure: Option<std::time::Instant>,
    circuit_open: bool,
}

/// LLM Router with multi-provider failover.
pub struct LlmRouter {
    providers: Vec<ProviderConfig>,
    health: tokio::sync::RwLock<std::collections::HashMap<String, ProviderHealth>>,
    client: reqwest::Client,
    circuit_threshold: u32,
    circuit_reset_secs: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmRouterError {
    #[error("no available providers")]
    NoProviders,
    #[error("all providers failed")]
    AllProvidersFailed,
    #[error("provider error ({provider}): {message}")]
    ProviderError { provider: String, message: String },
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("circuit breaker open for provider: {0}")]
    CircuitOpen(String),
    #[error("timeout")]
    Timeout,
}

impl LlmRouter {
    /// Create a new LLM router with the given provider configs.
    pub fn new(mut providers: Vec<ProviderConfig>) -> Self {
        // Sort by priority (lower = higher priority)
        providers.sort_by_key(|p| p.priority);

        let health = providers
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    ProviderHealth {
                        failures: 0,
                        last_failure: None,
                        circuit_open: false,
                    },
                )
            })
            .collect();

        Self {
            providers,
            health: tokio::sync::RwLock::new(health),
            client: reqwest::Client::new(),
            circuit_threshold: 5,
            circuit_reset_secs: 30,
        }
    }

    /// Route a completion request, trying providers in priority order with failover.
    pub async fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, LlmRouterError> {
        let available = self.get_available_providers().await;
        if available.is_empty() {
            return Err(LlmRouterError::NoProviders);
        }

        let mut last_error = None;

        for provider in &available {
            match self.call_provider(provider, request).await {
                Ok(response) => {
                    self.record_success(&provider.name).await;
                    return Ok(response);
                }
                Err(e) => {
                    tracing::warn!(
                        provider = %provider.name,
                        error = %e,
                        "Provider failed, trying next"
                    );
                    self.record_failure(&provider.name).await;
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or(LlmRouterError::AllProvidersFailed))
    }

    /// Select the best provider for a request.
    pub async fn route_request(
        &self,
        _agent_id: &str,
        model_hint: Option<&str>,
    ) -> Result<ProviderConfig, LlmRouterError> {
        let available = self.get_available_providers().await;

        // If a model hint is provided, find a provider that supports it
        if let Some(hint) = model_hint
            && let Some(provider) = available
                .iter()
                .find(|p| p.default_model.contains(hint))
            {
                return Ok(provider.clone());
            }

        // Return the highest-priority available provider
        available
            .into_iter()
            .next()
            .ok_or(LlmRouterError::NoProviders)
    }

    /// Call a specific provider with the completion request.
    async fn call_provider(
        &self,
        provider: &ProviderConfig,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, LlmRouterError> {
        let url = format!("{}/chat/completions", provider.api_url.trim_end_matches('/'));
        let timeout = Duration::from_millis(provider.timeout_ms);

        let body = self.format_request(provider, request);

        let mut req = self
            .client
            .post(&url)
            .timeout(timeout)
            .json(&body);

        // Add auth header based on provider type
        if let Some(key) = &provider.api_key {
            req = match provider.provider_type {
                ProviderType::Anthropic => req
                    .header("x-api-key", key)
                    .header("anthropic-version", "2023-06-01"),
                _ => req.header("Authorization", format!("Bearer {key}")),
            };
        }

        let response = req.send().await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(LlmRouterError::ProviderError {
                provider: provider.name.clone(),
                message: format!("HTTP {status}: {text}"),
            });
        }

        let json: serde_json::Value = response.json().await?;
        self.parse_response(provider, &json)
    }

    /// Format request body based on provider type.
    fn format_request(
        &self,
        provider: &ProviderConfig,
        request: &CompletionRequest,
    ) -> serde_json::Value {
        match provider.provider_type {
            ProviderType::Anthropic => {
                serde_json::json!({
                    "model": request.model,
                    "max_tokens": request.max_tokens.unwrap_or(4096),
                    "messages": request.messages,
                })
            }
            _ => {
                // OpenAI-compatible format (works for OpenAI, Ollama, custom)
                let mut body = serde_json::json!({
                    "model": request.model,
                    "messages": request.messages,
                    "stream": request.stream,
                });
                if let Some(max) = request.max_tokens {
                    body["max_tokens"] = serde_json::json!(max);
                }
                if let Some(temp) = request.temperature {
                    body["temperature"] = serde_json::json!(temp);
                }
                body
            }
        }
    }

    /// Parse response based on provider type.
    fn parse_response(
        &self,
        provider: &ProviderConfig,
        json: &serde_json::Value,
    ) -> Result<CompletionResponse, LlmRouterError> {
        match provider.provider_type {
            ProviderType::Anthropic => {
                let content = json["content"]
                    .as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|c| c["text"].as_str())
                    .unwrap_or("")
                    .to_string();

                Ok(CompletionResponse {
                    content,
                    model: json["model"].as_str().unwrap_or("").to_string(),
                    input_tokens: json["usage"]["input_tokens"].as_u64(),
                    output_tokens: json["usage"]["output_tokens"].as_u64(),
                    provider: provider.name.clone(),
                })
            }
            _ => {
                let content = json["choices"]
                    .as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|c| c["message"]["content"].as_str())
                    .unwrap_or("")
                    .to_string();

                Ok(CompletionResponse {
                    content,
                    model: json["model"].as_str().unwrap_or("").to_string(),
                    input_tokens: json["usage"]["prompt_tokens"].as_u64(),
                    output_tokens: json["usage"]["completion_tokens"].as_u64(),
                    provider: provider.name.clone(),
                })
            }
        }
    }

    /// Get providers that are enabled and not circuit-broken.
    async fn get_available_providers(&self) -> Vec<ProviderConfig> {
        let health = self.health.read().await;
        let now = std::time::Instant::now();

        self.providers
            .iter()
            .filter(|p| {
                if !p.enabled {
                    return false;
                }
                if let Some(h) = health.get(&p.name)
                    && h.circuit_open {
                        // Check if reset period has passed
                        if let Some(last) = h.last_failure
                            && now.duration_since(last).as_secs() < self.circuit_reset_secs {
                                return false;
                            }
                    }
                true
            })
            .cloned()
            .collect()
    }

    /// Record a successful call (reset failure count).
    async fn record_success(&self, provider_name: &str) {
        let mut health = self.health.write().await;
        if let Some(h) = health.get_mut(provider_name) {
            h.failures = 0;
            h.circuit_open = false;
        }
    }

    /// Record a failed call (increment counter, potentially open circuit).
    async fn record_failure(&self, provider_name: &str) {
        let mut health = self.health.write().await;
        if let Some(h) = health.get_mut(provider_name) {
            h.failures += 1;
            h.last_failure = Some(std::time::Instant::now());
            if h.failures >= self.circuit_threshold {
                h.circuit_open = true;
                tracing::warn!(
                    provider = provider_name,
                    failures = h.failures,
                    "Circuit breaker opened"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_provider(name: &str, priority: u32) -> ProviderConfig {
        ProviderConfig {
            name: name.to_string(),
            provider_type: ProviderType::OpenAI,
            api_url: "http://localhost:8080".to_string(),
            api_key: Some("test-key".to_string()),
            default_model: "gpt-4".to_string(),
            priority,
            enabled: true,
            max_retries: 3,
            timeout_ms: 30000,
        }
    }

    #[test]
    fn test_provider_priority_sorting() {
        let router = LlmRouter::new(vec![
            test_provider("low-priority", 10),
            test_provider("high-priority", 1),
            test_provider("mid-priority", 5),
        ]);

        assert_eq!(router.providers[0].name, "high-priority");
        assert_eq!(router.providers[1].name, "mid-priority");
        assert_eq!(router.providers[2].name, "low-priority");
    }

    #[test]
    fn test_openai_request_format() {
        let router = LlmRouter::new(vec![test_provider("openai", 1)]);
        let request = CompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
            }],
            max_tokens: Some(100),
            temperature: Some(0.7),
            stream: false,
        };

        let body = router.format_request(&router.providers[0], &request);
        assert_eq!(body["model"], "gpt-4");
        assert_eq!(body["max_tokens"], 100);
        assert!(body["temperature"].as_f64().is_some_and(|t| (t - 0.7).abs() < 0.01));
    }

    #[test]
    fn test_anthropic_request_format() {
        let provider = ProviderConfig {
            name: "anthropic".to_string(),
            provider_type: ProviderType::Anthropic,
            api_url: "https://api.anthropic.com".to_string(),
            api_key: Some("test".to_string()),
            default_model: "claude-3-opus".to_string(),
            priority: 1,
            enabled: true,
            max_retries: 3,
            timeout_ms: 60000,
        };

        let router = LlmRouter::new(vec![provider.clone()]);
        let request = CompletionRequest {
            model: "claude-3-opus".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
            }],
            max_tokens: Some(200),
            temperature: None,
            stream: false,
        };

        let body = router.format_request(&provider, &request);
        assert_eq!(body["model"], "claude-3-opus");
        assert_eq!(body["max_tokens"], 200);
    }

    #[test]
    fn test_parse_openai_response() {
        let router = LlmRouter::new(vec![test_provider("openai", 1)]);
        let json = serde_json::json!({
            "choices": [{"message": {"content": "Hello there!"}}],
            "model": "gpt-4",
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        });

        let response = router.parse_response(&router.providers[0], &json).unwrap();
        assert_eq!(response.content, "Hello there!");
        assert_eq!(response.model, "gpt-4");
        assert_eq!(response.input_tokens, Some(10));
        assert_eq!(response.output_tokens, Some(5));
    }

    #[test]
    fn test_parse_anthropic_response() {
        let provider = ProviderConfig {
            provider_type: ProviderType::Anthropic,
            ..test_provider("anthropic", 1)
        };

        let router = LlmRouter::new(vec![provider.clone()]);
        let json = serde_json::json!({
            "content": [{"type": "text", "text": "Hi from Claude!"}],
            "model": "claude-3-opus",
            "usage": {"input_tokens": 8, "output_tokens": 4}
        });

        let response = router.parse_response(&provider, &json).unwrap();
        assert_eq!(response.content, "Hi from Claude!");
        assert_eq!(response.input_tokens, Some(8));
    }

    #[tokio::test]
    async fn test_circuit_breaker() {
        let router = LlmRouter::new(vec![test_provider("test", 1)]);

        // Record failures up to threshold
        for _ in 0..5 {
            router.record_failure("test").await;
        }

        let health = router.health.read().await;
        let h = health.get("test").unwrap();
        assert!(h.circuit_open);
        assert_eq!(h.failures, 5);
    }

    #[tokio::test]
    async fn test_available_providers_filters_disabled() {
        let mut disabled = test_provider("disabled", 1);
        disabled.enabled = false;

        let router = LlmRouter::new(vec![
            disabled,
            test_provider("enabled", 2),
        ]);

        let available = router.get_available_providers().await;
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].name, "enabled");
    }
}
