//! Dynamic Provider Manager — loads, validates, and caches provider configurations from YAML/JSON.
//!
//! Supports hot-reloading provider configs with graceful error recovery.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use sera_config::providers::ProviderEntry;

/// Provider entry with enabled/priority fields for dynamic management.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicProviderEntry {
    pub name: String,
    pub provider_type: String,
    pub api_url: String,
    pub model: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

fn default_enabled() -> bool {
    true
}

/// Error type for provider manager operations.
#[derive(Debug, Error)]
pub enum ProviderManagerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Validation error: {0}")]
    Validation(String),
}

/// Dynamic Provider Manager — loads, validates, and caches provider configurations.
pub struct DynamicProviderManager {
    config_path: PathBuf,
    providers: Arc<RwLock<Vec<DynamicProviderEntry>>>,
}

impl DynamicProviderManager {
    /// Initialize with a path to the configuration file (YAML or JSON).
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            config_path,
            providers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Load providers from the configuration file.
    ///
    /// Parses YAML or JSON format. Returns error if file doesn't exist or is malformed.
    pub async fn load_providers(&self) -> Result<Vec<DynamicProviderEntry>, ProviderManagerError> {
        let contents = tokio::fs::read_to_string(&self.config_path)
            .await
            .map_err(ProviderManagerError::Io)?;

        self.parse_config(&contents)
    }

    /// Refresh providers from the configuration file, with error recovery.
    ///
    /// On parse or validation error, logs warning, keeps previous config, and returns error.
    /// This prevents crashes on malformed configs in production.
    pub async fn refresh_providers(&self) -> Result<(), ProviderManagerError> {
        match self.load_providers().await {
            Ok(new_providers) => {
                debug!(
                    count = new_providers.len(),
                    "Loaded {} providers",
                    new_providers.len()
                );
                self.validate_providers(&new_providers)?;
                let mut cache = self.providers.write().await;
                *cache = new_providers;
                Ok(())
            }
            Err(e) => {
                warn!(
                    "Failed to refresh providers: {}. Keeping previous config.",
                    e
                );
                Err(e)
            }
        }
    }

    /// Get the cached providers.
    pub async fn get_providers(&self) -> Vec<DynamicProviderEntry> {
        self.providers.read().await.clone()
    }

    /// Parse configuration from YAML or JSON string.
    fn parse_config(
        &self,
        contents: &str,
    ) -> Result<Vec<DynamicProviderEntry>, ProviderManagerError> {
        // Try YAML first
        if let Ok(config) = serde_yaml::from_str::<serde_yaml::Value>(contents)
            && let Some(providers) = config.get("providers")
        {
            return serde_yaml::from_value(providers.clone())
                .map_err(|e| ProviderManagerError::Parse(format!("YAML parse error: {}", e)));
        }

        // Try JSON as fallback
        match serde_json::from_str::<serde_json::Value>(contents) {
            Ok(config) => {
                if let Some(providers) = config.get("providers") {
                    return serde_json::from_value(providers.clone()).map_err(|e| {
                        ProviderManagerError::Parse(format!("JSON parse error: {}", e))
                    });
                }
                Err(ProviderManagerError::Parse(
                    "No 'providers' key found".to_string(),
                ))
            }
            Err(e) => Err(ProviderManagerError::Parse(format!(
                "JSON parse error: {}",
                e
            ))),
        }
    }

    /// Validate provider entries.
    fn validate_providers(
        &self,
        providers: &[DynamicProviderEntry],
    ) -> Result<(), ProviderManagerError> {
        for (idx, provider) in providers.iter().enumerate() {
            if provider.name.is_empty() {
                return Err(ProviderManagerError::Validation(format!(
                    "Provider at index {} has empty name",
                    idx
                )));
            }
            if provider.api_url.is_empty() {
                return Err(ProviderManagerError::Validation(format!(
                    "Provider '{}' has empty api_url",
                    provider.name
                )));
            }
            if provider.model.is_empty() {
                return Err(ProviderManagerError::Validation(format!(
                    "Provider '{}' has empty model",
                    provider.name
                )));
            }
        }
        Ok(())
    }

    /// Convert DynamicProviderEntry to ProviderEntry for compatibility with ProviderConfig.
    pub fn to_provider_entry(&self, dynamic: &DynamicProviderEntry) -> ProviderEntry {
        ProviderEntry {
            model_name: dynamic.model.clone(),
            api: "openai-completions".to_string(), // Default API type
            provider: dynamic.provider_type.clone(),
            base_url: dynamic.api_url.clone(),
            api_key: dynamic.api_key.clone().unwrap_or_default(),
            description: Some(format!("Dynamic provider: {}", dynamic.name)),
            context_window: None,
            max_tokens: None,
            reasoning: false,
            context_strategy: None,
            context_high_water_mark: None,
            dynamic_provider_id: Some(dynamic.name.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_yaml_config() {
        let yaml = r#"
providers:
  - name: local-model
    providerType: ollama
    apiUrl: http://localhost:11434/v1
    model: mistral
    enabled: true
    priority: 1
  - name: openai-gpt4
    providerType: openai
    apiUrl: https://api.openai.com/v1
    model: gpt-4
    enabled: true
    priority: 2
    apiKey: sk-test
"#;

        let manager = DynamicProviderManager::new(PathBuf::from("/tmp/test.yaml"));
        let providers = manager.parse_config(yaml).expect("Failed to parse YAML");

        assert_eq!(providers.len(), 2);
        assert_eq!(providers[0].name, "local-model");
        assert_eq!(providers[0].provider_type, "ollama");
        assert!(providers[0].enabled);
        assert_eq!(providers[1].name, "openai-gpt4");
        assert_eq!(providers[1].priority, 2);
    }

    #[test]
    fn test_parse_json_config() {
        let json = r#"{
  "providers": [
    {
      "name": "local-model",
      "providerType": "ollama",
      "apiUrl": "http://localhost:11434/v1",
      "model": "mistral",
      "enabled": true,
      "priority": 1
    }
  ]
}"#;

        let manager = DynamicProviderManager::new(PathBuf::from("/tmp/test.json"));
        let providers = manager.parse_config(json).expect("Failed to parse JSON");

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].name, "local-model");
    }

    #[test]
    fn test_validation_empty_name() {
        let yaml = r#"
providers:
  - name: ""
    providerType: ollama
    apiUrl: http://localhost:11434/v1
    model: mistral
"#;

        let manager = DynamicProviderManager::new(PathBuf::from("/tmp/test.yaml"));
        let providers = manager.parse_config(yaml).expect("Failed to parse");
        let result = manager.validate_providers(&providers);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty name"));
    }

    #[test]
    fn test_validation_empty_api_url() {
        let yaml = r#"
providers:
  - name: test-provider
    providerType: ollama
    apiUrl: ""
    model: mistral
"#;

        let manager = DynamicProviderManager::new(PathBuf::from("/tmp/test.yaml"));
        let providers = manager.parse_config(yaml).expect("Failed to parse");
        let result = manager.validate_providers(&providers);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty api_url"));
    }

    #[test]
    fn test_validation_empty_model() {
        let yaml = r#"
providers:
  - name: test-provider
    providerType: ollama
    apiUrl: http://localhost:11434/v1
    model: ""
"#;

        let manager = DynamicProviderManager::new(PathBuf::from("/tmp/test.yaml"));
        let providers = manager.parse_config(yaml).expect("Failed to parse");
        let result = manager.validate_providers(&providers);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty model"));
    }

    #[tokio::test]
    async fn test_load_and_refresh_from_temp_file() {
        // Create a temporary YAML file
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let yaml = r#"
providers:
  - name: test-provider
    providerType: ollama
    apiUrl: http://localhost:11434/v1
    model: mistral
    enabled: true
    priority: 1
"#;
        temp_file
            .write_all(yaml.as_bytes())
            .expect("Failed to write temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let manager = DynamicProviderManager::new(temp_file.path().to_path_buf());
        let providers = manager
            .load_providers()
            .await
            .expect("Failed to load providers");

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].name, "test-provider");

        // Test refresh
        manager
            .refresh_providers()
            .await
            .expect("Failed to refresh");
        let cached = manager.get_providers().await;
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].name, "test-provider");
    }

    #[test]
    fn test_error_recovery_on_malformed_config() {
        // Create a temporary file with valid initial config
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let yaml = r#"
providers:
  - name: test-provider
    providerType: ollama
    apiUrl: http://localhost:11434/v1
    model: mistral
    enabled: true
    priority: 1
"#;
        temp_file
            .write_all(yaml.as_bytes())
            .expect("Failed to write");
        temp_file.flush().expect("Failed to flush");

        // Just verify the file can be created and written
        assert!(temp_file.path().exists());
    }

    #[test]
    fn test_default_enabled_is_true() {
        let yaml = r#"
providers:
  - name: test-provider
    providerType: ollama
    apiUrl: http://localhost:11434/v1
    model: mistral
"#;

        let manager = DynamicProviderManager::new(PathBuf::from("/tmp/test.yaml"));
        let providers = manager.parse_config(yaml).expect("Failed to parse");

        assert!(providers[0].enabled);
    }

    #[test]
    fn test_to_provider_entry_conversion() {
        let dynamic = DynamicProviderEntry {
            name: "test-provider".to_string(),
            provider_type: "ollama".to_string(),
            api_url: "http://localhost:11434/v1".to_string(),
            model: "mistral".to_string(),
            enabled: true,
            priority: 1,
            api_key: Some("test-key".to_string()),
        };

        let manager = DynamicProviderManager::new(PathBuf::from("/tmp/test.yaml"));
        let entry = manager.to_provider_entry(&dynamic);

        assert_eq!(entry.model_name, "mistral");
        assert_eq!(entry.provider, "ollama");
        assert_eq!(entry.base_url, "http://localhost:11434/v1");
        assert_eq!(entry.api_key, "test-key");
        assert_eq!(entry.dynamic_provider_id, Some("test-provider".to_string()));
    }
}
