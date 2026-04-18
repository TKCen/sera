//! HTTP request tool.
//!
//! Native `Tool` trait implementation (bead sera-ttrm-5) with
//! [`RiskLevel::Execute`] — HTTP requests can mutate remote state via
//! POST/PUT/DELETE/PATCH, so treat the generic tool as Execute-class.

use std::collections::HashMap;

use async_trait::async_trait;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema,
};

pub struct HttpRequest;

#[async_trait]
impl Tool for HttpRequest {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "http-request".to_string(),
            description: "Make an HTTP request to a URL".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            // HTTP supports POST/PUT/DELETE/PATCH — side-effects possible.
            risk_level: RiskLevel::Execute,
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
                description: Some("URL to request".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "method".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("HTTP method (GET, POST, etc.)".to_string()),
                enum_values: None,
                default: Some(serde_json::json!("GET")),
            },
        );
        properties.insert(
            "body".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Request body (for POST/PUT)".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "headers".to_string(),
            ParameterSchema {
                schema_type: "object".to_string(),
                description: Some("Request headers".to_string()),
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
        let method = args["method"].as_str().unwrap_or("GET").to_uppercase();

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ToolError::ExecutionFailed(format!("client build: {e}")))?;

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

        let resp = req
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("send failed: {e}")))?;
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();

        Ok(ToolOutput::success(format!("HTTP {status}\n{body}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_risk_level_is_execute() {
        assert_eq!(HttpRequest.metadata().risk_level, RiskLevel::Execute);
        assert_eq!(HttpRequest.metadata().name, "http-request");
    }
}
