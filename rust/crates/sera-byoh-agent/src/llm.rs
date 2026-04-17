use serde_json::json;
use tracing::debug;

/// Build the LLM proxy URL from the base URL.
pub fn build_llm_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url)
}

/// Build a chat completion request body.
pub fn build_chat_request(prompt: &str) -> serde_json::Value {
    json!({
        "model": "default",
        "messages": [
            { "role": "user", "content": prompt }
        ]
    })
}

/// Extract content from LLM response JSON.
pub fn extract_content(response_json: &serde_json::Value) -> Option<String> {
    response_json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
}

/// Send a chat completion request to the SERA LLM proxy.
pub async fn chat(
    config: &sera_config::SeraConfig,
    prompt: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let url = build_llm_url(&config.llm_proxy_url);

    debug!("LLM request to {url}");

    let body = build_chat_request(prompt);

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
    let content = extract_content(&data)
        .unwrap_or_default();

    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_llm_url_standard() {
        let url = build_llm_url("https://llm.sera.internal");
        assert_eq!(url, "https://llm.sera.internal/chat/completions");
    }

    #[test]
    fn build_llm_url_with_port() {
        let url = build_llm_url("http://localhost:8000");
        assert_eq!(url, "http://localhost:8000/chat/completions");
    }

    #[test]
    fn build_llm_url_with_trailing_slash() {
        let url = build_llm_url("https://example.com/");
        assert_eq!(url, "https://example.com//chat/completions");
    }

    #[test]
    fn build_chat_request_simple_prompt() {
        let request = build_chat_request("What is 2+2?");
        assert_eq!(request["model"], "default");
        assert_eq!(request["messages"][0]["role"], "user");
        assert_eq!(request["messages"][0]["content"], "What is 2+2?");
    }

    #[test]
    fn build_chat_request_multiline_prompt() {
        let prompt = "Line 1\nLine 2\nLine 3";
        let request = build_chat_request(prompt);
        assert_eq!(request["messages"][0]["content"], prompt);
    }

    #[test]
    fn build_chat_request_special_chars() {
        let prompt = "Test with \"quotes\" and 'apostrophes'";
        let request = build_chat_request(prompt);
        assert_eq!(request["messages"][0]["content"], prompt);
    }

    #[test]
    fn extract_content_valid_response() {
        let response = json!({
            "choices": [
                {
                    "message": {
                        "content": "The answer is 4"
                    }
                }
            ]
        });
        let content = extract_content(&response);
        assert_eq!(content, Some("The answer is 4".to_string()));
    }

    #[test]
    fn extract_content_multiline_response() {
        let response = json!({
            "choices": [
                {
                    "message": {
                        "content": "Line 1\nLine 2\nLine 3"
                    }
                }
            ]
        });
        let content = extract_content(&response);
        assert!(content.unwrap().contains("Line 1"));
    }

    #[test]
    fn extract_content_missing_field() {
        let response = json!({
            "choices": [
                {
                    "message": {}
                }
            ]
        });
        let content = extract_content(&response);
        assert_eq!(content, None);
    }

    #[test]
    fn extract_content_empty_choices() {
        let response = json!({
            "choices": []
        });
        let content = extract_content(&response);
        assert_eq!(content, None);
    }

    #[test]
    fn extract_content_non_string_content() {
        let response = json!({
            "choices": [
                {
                    "message": {
                        "content": 123
                    }
                }
            ]
        });
        let content = extract_content(&response);
        assert_eq!(content, None);
    }

    #[test]
    fn extract_content_empty_string() {
        let response = json!({
            "choices": [
                {
                    "message": {
                        "content": ""
                    }
                }
            ]
        });
        let content = extract_content(&response);
        assert_eq!(content, Some("".to_string()));
    }
}
