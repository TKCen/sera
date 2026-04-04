//! Provider configuration — parses providers.json.
//! Maps from TS: ProviderRegistry + providers.json format.

use serde::{Deserialize, Serialize};

/// Top-level providers.json structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    pub providers: Vec<ProviderEntry>,
}

/// A single provider entry in providers.json.
/// Maps from the TypeScript ProviderRegistry config format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntry {
    pub model_name: String,
    #[serde(default = "default_api")]
    pub api: String,
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_high_water_mark: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamic_provider_id: Option<String>,
}

fn default_api() -> String {
    "openai-completions".to_string()
}

impl ProvidersConfig {
    /// Parse from JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Find a provider entry by model name.
    pub fn find_by_model(&self, model_name: &str) -> Option<&ProviderEntry> {
        self.providers.iter().find(|p| p.model_name == model_name)
    }

    /// List all non-dynamic provider model names.
    pub fn static_models(&self) -> Vec<&str> {
        self.providers
            .iter()
            .filter(|p| p.dynamic_provider_id.is_none())
            .map(|p| p.model_name.as_str())
            .collect()
    }
}

impl ProviderEntry {
    /// Effective context window, defaulting to 128K if unset.
    pub fn effective_context_window(&self) -> u64 {
        self.context_window.unwrap_or(131_072)
    }

    /// Effective max tokens, defaulting to 4K if unset.
    pub fn effective_max_tokens(&self) -> u64 {
        self.max_tokens.unwrap_or(4096)
    }
}

impl ProvidersConfig {
    /// Add a new provider entry. Returns error if modelName already exists.
    pub fn add_provider(&mut self, entry: ProviderEntry) -> Result<(), String> {
        if self.providers.iter().any(|p| p.model_name == entry.model_name) {
            return Err(format!("Provider '{}' already exists", entry.model_name));
        }
        self.providers.push(entry);
        Ok(())
    }

    /// Update fields on an existing provider. Returns error if not found.
    pub fn update_provider(
        &mut self,
        model_name: &str,
        context_window: Option<u64>,
        max_tokens: Option<u64>,
        reasoning: Option<bool>,
        description: Option<String>,
        context_strategy: Option<String>,
    ) -> Result<(), String> {
        let entry = self
            .providers
            .iter_mut()
            .find(|p| p.model_name == model_name)
            .ok_or_else(|| format!("Provider '{}' not found", model_name))?;

        if let Some(v) = context_window {
            entry.context_window = Some(v);
        }
        if let Some(v) = max_tokens {
            entry.max_tokens = Some(v);
        }
        if let Some(v) = reasoning {
            entry.reasoning = v;
        }
        if let Some(v) = description {
            entry.description = Some(v);
        }
        if let Some(v) = context_strategy {
            entry.context_strategy = Some(v);
        }
        Ok(())
    }

    /// Remove a provider by model name. Returns error if not found.
    pub fn remove_provider(&mut self, model_name: &str) -> Result<(), String> {
        let len_before = self.providers.len();
        self.providers.retain(|p| p.model_name != model_name);
        if self.providers.len() == len_before {
            return Err(format!("Provider '{}' not found", model_name));
        }
        Ok(())
    }

    /// Save to a JSON file.
    pub fn save_to_file(&self, path: &str) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("Failed to write {path}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_JSON: &str = r#"{
        "providers": [
            {
                "modelName": "lmstudio-local",
                "api": "openai-completions",
                "provider": "lmstudio",
                "baseUrl": "http://host.docker.internal:1234/v1",
                "apiKey": "lm-studio",
                "description": "Local LM Studio instance",
                "contextWindow": 32768,
                "maxTokens": 4096
            },
            {
                "modelName": "qwen3.5-35b-a3b",
                "api": "openai-completions",
                "provider": "lmstudio",
                "baseUrl": "http://host.docker.internal:1234/v1",
                "apiKey": "lm-studio",
                "contextWindow": 190000,
                "maxTokens": 8192,
                "contextStrategy": "summarize",
                "reasoning": true,
                "contextHighWaterMark": 0.8
            },
            {
                "modelName": "dp-agw-discovered",
                "api": "openai-completions",
                "provider": "lmstudio",
                "baseUrl": "http://host.docker.internal:1234/v1",
                "apiKey": "lm-studio",
                "dynamicProviderId": "agw"
            }
        ]
    }"#;

    #[test]
    fn parse_providers_json() {
        let config = ProvidersConfig::from_json(SAMPLE_JSON).unwrap();
        assert_eq!(config.providers.len(), 3);

        let lm = config.find_by_model("lmstudio-local").unwrap();
        assert_eq!(lm.provider, "lmstudio");
        assert_eq!(lm.effective_context_window(), 32768);
        assert_eq!(lm.effective_max_tokens(), 4096);
        assert!(!lm.reasoning);
    }

    #[test]
    fn reasoning_model_flag() {
        let config = ProvidersConfig::from_json(SAMPLE_JSON).unwrap();
        let qwen = config.find_by_model("qwen3.5-35b-a3b").unwrap();
        assert!(qwen.reasoning);
        assert_eq!(qwen.context_strategy.as_deref(), Some("summarize"));
        assert_eq!(qwen.context_high_water_mark, Some(0.8));
    }

    #[test]
    fn static_models_excludes_dynamic() {
        let config = ProvidersConfig::from_json(SAMPLE_JSON).unwrap();
        let static_models = config.static_models();
        assert_eq!(static_models.len(), 2);
        assert!(static_models.contains(&"lmstudio-local"));
        assert!(static_models.contains(&"qwen3.5-35b-a3b"));
        assert!(!static_models.contains(&"dp-agw-discovered"));
    }

    #[test]
    fn defaults_for_missing_fields() {
        let json = r#"{"providers": [{"modelName": "bare", "provider": "test"}]}"#;
        let config = ProvidersConfig::from_json(json).unwrap();
        let entry = &config.providers[0];
        assert_eq!(entry.api, "openai-completions");
        assert_eq!(entry.effective_context_window(), 131_072);
        assert_eq!(entry.effective_max_tokens(), 4096);
        assert!(!entry.reasoning);
    }

    #[test]
    fn parse_real_providers_file() {
        let path = "../../contracts/../../../core/config/providers.json";
        if let Ok(contents) = std::fs::read_to_string(path) {
            let config = ProvidersConfig::from_json(&contents).unwrap();
            assert!(!config.providers.is_empty());
        }
        // Skip if file not found — CI may not have it
    }
}
