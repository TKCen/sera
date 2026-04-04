//! Core server configuration — loaded from environment variables.
//! Used by sera-core (the API server), not by agent containers.

use std::env;

use crate::providers::ProvidersConfig;

/// Full sera-core server configuration.
#[derive(Debug, Clone)]
pub struct CoreConfig {
    pub database_url: String,
    pub port: u16,
    pub api_key: String,
    pub llm: LlmConfig,
    pub centrifugo: CentrifugoConfig,
    pub qdrant: QdrantConfig,
    pub ollama: OllamaConfig,
    pub secrets_master_key: String,
    pub providers: Option<ProvidersConfig>,
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct CentrifugoConfig {
    pub api_url: String,
    pub api_key: String,
    pub token_secret: String,
}

#[derive(Debug, Clone)]
pub struct QdrantConfig {
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct OllamaConfig {
    pub url: String,
}

/// Errors during config loading.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required env var: {0}")]
    MissingEnvVar(String),
    #[error("failed to parse providers.json: {0}")]
    ProvidersParse(String),
    #[error("failed to read file: {0}")]
    FileRead(String),
}

impl CoreConfig {
    /// Load core configuration from environment variables.
    /// Providers config is loaded separately via `load_providers()`.
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = require_env("DATABASE_URL")?;

        let port = env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3001);

        let api_key = env::var("SERA_API_KEY")
            .unwrap_or_else(|_| "sera_bootstrap_dev_123".to_string());

        let llm = LlmConfig {
            base_url: env::var("LLM_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:1234/v1".to_string()),
            api_key: env::var("LLM_API_KEY")
                .unwrap_or_else(|_| "lm-studio".to_string()),
            model: env::var("LLM_MODEL")
                .unwrap_or_else(|_| "lmstudio-local".to_string()),
        };

        let centrifugo = CentrifugoConfig {
            api_url: env::var("CENTRIFUGO_API_URL")
                .unwrap_or_else(|_| "http://centrifugo:8000/api".to_string()),
            api_key: env::var("CENTRIFUGO_API_KEY")
                .unwrap_or_else(|_| "sera-api-key".to_string()),
            token_secret: env::var("CENTRIFUGO_TOKEN_SECRET")
                .unwrap_or_else(|_| "sera-token-secret".to_string()),
        };

        let qdrant = QdrantConfig {
            url: env::var("QDRANT_URL")
                .unwrap_or_else(|_| "http://qdrant:6333".to_string()),
        };

        let ollama = OllamaConfig {
            url: env::var("OLLAMA_URL")
                .unwrap_or_else(|_| "http://host.docker.internal:11434".to_string()),
        };

        let secrets_master_key = env::var("SECRETS_MASTER_KEY")
            .unwrap_or_else(|_| "sera-dev-master-key-change-me".to_string());

        Ok(Self {
            database_url,
            port,
            api_key,
            llm,
            centrifugo,
            qdrant,
            ollama,
            secrets_master_key,
            providers: None,
        })
    }

    /// Load and attach providers config from a JSON file.
    pub fn load_providers(&mut self, path: &str) -> Result<(), ConfigError> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::FileRead(format!("{path}: {e}")))?;
        let config: ProvidersConfig = serde_json::from_str(&contents)
            .map_err(|e| ConfigError::ProvidersParse(e.to_string()))?;
        self.providers = Some(config);
        Ok(())
    }
}

fn require_env(key: &str) -> Result<String, ConfigError> {
    env::var(key).map_err(|_| ConfigError::MissingEnvVar(key.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: these tests mutate process env vars and must run with `--test-threads=1`
    // for sera-config, or accept occasional flakiness from parallel execution.
    // In practice, cargo runs each crate's tests in a single binary so ordering
    // is the main concern.

    #[test]
    fn core_config_requires_database_url() {
        // Save, remove, test, restore
        let saved = env::var("DATABASE_URL").ok();
        unsafe { env::remove_var("DATABASE_URL") };
        let result = CoreConfig::from_env();
        if let Some(val) = &saved {
            unsafe { env::set_var("DATABASE_URL", val) };
        }
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("DATABASE_URL"));
    }

    #[test]
    fn core_config_defaults() {
        let saved = env::var("DATABASE_URL").ok();
        unsafe { env::set_var("DATABASE_URL", "postgres://test:test@localhost/sera") };
        let result = CoreConfig::from_env();
        // Restore original state
        match &saved {
            Some(val) => unsafe { env::set_var("DATABASE_URL", val) },
            None => unsafe { env::remove_var("DATABASE_URL") },
        }
        let config = result.unwrap();
        assert_eq!(config.port, 3001);
        assert_eq!(config.api_key, "sera_bootstrap_dev_123");
        assert_eq!(config.llm.model, "lmstudio-local");
        assert_eq!(config.centrifugo.token_secret, "sera-token-secret");
    }
}
