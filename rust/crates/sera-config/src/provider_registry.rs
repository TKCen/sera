//! LLM provider registry — data, not code.
//!
//! Every provider is a [`ProviderEntry`] in the static [`PROVIDER_REGISTRY`]
//! table.  Per-provider behaviour is encoded as one of four [`AuthStrategy`]
//! variants; the wizard / runtime dispatch on the strategy, NOT on the
//! provider id.  Adding a new provider is a one-line table addition.

/// Authentication strategy supported by SERA.
///
/// All providers map to exactly one of these.  Each strategy has a single
/// shared implementation so the surface scales O(1) with provider count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthStrategy {
    /// Static API key (env var or OS keychain).
    ApiKey,
    /// OAuth 2.0 device-code flow.
    OAuthDevice,
    /// OAuth 2.0 authorization-code flow with browser redirect.
    OAuthExternal,
    /// User-supplied base URL with optional API key (LM Studio, Ollama, vLLM…).
    CustomEndpoint,
}

/// Static registry entry describing a single LLM provider.
#[derive(Debug, Clone, Copy)]
pub struct ProviderEntry {
    /// Stable slug used in YAML manifests and CLI args.
    pub id: &'static str,
    /// Human-readable name shown in CLI / wizard prompts.
    pub display_name: &'static str,
    /// Authentication strategy this provider uses.
    pub auth_strategy: AuthStrategy,
    /// Default base URL — pre-filled in the wizard, may be overridden.
    pub default_base_url: Option<&'static str>,
    /// Optional URL for `sera provider models` to fetch the model list from.
    pub model_list_url: Option<&'static str>,
    /// Environment variables that may carry the API key (checked in order).
    pub env_keys: &'static [&'static str],
}

const PROVIDER_REGISTRY: &[ProviderEntry] = &[
    ProviderEntry {
        id: "openrouter",
        display_name: "OpenRouter",
        auth_strategy: AuthStrategy::ApiKey,
        default_base_url: Some("https://openrouter.ai/api/v1"),
        model_list_url: Some("https://openrouter.ai/api/v1/models"),
        env_keys: &["OPENROUTER_API_KEY"],
    },
    ProviderEntry {
        // Anthropic OAuth device flow is deferred to v1.1; ApiKey is sufficient
        // for now since ANTHROPIC_API_KEY works against the public API.
        id: "anthropic",
        display_name: "Anthropic",
        auth_strategy: AuthStrategy::ApiKey,
        default_base_url: Some("https://api.anthropic.com/v1"),
        model_list_url: None,
        env_keys: &["ANTHROPIC_API_KEY"],
    },
    ProviderEntry {
        id: "openai",
        display_name: "OpenAI",
        auth_strategy: AuthStrategy::ApiKey,
        default_base_url: Some("https://api.openai.com/v1"),
        model_list_url: Some("https://api.openai.com/v1/models"),
        env_keys: &["OPENAI_API_KEY"],
    },
    ProviderEntry {
        id: "lm-studio",
        display_name: "LM Studio (local)",
        auth_strategy: AuthStrategy::CustomEndpoint,
        default_base_url: Some("http://localhost:1234/v1"),
        model_list_url: Some("http://localhost:1234/v1/models"),
        env_keys: &[],
    },
    ProviderEntry {
        id: "ollama",
        display_name: "Ollama (local)",
        auth_strategy: AuthStrategy::CustomEndpoint,
        default_base_url: Some("http://localhost:11434/v1"),
        model_list_url: Some("http://localhost:11434/v1/models"),
        env_keys: &[],
    },
];

/// All built-in provider entries.
pub fn registry() -> &'static [ProviderEntry] {
    PROVIDER_REGISTRY
}

/// Look up a provider entry by id.  Returns `None` when no entry matches.
pub fn lookup(id: &str) -> Option<&'static ProviderEntry> {
    PROVIDER_REGISTRY.iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_has_at_least_three_providers() {
        assert!(registry().len() >= 3);
    }

    #[test]
    fn lookup_finds_known_provider() {
        let entry = lookup("openrouter").unwrap();
        assert_eq!(entry.id, "openrouter");
        assert_eq!(entry.auth_strategy, AuthStrategy::ApiKey);
        assert_eq!(entry.env_keys, &["OPENROUTER_API_KEY"]);
    }

    #[test]
    fn lookup_returns_none_for_unknown() {
        assert!(lookup("does-not-exist").is_none());
    }

    #[test]
    fn ids_are_unique() {
        let mut seen = HashSet::new();
        for entry in registry() {
            assert!(seen.insert(entry.id), "duplicate provider id: {}", entry.id);
        }
    }

    #[test]
    fn custom_endpoint_providers_have_default_url() {
        for entry in registry() {
            if entry.auth_strategy == AuthStrategy::CustomEndpoint {
                assert!(
                    entry.default_base_url.is_some(),
                    "{} (CustomEndpoint) must have default_base_url",
                    entry.id
                );
            }
        }
    }

    #[test]
    fn api_key_providers_declare_env_var() {
        for entry in registry() {
            if entry.auth_strategy == AuthStrategy::ApiKey {
                assert!(
                    !entry.env_keys.is_empty(),
                    "{} (ApiKey) must declare at least one env var",
                    entry.id
                );
            }
        }
    }

    #[test]
    fn local_providers_use_custom_endpoint() {
        let lm = lookup("lm-studio").unwrap();
        assert_eq!(lm.auth_strategy, AuthStrategy::CustomEndpoint);
        let ollama = lookup("ollama").unwrap();
        assert_eq!(ollama.auth_strategy, AuthStrategy::CustomEndpoint);
    }
}
