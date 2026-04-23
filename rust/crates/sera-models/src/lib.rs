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

pub mod account_pool;
pub mod provider;
pub mod routing;
pub mod sera_errors;
pub mod thinking;

pub use account_pool::{
    AccountGuard, AccountPool, AccountState, CooldownConfig, CooldownReason, PoolError,
    ProviderAccount,
};
// Re-export canonical SPEC-runtime model types from sera-types so downstream
// callers can keep using `sera_models::ModelResponse` / `ModelError` etc.
pub use sera_types::model::{FinishReason, ModelError, ModelResponse};
pub use provider::{Credential, ModelProvider, ProviderConfig, ProviderCredentials};
pub use routing::{
    AgentPreferences, CatalogError, CatalogRefreshConfig, CircuitConfig, CircuitState,
    HealthStore, ModelCatalogRegistry, ModelHealth, ModelInfo, ModelRef, ProviderCatalog,
    RoutingError, RoutingPolicy, StaticProviderCatalog, WeightedRoutingPolicy,
    WeightedScoreConfig,
};
pub use thinking::{ProviderKind, ReasoningLevel, ThinkingConfig};
