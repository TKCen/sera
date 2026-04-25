//! HTTP request tool.
//!
//! Native `Tool` trait implementation (bead sera-ttrm-5) with
//! [`RiskLevel::Execute`] — HTTP requests can mutate remote state via
//! POST/PUT/DELETE/PATCH, so treat the generic tool as Execute-class.
//!
//! SSRF protection (sera-udjf): IP-literal hosts are validated against
//! [`SsrfValidator`] before the request is sent.  Blocks LLM-instructed
//! requests to loopback, RFC-1918 private ranges, link-local addresses,
//! IPv6 ULA, and cloud metadata endpoints.  Hostname hosts (`example.com`)
//! bypass validation — full DNS-resolved SSRF protection is tracked as a
//! follow-up.

use std::collections::HashMap;

use async_trait::async_trait;
use sera_tools::ssrf::SsrfValidator;
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

        // SSRF guard (sera-udjf) — parse the URL, resolve the host via
        // `tokio::net::lookup_host` (returns IP literals verbatim, system
        // resolver for hostnames), validate every resolved IP, and pin the
        // DNS answer in reqwest to mitigate DNS rebinding.
        let parsed = reqwest::Url::parse(url)
            .map_err(|e| ToolError::InvalidInput(format!("invalid URL: {e}")))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| ToolError::InvalidInput(format!("URL has no host: {url}")))?
            .to_owned();
        let port = parsed.port_or_known_default().unwrap_or(80);
        let pinned_addrs = SsrfValidator::resolve_and_validate(&host, port)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("ssrf: refusing to fetch {host}: {e}"))
            })?;

        // Same-host redirects are safe (DNS pinned via resolve_to_addrs).
        // Cross-host redirects are stricter: IP literals get validated; a
        // different hostname is blocked outright because we cannot do an
        // async resolve inside the sync redirect policy closure (and a
        // fresh DNS lookup would skip the pin).
        let redirect_host = host.clone();
        let client = reqwest::Client::builder()
            .resolve_to_addrs(&host, &pinned_addrs)
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                let target = attempt.url().host_str().map(str::to_owned);
                match target.as_deref() {
                    Some(t) if t == redirect_host => attempt.follow(),
                    Some(t) => {
                        if t.parse::<std::net::IpAddr>().is_ok() {
                            match SsrfValidator::validate(t) {
                                Ok(()) => attempt.follow(),
                                Err(e) => attempt.error(std::io::Error::other(format!(
                                    "ssrf: redirect blocked to {t}: {e}"
                                ))),
                            }
                        } else {
                            attempt.error(std::io::Error::other(format!(
                                "ssrf: cross-host hostname redirect blocked: {t}"
                            )))
                        }
                    }
                    None => attempt.follow(),
                }
            }))
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
    use serde_json::json;

    #[test]
    fn metadata_risk_level_is_execute() {
        assert_eq!(HttpRequest.metadata().risk_level, RiskLevel::Execute);
        assert_eq!(HttpRequest.metadata().name, "http-request");
    }

    /// sera-udjf — IP-literal hosts in the SSRF blocklist must be rejected
    /// before the request is sent.  Covers RFC-1918 (10.x, 192.168.x),
    /// loopback (127.x), link-local (169.254.x), and cloud metadata
    /// (169.254.169.254).  Hostname hosts are allowed-through for now.
    #[tokio::test]
    async fn execute_rejects_ssrf_private_range() {
        let tool = HttpRequest;
        let cases = [
            "http://10.0.0.1/",
            "http://192.168.1.1/",
            "http://127.0.0.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
        ];
        for url in cases {
            let input = ToolInput {
                name: "http-request".to_string(),
                call_id: "test".to_string(),
                arguments: json!({ "url": url }),
            };
            let result = tool.execute(input, ToolContext::default()).await;
            assert!(
                matches!(result, Err(ToolError::ExecutionFailed(ref msg)) if msg.starts_with("ssrf:")),
                "{url} should be SSRF-rejected, got {result:?}"
            );
        }
    }

    /// sera-udjf hostname-bypass closure — `http://localhost/` resolves
    /// (on every Linux distro the test runs on) to 127.0.0.1 / ::1, both
    /// loopback.  Pre-fix the validator only saw the hostname `localhost`
    /// and returned `NotAllowed`, which the integration swallowed and let
    /// through.  Post-fix the DNS resolution runs first, the loopback IP
    /// is rejected, and the request never leaves the process.
    ///
    /// The test does not require any specific resolver configuration —
    /// `localhost` is reserved by RFC 6761 and every system resolver maps
    /// it to loopback.
    #[tokio::test]
    async fn execute_rejects_hostname_resolving_to_loopback() {
        let tool = HttpRequest;
        let input = ToolInput {
            name: "http-request".to_string(),
            call_id: "test".to_string(),
            arguments: json!({ "url": "http://localhost/" }),
        };
        let result = tool.execute(input, ToolContext::default()).await;
        assert!(
            matches!(result, Err(ToolError::ExecutionFailed(ref msg)) if msg.contains("ssrf:")),
            "localhost must be rejected via DNS-resolved SSRF guard, got {result:?}"
        );
    }

    /// Malformed URLs surface as `InvalidInput` rather than a network error
    /// so the caller can distinguish "you typo-ed" from "the host blew up".
    #[tokio::test]
    async fn execute_rejects_malformed_url() {
        let tool = HttpRequest;
        let input = ToolInput {
            name: "http-request".to_string(),
            call_id: "test".to_string(),
            arguments: json!({ "url": "not a url" }),
        };
        let result = tool.execute(input, ToolContext::default()).await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))), "got {result:?}");
    }
}
