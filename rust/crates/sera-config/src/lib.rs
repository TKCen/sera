//! SERA Config — environment variable loading for BYOH containers.

use std::env;

/// Configuration loaded from BYOH contract environment variables.
#[derive(Debug, Clone)]
pub struct SeraConfig {
    pub core_url: String,
    pub identity_token: String,
    pub llm_proxy_url: String,
    pub agent_name: String,
    pub instance_id: String,
    pub chat_port: u16,
    pub heartbeat_interval_ms: u64,
    pub lifecycle_mode: String,
}

impl SeraConfig {
    /// Load configuration from environment variables per the BYOH contract.
    pub fn from_env() -> Result<Self, String> {
        let core_url = env::var("SERA_CORE_URL")
            .unwrap_or_else(|_| "http://sera-core:3001".to_string());
        let identity_token = env::var("SERA_IDENTITY_TOKEN")
            .map_err(|_| "SERA_IDENTITY_TOKEN not set")?;
        let llm_proxy_url = env::var("SERA_LLM_PROXY_URL")
            .unwrap_or_else(|_| format!("{}/v1/llm", core_url));
        let agent_name = env::var("AGENT_NAME")
            .unwrap_or_else(|_| "unknown-agent".to_string());
        let instance_id = env::var("AGENT_INSTANCE_ID")
            .unwrap_or_else(|_| "unknown-instance".to_string());
        let chat_port = env::var("AGENT_CHAT_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3100);
        let heartbeat_interval_ms = env::var("AGENT_HEARTBEAT_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30000);
        let lifecycle_mode = env::var("AGENT_LIFECYCLE_MODE")
            .unwrap_or_else(|_| "ephemeral".to_string());

        Ok(Self {
            core_url,
            identity_token,
            llm_proxy_url,
            agent_name,
            instance_id,
            chat_port,
            heartbeat_interval_ms,
            lifecycle_mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_requires_identity_token() {
        // Clear any existing env
        // SAFETY: single-threaded test, no other threads reading this var
        unsafe { env::remove_var("SERA_IDENTITY_TOKEN") };
        let result = SeraConfig::from_env();
        assert!(result.is_err());
    }
}
