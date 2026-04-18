//! Concrete [`McpClientBridge`] backed by the Anthropic [`rmcp`] SDK.
//!
//! One [`RmcpClientBridge`] holds the configuration for a single external MCP
//! server plus a lazily-connected [`rmcp::service::RunningService`]. Tools
//! returned from the server are namespaced with the configured server name so
//! callers can distinguish `github.create_issue` from `filesystem.read`.
//!
//! Scope is phase **S** per `docs/plan/PLUGIN-MCP-ECOSYSTEM.md §5.5`:
//! transport wiring only. Higher-level concerns (pooling, REST management API,
//! TUI view, per-agent namespaces) land in phase M/L and live in other
//! crates — this module intentionally does not know about them.
//!
//! Behind the `rmcp-client` cargo feature.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, CallToolResult, JsonObject, RawContent, Tool},
    service::RunningService,
    transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess},
};

use crate::{
    McpClientBridge, McpError, McpServerConfig, McpToolDescriptor, McpToolResult, McpTransport,
    namespace, split_namespace,
};

// ---------------------------------------------------------------------------
// Bridge
// ---------------------------------------------------------------------------

/// An MCP client bridge backed by [`rmcp`].
///
/// One bridge instance = one configured external MCP server. The running
/// service is kept behind an [`Arc<RwLock<_>>`] so [`Self::connect`] and
/// [`Self::disconnect`] are reentrant and safe to call from multiple tasks.
pub struct RmcpClientBridge {
    config: McpServerConfig,
    running: Arc<RwLock<Option<RunningService<RoleClient, ()>>>>,
}

impl RmcpClientBridge {
    /// Build a new bridge for the given server configuration.
    ///
    /// This does **not** start a connection — call [`Self::connect`] to do
    /// that. Constructing the bridge never fails; bad configs are caught at
    /// connect time when the transport actually tries to start.
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            running: Arc::new(RwLock::new(None)),
        }
    }

    /// Returns a reference to the configuration this bridge was built with.
    pub fn config(&self) -> &McpServerConfig {
        &self.config
    }

    /// True once [`Self::connect`] has successfully produced a running service
    /// and [`Self::disconnect`] has not yet been called.
    pub async fn is_connected(&self) -> bool {
        self.running.read().await.is_some()
    }
}

// ---------------------------------------------------------------------------
// Transport construction
// ---------------------------------------------------------------------------

async fn connect_stdio(
    config: &McpServerConfig,
) -> Result<RunningService<RoleClient, ()>, McpError> {
    let cmd = config
        .command
        .as_deref()
        .ok_or_else(|| McpError::ConnectionFailed {
            reason: format!(
                "stdio transport requires `command` in server '{}'",
                config.name
            ),
        })?;

    let args = config.args.clone();
    let env = config.env.clone();

    let tokio_cmd = tokio::process::Command::new(cmd).configure(|c| {
        c.args(&args);
        for (k, v) in &env {
            c.env(k, v);
        }
    });

    let transport = TokioChildProcess::new(tokio_cmd)
        .map_err(|e| McpError::transport(format!("spawn child '{cmd}': {e}")))?;

    let service: () = ();
    service
        .serve(transport)
        .await
        .map_err(|e| McpError::transport(format!("stdio handshake: {e}")))
}

async fn connect_http_like(
    config: &McpServerConfig,
) -> Result<RunningService<RoleClient, ()>, McpError> {
    let url = config
        .url
        .as_deref()
        .ok_or_else(|| McpError::ConnectionFailed {
            reason: format!(
                "{:?} transport requires `url` in server '{}'",
                config.transport, config.name
            ),
        })?;

    // rmcp 1.5 merged the legacy SSE-only transport into the streamable-HTTP
    // transport — the server negotiates SSE vs. streaming on a per-request
    // basis. We therefore use the same constructor for both `Sse` and
    // `StreamableHttp` config variants.
    //
    // `from_uri` pulls in the default reqwest-backed client (TLS comes from
    // the workspace `rustls-tls` defaults); operators who need custom
    // TLS/proxies can swap in `StreamableHttpClientTransport::with_client`
    // at a later phase.
    let transport = StreamableHttpClientTransport::from_uri(url.to_string());

    let service: () = ();
    service
        .serve(transport)
        .await
        .map_err(|e| McpError::transport(format!("http handshake: {e}")))
}

// ---------------------------------------------------------------------------
// Tool mapping
// ---------------------------------------------------------------------------

fn tool_to_descriptor(server: &str, tool: &Tool) -> McpToolDescriptor {
    let description = tool.description.as_deref().unwrap_or("").to_string();
    // `input_schema` is `Arc<JsonObject>` — clone the inner map into a plain
    // `serde_json::Value::Object` so callers don't need rmcp types.
    let schema = serde_json::Value::Object((*tool.input_schema).clone());
    McpToolDescriptor {
        name: namespace(server, &tool.name),
        description,
        input_schema: schema,
    }
}

fn call_result_to_mcp(result: CallToolResult) -> McpToolResult {
    let is_error = result.is_error.unwrap_or(false);

    // Prefer structured_content when the server produced it (SEP-1319 path);
    // otherwise collapse the `content` vec into a JSON array of its
    // text/image/resource payloads so the default tool-use transcript keeps
    // something sensible.
    if let Some(structured) = result.structured_content {
        return McpToolResult {
            content: structured,
            is_error,
        };
    }

    let rendered: Vec<serde_json::Value> = result
        .content
        .into_iter()
        .map(|c| match c.raw {
            RawContent::Text(t) => serde_json::json!({"type": "text", "text": t.text}),
            RawContent::Image(i) => serde_json::json!({
                "type": "image",
                "mime_type": i.mime_type,
                "data": i.data,
            }),
            RawContent::Audio(a) => serde_json::json!({
                "type": "audio",
                "mime_type": a.mime_type,
                "data": a.data,
            }),
            RawContent::Resource(r) => {
                serde_json::to_value(&r).unwrap_or(serde_json::Value::Null)
            }
            RawContent::ResourceLink(r) => {
                serde_json::to_value(&r).unwrap_or(serde_json::Value::Null)
            }
        })
        .collect();

    McpToolResult {
        content: serde_json::Value::Array(rendered),
        is_error,
    }
}

// ---------------------------------------------------------------------------
// McpClientBridge impl
// ---------------------------------------------------------------------------

#[async_trait]
impl McpClientBridge for RmcpClientBridge {
    async fn connect(&self, config: &McpServerConfig) -> Result<(), McpError> {
        // Sanity: the bridge was built with a specific config. Reject attempts
        // to point it at a different logical server by accident.
        if config.name != self.config.name {
            return Err(McpError::ConnectionFailed {
                reason: format!(
                    "bridge built for server '{}', refused connect for '{}'",
                    self.config.name, config.name
                ),
            });
        }

        let running = match config.transport {
            McpTransport::Stdio => connect_stdio(config).await?,
            McpTransport::Sse | McpTransport::StreamableHttp => connect_http_like(config).await?,
        };

        let mut slot = self.running.write().await;
        // Drop any previous service cleanly before replacing.
        if let Some(prev) = slot.take() {
            let _ = prev.cancel().await;
        }
        *slot = Some(running);
        Ok(())
    }

    async fn disconnect(&self, server_name: &str) -> Result<(), McpError> {
        if server_name != self.config.name {
            return Err(McpError::UnknownServer(server_name.to_string()));
        }
        let mut slot = self.running.write().await;
        if let Some(service) = slot.take() {
            // `cancel` consumes the RunningService and waits for the
            // background loop to drain. We intentionally swallow a JoinError
            // here — the process is going away regardless, and a panic in the
            // rmcp task shouldn't surface as a user-facing Transport error.
            let _ = service.cancel().await;
        }
        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        let guard = self.running.read().await;
        let running = guard.as_ref().ok_or(McpError::NotConnected)?;
        let tools = running
            .peer()
            .list_all_tools()
            .await
            .map_err(|e| McpError::transport(format!("list_tools: {e}")))?;
        Ok(tools
            .iter()
            .map(|t| tool_to_descriptor(&self.config.name, t))
            .collect())
    }

    async fn list_server_tools(
        &self,
        server_name: &str,
    ) -> Result<Vec<McpToolDescriptor>, McpError> {
        if server_name != self.config.name {
            return Err(McpError::UnknownServer(server_name.to_string()));
        }
        self.list_tools().await
    }

    async fn call_tool(
        &self,
        namespaced_tool: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolResult, McpError> {
        // Strip the `{server}.` prefix if present; otherwise treat the whole
        // string as the tool name (matches gating convention for built-ins).
        let (maybe_server, bare_tool) = split_namespace(namespaced_tool);
        if let Some(server) = maybe_server
            && server != self.config.name
        {
            return Err(McpError::UnknownServer(server.to_string()));
        }

        // MCP spec: `arguments` is an object or absent. Reject non-object
        // JSON up front with a clear serialization error instead of letting
        // rmcp return an opaque transport failure.
        let args_object: Option<JsonObject> = match arguments {
            serde_json::Value::Null => None,
            serde_json::Value::Object(m) => Some(m),
            other => {
                return Err(McpError::Serialization {
                    reason: format!(
                        "tool arguments must be a JSON object or null, got {:?}",
                        other
                    ),
                });
            }
        };

        let mut params = CallToolRequestParams::new(bare_tool.to_string());
        if let Some(obj) = args_object {
            params = params.with_arguments(obj);
        }

        let guard = self.running.read().await;
        let running = guard.as_ref().ok_or(McpError::NotConnected)?;
        let result = running
            .peer()
            .call_tool(params)
            .await
            .map_err(|e| McpError::transport(format!("call_tool '{bare_tool}': {e}")))?;

        Ok(call_result_to_mcp(result))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn stdio_cfg(name: &str) -> McpServerConfig {
        McpServerConfig {
            name: name.into(),
            url: None,
            command: Some("true".into()),
            args: vec![],
            transport: McpTransport::Stdio,
            env: HashMap::new(),
        }
    }

    fn sse_cfg(name: &str) -> McpServerConfig {
        McpServerConfig {
            name: name.into(),
            url: Some("http://127.0.0.1:0/mcp".into()),
            command: None,
            args: vec![],
            transport: McpTransport::Sse,
            env: HashMap::new(),
        }
    }

    fn http_cfg(name: &str) -> McpServerConfig {
        McpServerConfig {
            name: name.into(),
            url: Some("http://127.0.0.1:0/mcp".into()),
            command: None,
            args: vec![],
            transport: McpTransport::StreamableHttp,
            env: HashMap::new(),
        }
    }

    // ---- Construction ----------------------------------------------------

    #[tokio::test]
    async fn new_accepts_stdio_config() {
        let bridge = RmcpClientBridge::new(stdio_cfg("local-fs"));
        assert_eq!(bridge.config().name, "local-fs");
        assert!(!bridge.is_connected().await);
    }

    #[tokio::test]
    async fn new_accepts_sse_config() {
        let bridge = RmcpClientBridge::new(sse_cfg("web-api"));
        assert_eq!(bridge.config().transport, McpTransport::Sse);
        assert!(!bridge.is_connected().await);
    }

    #[tokio::test]
    async fn new_accepts_streamable_http_config() {
        let bridge = RmcpClientBridge::new(http_cfg("web-api-http"));
        assert_eq!(bridge.config().transport, McpTransport::StreamableHttp);
        assert!(!bridge.is_connected().await);
    }

    // ---- Not-connected semantics -----------------------------------------

    #[tokio::test]
    async fn list_tools_when_not_connected_errors() {
        let bridge = RmcpClientBridge::new(stdio_cfg("srv"));
        let err = bridge.list_tools().await.unwrap_err();
        assert!(matches!(err, McpError::NotConnected));
    }

    #[tokio::test]
    async fn list_server_tools_when_not_connected_errors() {
        let bridge = RmcpClientBridge::new(stdio_cfg("srv"));
        let err = bridge.list_server_tools("srv").await.unwrap_err();
        assert!(matches!(err, McpError::NotConnected));
    }

    #[tokio::test]
    async fn list_server_tools_wrong_name_errors_before_connect_check() {
        // `UnknownServer` beats `NotConnected` because server-name is a
        // programming error; no point pretending the transport was the
        // problem.
        let bridge = RmcpClientBridge::new(stdio_cfg("srv"));
        let err = bridge.list_server_tools("other").await.unwrap_err();
        assert!(matches!(err, McpError::UnknownServer(ref s) if s == "other"));
    }

    #[tokio::test]
    async fn call_tool_when_not_connected_errors() {
        let bridge = RmcpClientBridge::new(stdio_cfg("srv"));
        let err = bridge
            .call_tool("srv.whatever", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::NotConnected));
    }

    #[tokio::test]
    async fn call_tool_wrong_server_namespace_errors() {
        let bridge = RmcpClientBridge::new(stdio_cfg("srv"));
        let err = bridge
            .call_tool("other.tool", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::UnknownServer(ref s) if s == "other"));
    }

    #[tokio::test]
    async fn call_tool_rejects_non_object_arguments() {
        // Note: this test also reaches the `NotConnected` branch after
        // shape-check, which would clobber the Serialization error. To
        // isolate the shape check we build params inline instead of going
        // through `call_tool`. The `call_tool` path asserts shape BEFORE
        // reading `running`, so we test it through the real entrypoint:
        let bridge = RmcpClientBridge::new(stdio_cfg("srv"));
        let err = bridge
            .call_tool("srv.tool", serde_json::json!("a string"))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::Serialization { .. }));
    }

    #[tokio::test]
    async fn call_tool_rejects_array_arguments() {
        let bridge = RmcpClientBridge::new(stdio_cfg("srv"));
        let err = bridge
            .call_tool("srv.tool", serde_json::json!([1, 2, 3]))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::Serialization { .. }));
    }

    #[tokio::test]
    async fn call_tool_rejects_number_arguments() {
        let bridge = RmcpClientBridge::new(stdio_cfg("srv"));
        let err = bridge
            .call_tool("srv.tool", serde_json::json!(42))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::Serialization { .. }));
    }

    #[tokio::test]
    async fn call_tool_null_arguments_reaches_not_connected() {
        // Null args are valid per MCP spec — the shape-check passes and
        // the error comes from the not-connected check instead.
        let bridge = RmcpClientBridge::new(stdio_cfg("srv"));
        let err = bridge
            .call_tool("srv.tool", serde_json::Value::Null)
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::NotConnected));
    }

    #[tokio::test]
    async fn call_tool_bare_name_no_namespace_reaches_not_connected() {
        // A bare tool name (no ".") has no server prefix to validate,
        // so the check passes and we fall through to NotConnected.
        let bridge = RmcpClientBridge::new(stdio_cfg("srv"));
        let err = bridge
            .call_tool("bare_tool", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::NotConnected));
    }

    // ---- Disconnect semantics --------------------------------------------

    #[tokio::test]
    async fn disconnect_when_not_connected_is_noop() {
        let bridge = RmcpClientBridge::new(stdio_cfg("srv"));
        bridge.disconnect("srv").await.expect("no-op disconnect");
        assert!(!bridge.is_connected().await);
    }

    #[tokio::test]
    async fn disconnect_wrong_server_name_errors() {
        let bridge = RmcpClientBridge::new(stdio_cfg("srv"));
        let err = bridge.disconnect("not-srv").await.unwrap_err();
        assert!(matches!(err, McpError::UnknownServer(ref s) if s == "not-srv"));
    }

    // ---- Mismatched connect config ---------------------------------------

    #[tokio::test]
    async fn connect_with_mismatched_name_errors() {
        let bridge = RmcpClientBridge::new(stdio_cfg("srv"));
        let other = stdio_cfg("other");
        let err = bridge.connect(&other).await.unwrap_err();
        assert!(matches!(err, McpError::ConnectionFailed { .. }));
    }

    #[tokio::test]
    async fn connect_stdio_missing_command_errors() {
        let mut cfg = stdio_cfg("srv");
        cfg.command = None;
        let bridge = RmcpClientBridge::new(cfg.clone());
        let err = bridge.connect(&cfg).await.unwrap_err();
        assert!(matches!(err, McpError::ConnectionFailed { .. }));
    }

    #[tokio::test]
    async fn connect_sse_missing_url_errors() {
        let mut cfg = sse_cfg("srv");
        cfg.url = None;
        let bridge = RmcpClientBridge::new(cfg.clone());
        let err = bridge.connect(&cfg).await.unwrap_err();
        assert!(matches!(err, McpError::ConnectionFailed { .. }));
    }

    // ---- Tool mapping ----------------------------------------------------

    #[test]
    fn tool_to_descriptor_namespaces_name() {
        let mut schema = serde_json::Map::new();
        schema.insert("type".into(), serde_json::Value::String("object".into()));
        let tool = Tool::new("create_issue", "Create a GitHub issue", Arc::new(schema));
        let desc = tool_to_descriptor("github", &tool);
        assert_eq!(desc.name, "github.create_issue");
        assert_eq!(desc.description, "Create a GitHub issue");
        assert_eq!(
            desc.input_schema.get("type").and_then(|v| v.as_str()),
            Some("object")
        );
    }

    #[test]
    fn call_result_to_mcp_prefers_structured_content() {
        let raw = CallToolResult::structured(serde_json::json!({"ok": true}));
        let mapped = call_result_to_mcp(raw);
        assert!(!mapped.is_error);
        assert_eq!(mapped.content, serde_json::json!({"ok": true}));
    }

    #[test]
    fn call_result_to_mcp_renders_text_content_array() {
        use rmcp::model::Content;
        let raw = CallToolResult::error(vec![Content::text("hello")]);
        let mapped = call_result_to_mcp(raw);
        assert!(mapped.is_error);
        let arr = mapped.content.as_array().expect("array rendering");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("text"));
        assert_eq!(arr[0].get("text").and_then(|v| v.as_str()), Some("hello"));
    }

    #[test]
    fn call_result_to_mcp_empty_content_renders_empty_array() {
        let raw = CallToolResult {
            content: vec![],
            is_error: Some(false),
            structured_content: None,
            meta: None,
        };
        let mapped = call_result_to_mcp(raw);
        assert!(!mapped.is_error);
        let arr = mapped.content.as_array().expect("should be array");
        assert!(arr.is_empty());
    }

    #[test]
    fn call_result_to_mcp_renders_multiple_text_items() {
        use rmcp::model::Content;
        let raw = CallToolResult::success(vec![
            Content::text("line one"),
            Content::text("line two"),
        ]);
        let mapped = call_result_to_mcp(raw);
        assert!(!mapped.is_error);
        let arr = mapped.content.as_array().expect("array rendering");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[1].get("text").and_then(|v| v.as_str()), Some("line two"));
    }

    #[test]
    fn call_result_to_mcp_is_error_none_defaults_false() {
        let raw = CallToolResult {
            content: vec![],
            is_error: None,
            structured_content: None,
            meta: None,
        };
        let mapped = call_result_to_mcp(raw);
        assert!(!mapped.is_error);
    }

    #[test]
    fn tool_to_descriptor_empty_description() {
        let schema = serde_json::Map::new();
        // Tool with no description — should map to empty string, not panic.
        let tool = Tool::new("bare_tool", None::<&str>, Arc::new(schema));
        let desc = tool_to_descriptor("srv", &tool);
        assert_eq!(desc.name, "srv.bare_tool");
        assert_eq!(desc.description, "");
    }

    // ---- Integration (gated) ---------------------------------------------

    // Real end-to-end test against a live MCP server process. Gated behind
    // the `rmcp-integration` feature because it requires an external binary
    // at runtime. See sera-ogl8 follow-up note for picking a stable test
    // server to wire up here (candidates: the `everything` reference server
    // from modelcontextprotocol/servers, or a 20-line python script).
    #[cfg(feature = "rmcp-integration")]
    #[tokio::test]
    #[ignore = "TODO(sera-ogl8 follow-up): wire a real stdio test server"]
    async fn integration_stdio_list_tools_returns_nonempty() {
        let cfg = McpServerConfig {
            name: "test-echo".into(),
            url: None,
            command: Some("mcp-test-echo".into()),
            args: vec![],
            transport: McpTransport::Stdio,
            env: HashMap::new(),
        };
        let bridge = RmcpClientBridge::new(cfg.clone());
        bridge.connect(&cfg).await.expect("connect");
        let tools = bridge.list_tools().await.expect("list_tools");
        assert!(!tools.is_empty());
        for t in &tools {
            assert!(t.name.starts_with("test-echo."));
        }
        bridge.disconnect("test-echo").await.expect("disconnect");
    }
}
