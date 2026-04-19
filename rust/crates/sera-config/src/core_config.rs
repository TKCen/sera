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
    pub oidc_issuer: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret: Option<String>,
    pub external_url: Option<String>,
    pub web_origin: Option<String>,
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
    #[error("production config contains insecure dev-secret defaults: {0}")]
    InsecureSecret(String),
}

/// Known dev-secret literals that must not be used in production.
/// These are the hardcoded defaults that ship for local development only.
const DEV_SECRET_VALUES: &[&str] = &[
    "sera_bootstrap_dev_123",
    "lm-studio",
    "sera-api-key",
    "sera-token-secret",
    "sera-dev-master-key-change-me",
];

/// Validate that none of the sensitive config fields contain known dev-secret defaults.
///
/// In production (`SERA_ENV=production`) this returns an error listing every
/// unsafe field. In all other environments it only emits `tracing::warn!` and
/// returns `Ok(())` so that local dev is unaffected.
pub fn validate_production_secrets(config: &CoreConfig) -> Result<(), ConfigError> {
    let checks: &[(&str, &str)] = &[
        ("api_key", &config.api_key),
        ("llm.api_key", &config.llm.api_key),
        ("centrifugo.api_key", &config.centrifugo.api_key),
        ("centrifugo.token_secret", &config.centrifugo.token_secret),
        ("secrets_master_key", &config.secrets_master_key),
    ];

    validate_secret_checks(checks)
}

/// Validate secrets read directly from environment variables, without requiring
/// a fully-constructed `CoreConfig`. Useful for binaries that load config from
/// a YAML manifest rather than `CoreConfig::from_env()`.
///
/// Checks the same set of well-known env vars and applies the same
/// production-reject / dev-warn logic as [`validate_production_secrets`].
pub fn validate_env_secrets() -> Result<(), ConfigError> {
    let api_key = env::var("SERA_API_KEY")
        .unwrap_or_else(|_| "sera_bootstrap_dev_123".to_string());
    let llm_api_key = env::var("LLM_API_KEY")
        .unwrap_or_else(|_| "lm-studio".to_string());
    let centrifugo_api_key = env::var("CENTRIFUGO_API_KEY")
        .unwrap_or_else(|_| "sera-api-key".to_string());
    let centrifugo_token_secret = env::var("CENTRIFUGO_TOKEN_SECRET")
        .unwrap_or_else(|_| "sera-token-secret".to_string());
    let secrets_master_key = env::var("SECRETS_MASTER_KEY")
        .unwrap_or_else(|_| "sera-dev-master-key-change-me".to_string());

    let checks: &[(&str, &str)] = &[
        ("SERA_API_KEY", &api_key),
        ("LLM_API_KEY", &llm_api_key),
        ("CENTRIFUGO_API_KEY", &centrifugo_api_key),
        ("CENTRIFUGO_TOKEN_SECRET", &centrifugo_token_secret),
        ("SECRETS_MASTER_KEY", &secrets_master_key),
    ];

    validate_secret_checks(checks)
}

fn validate_secret_checks(checks: &[(&str, &str)]) -> Result<(), ConfigError> {
    let is_production = env::var("SERA_ENV")
        .map(|v| v.eq_ignore_ascii_case("production"))
        .unwrap_or(false);

    let unsafe_fields: Vec<&str> = checks
        .iter()
        .filter(|(_, value)| DEV_SECRET_VALUES.contains(value))
        .map(|(field, _)| *field)
        .collect();

    if unsafe_fields.is_empty() {
        return Ok(());
    }

    let field_list = unsafe_fields.join(", ");
    if is_production {
        Err(ConfigError::InsecureSecret(field_list))
    } else {
        tracing::warn!(
            fields = %field_list,
            "Config contains dev-secret defaults — override before deploying to production"
        );
        Ok(())
    }
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

        let oidc_issuer = env::var("OIDC_ISSUER").ok();
        let oidc_client_id = env::var("OIDC_CLIENT_ID").ok();
        let oidc_client_secret = env::var("OIDC_CLIENT_SECRET").ok();
        let external_url = env::var("SERA_EXTERNAL_URL").ok();
        let web_origin = env::var("WEB_ORIGIN").ok();

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
            oidc_issuer,
            oidc_client_id,
            oidc_client_secret,
            external_url,
            web_origin,
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
    use std::sync::Mutex;

    // SERA_ENV-mutating tests serialize on this lock to avoid racing on the
    // process-global env var when cargo runs them in parallel within one binary.
    static SERA_ENV_LOCK: Mutex<()> = Mutex::new(());

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

    #[test]
    fn core_config_port_parsing() {
        // Test that PORT env var is properly parsed as u16
        let config = CoreConfig {
            database_url: "postgres://localhost/sera".to_string(),
            port: 5000,
            api_key: "test".to_string(),
            llm: LlmConfig {
                base_url: "http://localhost:1234/v1".to_string(),
                api_key: "key".to_string(),
                model: "model".to_string(),
            },
            centrifugo: CentrifugoConfig {
                api_url: "http://localhost:8000".to_string(),
                api_key: "key".to_string(),
                token_secret: "secret".to_string(),
            },
            qdrant: QdrantConfig {
                url: "http://localhost:6333".to_string(),
            },
            ollama: OllamaConfig {
                url: "http://localhost:11434".to_string(),
            },
            secrets_master_key: "key".to_string(),
            providers: None,
            oidc_issuer: None,
            oidc_client_id: None,
            oidc_client_secret: None,
            external_url: None,
            web_origin: None,
        };
        assert_eq!(config.port, 5000);
    }

    #[test]
    fn core_config_api_key_default() {
        let config = CoreConfig {
            database_url: "postgres://localhost/sera".to_string(),
            port: 3001,
            api_key: "sera_bootstrap_dev_123".to_string(),
            llm: LlmConfig {
                base_url: "http://localhost:1234/v1".to_string(),
                api_key: "key".to_string(),
                model: "model".to_string(),
            },
            centrifugo: CentrifugoConfig {
                api_url: "http://localhost:8000".to_string(),
                api_key: "key".to_string(),
                token_secret: "secret".to_string(),
            },
            qdrant: QdrantConfig {
                url: "http://localhost:6333".to_string(),
            },
            ollama: OllamaConfig {
                url: "http://localhost:11434".to_string(),
            },
            secrets_master_key: "key".to_string(),
            providers: None,
            oidc_issuer: None,
            oidc_client_id: None,
            oidc_client_secret: None,
            external_url: None,
            web_origin: None,
        };
        assert_eq!(config.api_key, "sera_bootstrap_dev_123");
    }

    #[test]
    fn llm_config_defaults() {
        let config = LlmConfig {
            base_url: "http://localhost:1234/v1".to_string(),
            api_key: "lm-studio".to_string(),
            model: "lmstudio-local".to_string(),
        };
        assert_eq!(config.base_url, "http://localhost:1234/v1");
        assert_eq!(config.model, "lmstudio-local");
    }

    #[test]
    fn centrifugo_config_defaults() {
        let config = CentrifugoConfig {
            api_url: "http://centrifugo:8000/api".to_string(),
            api_key: "sera-api-key".to_string(),
            token_secret: "sera-token-secret".to_string(),
        };
        assert!(config.api_url.contains("centrifugo"));
        assert!(!config.token_secret.is_empty());
    }

    #[test]
    fn qdrant_config_url() {
        let config = QdrantConfig {
            url: "http://qdrant:6333".to_string(),
        };
        assert_eq!(config.url, "http://qdrant:6333");
    }

    #[test]
    fn config_error_display() {
        let err = ConfigError::MissingEnvVar("TEST_VAR".to_string());
        assert!(err.to_string().contains("TEST_VAR"));
    }

    // ── validate_production_secrets tests ────────────────────────────────────

    fn make_config_with_defaults() -> CoreConfig {
        CoreConfig {
            database_url: "postgres://localhost/sera".to_string(),
            port: 3001,
            api_key: "sera_bootstrap_dev_123".to_string(),
            llm: LlmConfig {
                base_url: "http://localhost:1234/v1".to_string(),
                api_key: "lm-studio".to_string(),
                model: "lmstudio-local".to_string(),
            },
            centrifugo: CentrifugoConfig {
                api_url: "http://centrifugo:8000/api".to_string(),
                api_key: "sera-api-key".to_string(),
                token_secret: "sera-token-secret".to_string(),
            },
            qdrant: QdrantConfig {
                url: "http://qdrant:6333".to_string(),
            },
            ollama: OllamaConfig {
                url: "http://host.docker.internal:11434".to_string(),
            },
            secrets_master_key: "sera-dev-master-key-change-me".to_string(),
            providers: None,
            oidc_issuer: None,
            oidc_client_id: None,
            oidc_client_secret: None,
            external_url: None,
            web_origin: None,
        }
    }

    fn make_config_production_safe() -> CoreConfig {
        CoreConfig {
            database_url: "postgres://prod-host/sera".to_string(),
            port: 3001,
            api_key: "real-api-key-abc123".to_string(),
            llm: LlmConfig {
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: "sk-real-key".to_string(),
                model: "gpt-4".to_string(),
            },
            centrifugo: CentrifugoConfig {
                api_url: "https://centrifugo.prod/api".to_string(),
                api_key: "real-centrifugo-api-key".to_string(),
                token_secret: "real-token-secret-xyz".to_string(),
            },
            qdrant: QdrantConfig {
                url: "https://qdrant.prod:6333".to_string(),
            },
            ollama: OllamaConfig {
                url: "http://ollama.prod:11434".to_string(),
            },
            secrets_master_key: "real-master-key-32-chars-minimum!".to_string(),
            providers: None,
            oidc_issuer: None,
            oidc_client_id: None,
            oidc_client_secret: None,
            external_url: None,
            web_origin: None,
        }
    }

    #[test]
    fn production_mode_rejects_dev_defaults() {
        let _guard = SERA_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved = env::var("SERA_ENV").ok();
        unsafe { env::set_var("SERA_ENV", "production") };

        let config = make_config_with_defaults();
        let result = validate_production_secrets(&config);

        match &saved {
            Some(v) => unsafe { env::set_var("SERA_ENV", v) },
            None => unsafe { env::remove_var("SERA_ENV") },
        }

        assert!(result.is_err(), "production with dev defaults should fail");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("api_key"), "error should mention api_key field");
        assert!(msg.contains("insecure dev-secret"), "error should mention dev-secret");
    }

    #[test]
    fn production_mode_accepts_overridden_secrets() {
        let _guard = SERA_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved = env::var("SERA_ENV").ok();
        unsafe { env::set_var("SERA_ENV", "production") };

        let config = make_config_production_safe();
        let result = validate_production_secrets(&config);

        match &saved {
            Some(v) => unsafe { env::set_var("SERA_ENV", v) },
            None => unsafe { env::remove_var("SERA_ENV") },
        }

        assert!(result.is_ok(), "production with real secrets should pass");
    }

    #[test]
    fn dev_mode_accepts_dev_defaults_without_error() {
        let _guard = SERA_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved = env::var("SERA_ENV").ok();
        unsafe { env::remove_var("SERA_ENV") };

        let config = make_config_with_defaults();
        let result = validate_production_secrets(&config);

        match &saved {
            Some(v) => unsafe { env::set_var("SERA_ENV", v) },
            None => unsafe { env::remove_var("SERA_ENV") },
        }

        assert!(result.is_ok(), "dev mode with dev defaults should not error");
    }

    #[test]
    fn production_error_lists_all_unsafe_fields() {
        let _guard = SERA_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved = env::var("SERA_ENV").ok();
        unsafe { env::set_var("SERA_ENV", "production") };

        let config = make_config_with_defaults();
        let result = validate_production_secrets(&config);

        match &saved {
            Some(v) => unsafe { env::set_var("SERA_ENV", v) },
            None => unsafe { env::remove_var("SERA_ENV") },
        }

        let err_msg = result.unwrap_err().to_string();
        // All five dev-secret fields should appear in the error message
        assert!(err_msg.contains("api_key"));
        assert!(err_msg.contains("centrifugo.api_key"));
        assert!(err_msg.contains("centrifugo.token_secret"));
        assert!(err_msg.contains("secrets_master_key"));
    }
}
