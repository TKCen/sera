//! LLM client — calls the sera-core LLM proxy via reqwest.

use crate::config::RuntimeConfig;
use crate::types::{ChatMessage, LlmResponse, ToolDefinition};

/// HTTP client for the LLM proxy endpoint.
pub struct LlmClient {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
    max_tokens: u32,
}

impl LlmClient {
    pub fn new(config: &RuntimeConfig) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_default(),
            base_url: config.llm_base_url.clone(),
            model: config.llm_model.clone(),
            api_key: config.llm_api_key.clone(),
            max_tokens: config.max_tokens,
        }
    }

    /// Send a chat completion request to the LLM proxy.
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<LlmChatResult> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": self.max_tokens,
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools)?;
        }

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("LLM proxy returned HTTP {status}: {text}");
        }

        let llm_resp: LlmResponse = resp.json().await?;

        let choice = llm_resp
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Empty choices in LLM response"))?;

        let usage = llm_resp.usage.unwrap_or_default();

        Ok(LlmChatResult {
            message: choice.message,
            finish_reason: choice.finish_reason.unwrap_or_else(|| "stop".to_string()),
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
        })
    }
}

/// Result of a chat completion request.
pub struct LlmChatResult {
    pub message: ChatMessage,
    pub finish_reason: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

impl Default for crate::types::LlmUsage {
    fn default() -> Self {
        Self {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        }
    }
}
