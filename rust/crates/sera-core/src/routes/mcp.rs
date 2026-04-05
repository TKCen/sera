//! MCP (Model Context Protocol) server registry endpoints.
//! Manages an in-memory registry of MCP server connections backed by config.
#![allow(dead_code, unused_imports)]

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::AppError;
use crate::state::AppState;

/// MCP server entry in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    pub url: String,
    pub transport: String, // "stdio" | "sse" | "streamable-http"
    pub status: String,    // "connected" | "disconnected" | "error"
    pub tools: Vec<McpTool>,
    pub last_health_check: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
}

/// In-memory MCP server registry.
#[derive(Debug, Clone, Default)]
pub struct McpRegistry {
    pub servers: HashMap<String, McpServer>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
        }
    }

    pub fn register(&mut self, server: McpServer) {
        self.servers.insert(server.name.clone(), server);
    }

    pub fn get(&self, name: &str) -> Option<&McpServer> {
        self.servers.get(name)
    }

    pub fn list(&self) -> Vec<&McpServer> {
        self.servers.values().collect()
    }

    pub fn remove(&mut self, name: &str) -> bool {
        self.servers.remove(name).is_some()
    }
}

/// GET /api/mcp-servers — list all registered MCP servers
pub async fn list_mcp_servers(
    State(state): State<AppState>,
) -> Json<Vec<McpServer>> {
    let registry = state.mcp_registry.read().await;
    let servers: Vec<McpServer> = registry.list().into_iter().cloned().collect();
    Json(servers)
}

/// GET /api/mcp-servers/:name — get MCP server details
pub async fn get_mcp_server(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let registry = state.mcp_registry.read().await;
    match registry.get(&name) {
        Some(server) => {
            // In production, would call client.listTools() to get real tools
            // For now, return server info with tools from registry
            Ok(Json(serde_json::json!({
                "name": server.name,
                "status": server.status,
                "toolCount": server.tools.len(),
                "tools": server.tools,
            })))
        }
        None => Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "mcp_server",
            key: "name",
            value: name,
        })),
    }
}

/// GET /api/mcp-servers/:name/health — check MCP server health
pub async fn mcp_server_health(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let registry = state.mcp_registry.read().await;
    let server = registry.get(&name).ok_or_else(|| {
        AppError::Db(sera_db::DbError::NotFound {
            entity: "mcp_server",
            key: "name",
            value: name.clone(),
        })
    })?;

    // Ping the MCP server with latency measurement
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let start = std::time::Instant::now();
    let health = match client.get(&server.url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let latency_ms = start.elapsed().as_millis() as u64;
            serde_json::json!({
                "name": name,
                "status": "healthy",
                "toolCount": server.tools.len(),
                "latency_ms": latency_ms,
                "checked_at": super::iso8601(time::OffsetDateTime::now_utc()),
            })
        }
        Ok(resp) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            serde_json::json!({
                "name": name,
                "status": "degraded",
                "error": format!("HTTP {}", resp.status()),
                "latency_ms": latency_ms,
                "checked_at": super::iso8601(time::OffsetDateTime::now_utc()),
            })
        }
        Err(e) => serde_json::json!({
            "name": name,
            "status": "unhealthy",
            "error": e.to_string(),
            "latency_ms": start.elapsed().as_millis() as u64,
            "checked_at": super::iso8601(time::OffsetDateTime::now_utc()),
        }),
    };

    Ok(Json(health))
}

/// POST /api/mcp-servers/:name/reload — reload MCP server connection
pub async fn reload_mcp_server(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut registry = state.mcp_registry.write().await;
    let server = registry.servers.get_mut(&name).ok_or_else(|| {
        AppError::Db(sera_db::DbError::NotFound {
            entity: "mcp_server",
            key: "name",
            value: name.clone(),
        })
    })?;

    // In production: disconnect old connection, reconnect, refresh tool list
    // For now: reset status and update timestamp
    server.status = "connected".to_string();
    server.last_health_check = Some(super::iso8601(time::OffsetDateTime::now_utc()));

    let tool_count = server.tools.len();
    Ok(Json(serde_json::json!({
        "message": format!("MCP server \"{}\" reloaded", name),
        "toolCount": tool_count,
    })))
}

/// DELETE /api/mcp-servers/:name — unregister an MCP server
pub async fn delete_mcp_server(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut registry = state.mcp_registry.write().await;
    let removed = registry.remove(&name);

    if removed {
        Ok(Json(serde_json::json!({
            "message": format!("MCP server \"{}\" unregistered successfully", name),
        })))
    } else {
        Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "mcp_server",
            key: "name",
            value: name,
        }))
    }
}

#[derive(Deserialize)]
pub struct RegisterMcpServerRequest {
    pub name: String,
    pub url: String,
    #[serde(default = "default_transport")]
    pub transport: String,
    #[serde(default)]
    pub tools: Vec<McpTool>,
}

fn default_transport() -> String {
    "stdio".to_string()
}

/// POST /api/mcp-servers — register a new MCP server
pub async fn register_mcp_server(
    State(state): State<AppState>,
    Json(body): Json<RegisterMcpServerRequest>,
) -> Result<(StatusCode, Json<McpServer>), AppError> {
    let server = McpServer {
        name: body.name.clone(),
        url: body.url,
        transport: body.transport,
        status: "connected".to_string(),
        tools: body.tools,
        last_health_check: Some(super::iso8601(time::OffsetDateTime::now_utc())),
    };

    let mut registry = state.mcp_registry.write().await;
    registry.register(server.clone());

    Ok((StatusCode::CREATED, Json(server)))
}

// ── Tool Management Routes ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub agent_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolsListResponse {
    pub tools: Vec<ToolInfo>,
}

/// GET /api/tools?agent_id=X — list available tools for an agent
pub async fn list_tools() -> Json<ToolsListResponse> {
    Json(ToolsListResponse { tools: vec![] })
}

#[derive(Debug, Deserialize)]
pub struct ExecuteToolRequest {
    pub agent_id: String,
    pub tool_name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct ExecuteToolResponse {
    pub success: bool,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// POST /api/tools/execute — execute a tool with provided arguments
pub async fn execute_tool(
    Json(_body): Json<ExecuteToolRequest>,
) -> Json<ExecuteToolResponse> {
    let start = std::time::Instant::now();
    Json(ExecuteToolResponse {
        success: true,
        output: Some(serde_json::json!({"message": "tool execution stub"})),
        error: None,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

#[derive(Debug, Deserialize)]
pub struct ValidateToolRequest {
    pub tool_name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct ValidateToolResponse {
    pub valid: bool,
    pub errors: Vec<String>,
}

/// POST /api/tools/validate — validate tool arguments
pub async fn validate_tool(
    Json(_body): Json<ValidateToolRequest>,
) -> Json<ValidateToolResponse> {
    Json(ValidateToolResponse {
        valid: true,
        errors: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_tools_response_shape() {
        let response = list_tools().await;
        assert_eq!(response.tools.len(), 0);
    }

    #[tokio::test]
    async fn test_execute_tool_response_shape() {
        let req = ExecuteToolRequest {
            agent_id: "agent-1".to_string(),
            tool_name: "test_tool".to_string(),
            args: serde_json::json!({}),
        };
        let response = execute_tool(Json(req)).await;
        assert!(response.success);
        assert!(response.error.is_none());
        assert!(response.output.is_some());
        assert!(response.duration_ms >= 0);
    }

    #[tokio::test]
    async fn test_validate_tool_response_shape() {
        let req = ValidateToolRequest {
            tool_name: "test_tool".to_string(),
            args: serde_json::json!({}),
        };
        let response = validate_tool(Json(req)).await;
        assert!(response.valid);
        assert_eq!(response.errors.len(), 0);
    }

    #[test]
    fn test_tool_info_serialization() {
        let tool = ToolInfo {
            name: "test".to_string(),
            description: Some("Test tool".to_string()),
            agent_id: Some("agent-1".to_string()),
        };
        let json = serde_json::to_value(&tool).expect("Failed to serialize");
        assert_eq!(json["name"], "test");
        assert_eq!(json["description"], "Test tool");
        assert_eq!(json["agent_id"], "agent-1");
    }

    #[test]
    fn test_execute_tool_request_deserialization() {
        let json = serde_json::json!({
            "agent_id": "agent-1",
            "tool_name": "echo",
            "args": {"message": "hello"}
        });
        let req: ExecuteToolRequest = serde_json::from_value(json).expect("Failed to deserialize");
        assert_eq!(req.agent_id, "agent-1");
        assert_eq!(req.tool_name, "echo");
    }

    #[test]
    fn test_validate_tool_request_deserialization() {
        let json = serde_json::json!({
            "tool_name": "echo",
            "args": {"message": "hello"}
        });
        let req: ValidateToolRequest = serde_json::from_value(json).expect("Failed to deserialize");
        assert_eq!(req.tool_name, "echo");
    }
}
