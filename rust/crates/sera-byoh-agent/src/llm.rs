use serde_json::json;
use tracing::debug;

/// Send a chat completion request to the SERA LLM proxy.
pub async fn chat(
    config: &sera_config::SeraConfig,
    prompt: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", config.llm_proxy_url);

    debug!("LLM request to {url}");

    let body = json!({
        "model": "default",
        "messages": [
            { "role": "user", "content": prompt }
        ]
    });

    let res = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.identity_token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(format!("LLM proxy returned {status}: {body}").into());
    }

    let data: serde_json::Value = res.json().await?;
    let content = data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    Ok(content)
}
