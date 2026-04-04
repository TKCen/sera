//! Runtime configuration from environment variables.

/// Runtime configuration — read from env vars set by sera-core when spawning the container.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub llm_base_url: String,
    pub llm_model: String,
    pub llm_api_key: String,
    pub chat_port: u16,
    pub agent_id: String,
    pub lifecycle_mode: String,
    pub core_url: String,
    pub api_key: String,
    pub context_window: usize,
    pub compaction_strategy: String,
    pub max_tokens: u32,
}

impl RuntimeConfig {
    pub fn from_env() -> Self {
        Self {
            llm_base_url: std::env::var("LLM_BASE_URL")
                .unwrap_or_else(|_| "http://sera-core:3001/v1/llm".to_string()),
            llm_model: std::env::var("LLM_MODEL")
                .unwrap_or_else(|_| "lmstudio-local".to_string()),
            llm_api_key: std::env::var("LLM_API_KEY")
                .unwrap_or_else(|_| "lm-studio".to_string()),
            chat_port: std::env::var("AGENT_CHAT_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8080),
            agent_id: std::env::var("AGENT_ID").unwrap_or_default(),
            lifecycle_mode: std::env::var("LIFECYCLE_MODE")
                .unwrap_or_else(|_| "task".to_string()),
            core_url: std::env::var("SERA_CORE_URL")
                .unwrap_or_else(|_| "http://sera-core:3001".to_string()),
            api_key: std::env::var("SERA_API_KEY")
                .unwrap_or_else(|_| "sera_bootstrap_dev_123".to_string()),
            context_window: std::env::var("CONTEXT_WINDOW")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(128_000),
            compaction_strategy: std::env::var("CONTEXT_COMPACTION_STRATEGY")
                .unwrap_or_else(|_| "summarize".to_string()),
            max_tokens: std::env::var("MAX_TOKENS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(4096),
        }
    }
}
