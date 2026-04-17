//! Web fetch tool for retrieving web page content.
//!
//! Native `Tool` trait implementation (bead sera-ttrm-5) with
//! [`RiskLevel::Read`] — this tool is fixed to HTTP GET only, so from the
//! agent's perspective it is a read-only observation of a remote resource.

use std::collections::HashMap;

use async_trait::async_trait;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema,
};

pub struct WebFetch;

#[async_trait]
impl Tool for WebFetch {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "web-fetch".to_string(),
            description:
                "Fetch the content of a web page and return text (truncated to max_length)"
                    .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::External,
            tags: vec!["network".to_string()],
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "url".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("URL to fetch".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "max_length".to_string(),
            ParameterSchema {
                schema_type: "integer".to_string(),
                description: Some("Maximum content length in bytes (default 50000)".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["url".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let args = &input.arguments;
        let url = args["url"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'url'".to_string()))?;
        let max_length = args["max_length"].as_u64().unwrap_or(50_000) as usize;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("SERA-Agent/1.0")
            .build()
            .map_err(|e| ToolError::ExecutionFailed(format!("client build: {e}")))?;

        let resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("send failed: {e}")))?;

        if !resp.status().is_success() {
            return Ok(ToolOutput::success(format!(
                "HTTP {}: Failed to fetch {}",
                resp.status(),
                url
            )));
        }

        let mut content = resp.text().await.unwrap_or_default();
        if content.len() > max_length {
            content.truncate(max_length);
            content.push_str("\n[... truncated ...]");
        }

        Ok(ToolOutput::success(content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_risk_level_is_read() {
        assert_eq!(WebFetch.metadata().risk_level, RiskLevel::Read);
        assert_eq!(WebFetch.metadata().name, "web-fetch");
    }
}
