//! `sera-models` — Model provider abstractions for SERA.
//!
//! Provides the [`ModelProvider`] trait that abstracts LLM client interactions
//! away from specific providers (OpenAI, Anthropic, local, etc.).
//!
//! # Overview
//!
//! - [`ModelProvider`] — core trait for sending model requests and receiving responses
//! - [`ModelResponse`] — structured response from the model
//! - [`ProviderConfig`] — configuration for different provider types
//!
//! # Example
//!
//! ```rust,ignore
//! use sera_models::{ModelProvider, ProviderConfig, OpenAiProvider};
//!
//! let config = ProviderConfig::OpenAi {
//!     api_key: "sk-...".into(),
//!     model: "gpt-4o".into(),
//!     base_url: None,
//! };
//! let provider = OpenAiProvider::new(config).await?;
//! let response = provider.chat(request).await?;
//! ```

pub mod error;
pub mod provider;
pub mod response;
pub mod routing;

pub use error::ModelError;
pub use provider::{ModelProvider, ProviderConfig};
pub use response::ModelResponse;
pub use routing::{
    AgentPreferences, HealthStore, ModelHealth, ModelRef, RoutingError, RoutingPolicy,
    WeightedRoutingPolicy, WeightedScoreConfig,
};
