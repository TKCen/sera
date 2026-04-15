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
