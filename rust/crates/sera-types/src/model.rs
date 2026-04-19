//! Model adapter trait and associated types.
//!
//! Defines the `ModelAdapter` pluggable interface per SPEC-runtime §5.
//! All model providers (local, API, gRPC-bridged) implement this trait.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ── PersonaConfig ─────────────────────────────────────────────────────────────

/// Structured persona format for the context assembly pipeline (SPEC-runtime §4.3).
///
/// Splits the system prompt into an immutable safety anchor (operator-only) and
/// a mutable persona section the agent can propose modifications to via
/// `config_propose`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaConfig {
    /// Core safety directives, foundational identity, behavioral boundaries.
    /// The system CANNOT modify this section. Administered by operators only.
    pub immutable_anchor: String,

    /// Adaptable behavioral traits, tone, style, domain expertise.
    /// The agent CAN propose modifications to this section (via config_propose).
    pub mutable_persona: String,

    /// Maximum token budget for the mutable_persona section.
    /// When exceeded, an introspection workflow is triggered.
    pub mutable_token_budget: u32,
}

// ── ResponseFormat ────────────────────────────────────────────────────────────

/// Constrained output format for a model request (SPEC-runtime §5.1).
///
/// Provider adapters translate this to the appropriate mechanism:
/// - OpenAI/Gemini: `response_format` with JSON schema
/// - vLLM/SGLang: guided decoding / structured output API
/// - llama.cpp: GBNF grammar constraint
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "schema", rename_all = "snake_case")]
pub enum ResponseFormat {
    /// Plain text output — no constraints.
    Text,
    /// Output must be valid JSON (schema not enforced).
    Json,
    /// Output must be valid JSON matching the provided schema.
    JsonSchema(serde_json::Value),
}

// ── ModelRequest ──────────────────────────────────────────────────────────────

/// A request to a model adapter for chat completion (SPEC-runtime §5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequest {
    /// Conversation messages in OpenAI-format (role + content).
    pub messages: Vec<serde_json::Value>,

    /// Tool schemas available to the model — enables function calling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<crate::tool::ToolDefinition>>,

    /// Sampling temperature — controls output randomness (0.0–2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Maximum number of tokens in the completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Token sequences that stop generation when encountered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Constrain output to a specific format (SPEC-runtime §5.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

// ── FinishReason ──────────────────────────────────────────────────────────────

/// Why the model stopped generating output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Model produced a natural stop (end of response or stop sequence hit).
    Stop,
    /// Model issued one or more tool calls and stopped to await results.
    ToolCalls,
    /// Output was truncated at `max_tokens`.
    Length,
    /// Output was blocked by content filter.
    ContentFilter,
}

// ── ModelResponse ─────────────────────────────────────────────────────────────

/// The response from a model adapter's chat completion call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelResponse {
    /// Text content of the response, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Tool calls made by the model (empty if none).
    #[serde(default)]
    pub tool_calls: Vec<crate::runtime::ToolCall>,

    /// Token usage for this completion.
    pub usage: crate::runtime::TokenUsage,

    /// The model identifier that produced this response.
    pub model: String,

    /// Why the model stopped generating.
    pub finish_reason: FinishReason,
}

// ── ModelError ────────────────────────────────────────────────────────────────

/// Errors returned by a `ModelAdapter` implementation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ModelError {
    /// The provider returned an unclassified error.
    #[error("provider error: {0}")]
    ProviderError(String),

    /// The provider is rate-limiting this client.
    #[error("rate limited (retry after {retry_after_ms:?} ms)")]
    RateLimited {
        /// Milliseconds to wait before retrying, if the provider supplied one.
        retry_after_ms: Option<u64>,
    },

    /// The combined prompt + completion exceeds the model's context window.
    #[error("context length exceeded: limit {limit}, requested {requested}")]
    ContextLengthExceeded { limit: u32, requested: u32 },

    /// API key or credentials are missing / invalid.
    #[error("authentication failed")]
    AuthenticationFailed,

    /// The request payload is malformed or rejected by the provider.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// The request timed out before a response was received.
    #[error("request timed out")]
    Timeout,
}

// ── ModelAdapter trait ────────────────────────────────────────────────────────

/// Pluggable model provider interface (SPEC-runtime §5).
///
/// Implement this trait for every model backend (Anthropic, OpenAI, local vLLM,
/// LM Studio, etc.). The default runtime calls this trait exclusively — it never
/// speaks directly to a provider API.
#[async_trait]
pub trait ModelAdapter: Send + Sync {
    /// Send a chat completion request and return the full response.
    async fn chat_completion(&self, request: ModelRequest) -> Result<ModelResponse, ModelError>;

    /// Return the canonical model identifier (e.g. `"gpt-4o"`, `"claude-3-5-sonnet"`).
    fn model_name(&self) -> &str;

    /// Whether this adapter supports tool/function calling.
    fn supports_tools(&self) -> bool;

    /// Whether this adapter supports token-streaming responses.
    fn supports_streaming(&self) -> bool;

    /// Maximum number of tokens in the model's context window.
    fn max_context_tokens(&self) -> u32;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{TokenUsage, ToolCall};

    // ── PersonaConfig ─────────────────────────────────────────────────────────

    #[test]
    fn persona_config_serde_roundtrip() {
        let config = PersonaConfig {
            immutable_anchor: "You are Sera. You never reveal secrets.".to_string(),
            mutable_persona: "You prefer Rust over Python.".to_string(),
            mutable_token_budget: 300,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: PersonaConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.immutable_anchor, config.immutable_anchor);
        assert_eq!(parsed.mutable_persona, config.mutable_persona);
        assert_eq!(parsed.mutable_token_budget, 300);
    }

    // ── ModelRequest ──────────────────────────────────────────────────────────

    #[test]
    fn model_request_with_tools() {
        use crate::tool::{FunctionDefinition, FunctionParameters, ToolDefinition};

        let tool = ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "memory_read".to_string(),
                description: "Read a memory file".to_string(),
                parameters: FunctionParameters {
                    schema_type: "object".to_string(),
                    properties: std::collections::HashMap::new(),
                    required: vec![],
                },
            },
        };

        let request = ModelRequest {
            messages: vec![serde_json::json!({"role": "user", "content": "Hello"})],
            tools: Some(vec![tool]),
            temperature: Some(0.7),
            max_tokens: Some(1024),
            stop_sequences: None,
            response_format: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: ModelRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.messages.len(), 1);
        assert!(parsed.tools.is_some());
        assert_eq!(parsed.tools.as_ref().unwrap().len(), 1);
        assert_eq!(
            parsed.tools.as_ref().unwrap()[0].function.name,
            "memory_read"
        );
        assert_eq!(parsed.temperature, Some(0.7));
        assert_eq!(parsed.max_tokens, Some(1024));
        assert!(parsed.stop_sequences.is_none());
    }

    #[test]
    fn model_request_minimal() {
        let request = ModelRequest {
            messages: vec![],
            tools: None,
            temperature: None,
            max_tokens: None,
            stop_sequences: None,
            response_format: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        // Optional fields should be omitted
        assert!(!json.contains("tools"));
        assert!(!json.contains("temperature"));
        assert!(!json.contains("max_tokens"));
        assert!(!json.contains("stop_sequences"));
        assert!(!json.contains("response_format"));
    }

    // ── ModelResponse ─────────────────────────────────────────────────────────

    #[test]
    fn model_response_construction() {
        let response = ModelResponse {
            content: Some("Here is your answer.".to_string()),
            tool_calls: vec![],
            usage: TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
            },
            model: "claude-3-5-sonnet".to_string(),
            finish_reason: FinishReason::Stop,
        };

        assert_eq!(response.content.as_deref(), Some("Here is your answer."));
        assert!(response.tool_calls.is_empty());
        assert_eq!(response.usage.total_tokens, 150);
        assert_eq!(response.model, "claude-3-5-sonnet");
        assert_eq!(response.finish_reason, FinishReason::Stop);
    }

    #[test]
    fn model_response_with_tool_calls() {
        let response = ModelResponse {
            content: None,
            tool_calls: vec![ToolCall {
                id: "call-abc".to_string(),
                name: "memory_read".to_string(),
                arguments: serde_json::json!({"path": "notes.md"}),
                result: None,
            }],
            usage: TokenUsage {
                prompt_tokens: 200,
                completion_tokens: 30,
                total_tokens: 230,
            },
            model: "gpt-4o".to_string(),
            finish_reason: FinishReason::ToolCalls,
        };

        assert!(response.content.is_none());
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "memory_read");
        assert_eq!(response.finish_reason, FinishReason::ToolCalls);
    }

    #[test]
    fn model_response_serde_roundtrip() {
        let response = ModelResponse {
            content: Some("Done.".to_string()),
            tool_calls: vec![],
            usage: TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
            model: "gemma-4-12b".to_string(),
            finish_reason: FinishReason::Stop,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: ModelResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, response.content);
        assert_eq!(parsed.model, response.model);
        assert_eq!(parsed.finish_reason, response.finish_reason);
        assert_eq!(parsed.usage.total_tokens, 15);
    }

    // ── FinishReason ──────────────────────────────────────────────────────────

    #[test]
    fn finish_reason_serde() {
        let cases = vec![
            (FinishReason::Stop, "stop"),
            (FinishReason::ToolCalls, "tool_calls"),
            (FinishReason::Length, "length"),
            (FinishReason::ContentFilter, "content_filter"),
        ];
        for (reason, expected) in cases {
            let json = serde_json::to_string(&reason).unwrap();
            assert_eq!(json, format!("\"{}\"", expected));
            let parsed: FinishReason = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, reason);
        }
    }

    // ── ModelError ────────────────────────────────────────────────────────────

    #[test]
    fn model_error_display() {
        assert_eq!(
            ModelError::ProviderError("quota exceeded".to_string()).to_string(),
            "provider error: quota exceeded"
        );

        assert_eq!(
            ModelError::RateLimited { retry_after_ms: Some(5000) }.to_string(),
            "rate limited (retry after Some(5000) ms)"
        );

        assert_eq!(
            ModelError::RateLimited { retry_after_ms: None }.to_string(),
            "rate limited (retry after None ms)"
        );

        assert_eq!(
            ModelError::ContextLengthExceeded { limit: 4096, requested: 5000 }.to_string(),
            "context length exceeded: limit 4096, requested 5000"
        );

        assert_eq!(
            ModelError::AuthenticationFailed.to_string(),
            "authentication failed"
        );

        assert_eq!(
            ModelError::InvalidRequest("missing model".to_string()).to_string(),
            "invalid request: missing model"
        );

        assert_eq!(ModelError::Timeout.to_string(), "request timed out");
    }

    // ── ResponseFormat ────────────────────────────────────────────────────────

    #[test]
    fn response_format_serde() {
        let text = ResponseFormat::Text;
        let json_fmt = ResponseFormat::Json;
        let schema = ResponseFormat::JsonSchema(serde_json::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        }));

        let text_json = serde_json::to_string(&text).unwrap();
        let parsed_text: ResponseFormat = serde_json::from_str(&text_json).unwrap();
        assert!(matches!(parsed_text, ResponseFormat::Text));

        let json_str = serde_json::to_string(&json_fmt).unwrap();
        let parsed_json: ResponseFormat = serde_json::from_str(&json_str).unwrap();
        assert!(matches!(parsed_json, ResponseFormat::Json));

        let schema_str = serde_json::to_string(&schema).unwrap();
        let parsed_schema: ResponseFormat = serde_json::from_str(&schema_str).unwrap();
        assert!(matches!(parsed_schema, ResponseFormat::JsonSchema(_)));
    }
}
