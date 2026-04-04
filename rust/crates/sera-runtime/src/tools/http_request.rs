//! HTTP request tool.

use super::ToolExecutor;

pub struct HttpRequest;

#[async_trait::async_trait]
impl ToolExecutor for HttpRequest {
    fn name(&self) -> &str { "http-request" }
    fn description(&self) -> &str { "Make an HTTP request to a URL" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to request" },
                "method": { "type": "string", "description": "HTTP method (GET, POST, etc.)", "default": "GET" },
                "body": { "type": "string", "description": "Request body (for POST/PUT)" },
                "headers": { "type": "object", "description": "Request headers" }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let url = args["url"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'url'"))?;
        let method = args["method"].as_str().unwrap_or("GET").to_uppercase();

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let mut req = match method.as_str() {
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            "PATCH" => client.patch(url),
            _ => client.get(url),
        };

        if let Some(headers) = args["headers"].as_object() {
            for (k, v) in headers {
                if let Some(val) = v.as_str() {
                    req = req.header(k.as_str(), val);
                }
            }
        }

        if let Some(body) = args["body"].as_str() {
            req = req.body(body.to_string());
        }

        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();

        Ok(format!("HTTP {status}\n{body}"))
    }
}
