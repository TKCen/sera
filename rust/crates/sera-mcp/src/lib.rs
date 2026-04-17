//! sera-mcp — MCP (Model Context Protocol) server and client bridge.
//!
//! SERA acts as both MCP **Server** (exposing SERA tools to external LLMs)
//! and MCP **Client** (consuming external MCP servers as tool sources).
//!
//! ## Dependency model
//!
//! Depends on [`rmcp`](https://crates.io/crates/rmcp) ^1.3 (Anthropic official SDK).
//! The rmcp dependency is deferred until gateway integration — this crate
//! currently defines the trait interfaces and configuration types so that
//! `sera-gateway` and `sera-runtime` can code against them.
//!
//! See SPEC-interop §3 for the full protocol specification.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sera_errors::{SeraError, SeraErrorCode};
use std::collections::HashMap;
use thiserror::Error;

pub mod gating;
pub use gating::{
    AllowedServerGate, AlwaysVisibleGate, AndGate, GatedMcpClientBridge, McpToolGate, OrGate,
    SkillBoundGate, ToolGatingContext,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum McpError {
    #[error("transport error: {reason}")]
    Transport { reason: String },
    #[error("tool not found: {name}")]
    ToolNotFound { name: String },
    #[error("server not found: {name}")]
    ServerNotFound { name: String },
    #[error("authorization denied for tool {tool}")]
    Unauthorized { tool: String },
    #[error("serialization error: {reason}")]
    Serialization { reason: String },
    #[error("connection failed: {reason}")]
    ConnectionFailed { reason: String },
}

impl From<McpError> for SeraError {
    fn from(err: McpError) -> Self {
        let code = match &err {
            McpError::Transport { .. } => SeraErrorCode::Unavailable,
            McpError::ToolNotFound { .. } => SeraErrorCode::NotFound,
            McpError::ServerNotFound { .. } => SeraErrorCode::NotFound,
            McpError::Unauthorized { .. } => SeraErrorCode::Unauthorized,
            McpError::Serialization { .. } => SeraErrorCode::Serialization,
            McpError::ConnectionFailed { .. } => SeraErrorCode::Unavailable,
        };
        SeraError::new(code, err.to_string())
    }
}

// ---------------------------------------------------------------------------
// Configuration types (from SPEC-interop §3.3 and §8)
// ---------------------------------------------------------------------------

/// Transport mode for MCP connections.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum McpTransport {
    Stdio,
    Sse,
    StreamableHttp,
}

/// Configuration for a single external MCP server connection.
///
/// Agents declare these in their manifest under `mcp_servers`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Logical name used to namespace tools (e.g. "github" → `github.create_issue`).
    pub name: String,
    /// URL for SSE/HTTP transports.
    pub url: Option<String>,
    /// Command for stdio transport.
    pub command: Option<String>,
    /// Arguments for the stdio command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Transport type.
    pub transport: McpTransport,
    /// Optional environment variables passed to stdio processes.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// MCP server configuration (SERA acting as MCP server).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerSettings {
    pub enabled: bool,
    #[serde(default = "default_mcp_port")]
    pub port: u16,
}

fn default_mcp_port() -> u16 {
    50052
}

impl Default for McpServerSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 50052,
        }
    }
}

// ---------------------------------------------------------------------------
// Tool descriptor (protocol-agnostic, maps to MCP Tool schema)
// ---------------------------------------------------------------------------

/// A tool exposed via MCP, with a JSON Schema for its input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDescriptor {
    /// Fully-qualified name including namespace (e.g. `github.create_issue`).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: serde_json::Value,
}

/// Result of invoking a tool through the MCP bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    pub content: serde_json::Value,
    pub is_error: bool,
}

// ---------------------------------------------------------------------------
// Server trait — SERA exposing tools via MCP
// ---------------------------------------------------------------------------

/// Trait for the SERA MCP server that exposes tools to external callers.
///
/// Implementors provide tool discovery and invocation. The gateway wires
/// this into an MCP-compliant transport (stdio/SSE/streamable-http).
#[async_trait]
pub trait McpServer: Send + Sync + 'static {
    /// List all tools available to the given caller.
    ///
    /// `gate_ctx` is an optional [`ToolGatingContext`] — when `Some`, the
    /// server may filter its tool list to match the active skill bindings,
    /// allowed server namespaces, or other policy in the context. When `None`,
    /// the server returns its unfiltered tool list (legacy behavior).
    async fn list_tools(
        &self,
        caller_id: &str,
        gate_ctx: Option<&ToolGatingContext>,
    ) -> Result<Vec<McpToolDescriptor>, McpError>;

    /// Invoke a tool on behalf of an external caller.
    async fn call_tool(
        &self,
        caller_id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolResult, McpError>;
}

// ---------------------------------------------------------------------------
// Client bridge trait — SERA consuming external MCP servers
// ---------------------------------------------------------------------------

/// Trait for the MCP client bridge that connects to external MCP servers.
///
/// Each connected server's tools are namespaced by the server name from
/// [`McpServerConfig`] to avoid collisions with built-in tools.
#[async_trait]
pub trait McpClientBridge: Send + Sync + 'static {
    /// Connect to an external MCP server.
    async fn connect(&self, config: &McpServerConfig) -> Result<(), McpError>;

    /// Disconnect from a named MCP server.
    async fn disconnect(&self, server_name: &str) -> Result<(), McpError>;

    /// List tools from all connected servers (namespaced).
    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError>;

    /// List tools from a specific connected server.
    async fn list_server_tools(
        &self,
        server_name: &str,
    ) -> Result<Vec<McpToolDescriptor>, McpError>;

    /// Call a namespaced tool (e.g. `github.create_issue`).
    async fn call_tool(
        &self,
        namespaced_tool: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolResult, McpError>;

    /// List tools filtered for a specific gating context.
    ///
    /// Default implementation returns the full unfiltered list from
    /// [`Self::list_tools`] — existing implementors don't break. Types that
    /// wrap a bridge with a gate (see [`GatedMcpClientBridge`]) override this
    /// to apply the filter and truncate to `ctx.max_tools`.
    async fn list_tools_for_context(
        &self,
        _ctx: &ToolGatingContext,
    ) -> Result<Vec<McpToolDescriptor>, McpError> {
        self.list_tools().await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_transport_serde_roundtrip() {
        let t = McpTransport::StreamableHttp;
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"streamable-http\"");
        let back: McpTransport = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn mcp_server_config_deserialize() {
        let yaml = r#"
            name: github
            url: "http://localhost:3000"
            transport: sse
        "#;
        let config: McpServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name, "github");
        assert_eq!(config.transport, McpTransport::Sse);
        assert!(config.command.is_none());
    }

    #[test]
    fn mcp_server_config_stdio() {
        let yaml = r#"
            name: filesystem
            command: npx
            args: ["-y", "@modelcontextprotocol/server-filesystem", "/path"]
            transport: stdio
        "#;
        let config: McpServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.transport, McpTransport::Stdio);
        assert_eq!(config.command.as_deref(), Some("npx"));
        assert_eq!(config.args.len(), 3);
    }

    #[test]
    fn mcp_error_to_sera_error() {
        let err = McpError::ToolNotFound {
            name: "foo.bar".into(),
        };
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::NotFound);
    }

    #[test]
    fn default_server_settings() {
        let s = McpServerSettings::default();
        assert!(s.enabled);
        assert_eq!(s.port, 50052);
    }
}
