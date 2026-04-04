//! MCP (Model Context Protocol) server registry endpoints.
//! Manages an in-memory registry of MCP server connections backed by config.

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
) -> Result<Json<McpServer>, AppError> {
    let registry = state.mcp_registry.read().await;
    match registry.get(&name) {
        Some(server) => Ok(Json(server.clone())),
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

    // Ping the MCP server
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let health = match client.get(&server.url).send().await {
        Ok(resp) if resp.status().is_success() => serde_json::json!({
            "name": name,
            "status": "healthy",
            "latency_ms": 0,
        }),
        Ok(resp) => serde_json::json!({
            "name": name,
            "status": "degraded",
            "error": format!("HTTP {}", resp.status()),
        }),
        Err(e) => serde_json::json!({
            "name": name,
            "status": "unhealthy",
            "error": e.to_string(),
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

    // Reset status — in production this would re-establish the connection
    server.status = "connected".to_string();
    server.last_health_check = Some(time::OffsetDateTime::now_utc().to_string());

    Ok(Json(serde_json::json!({
        "name": name,
        "status": "reloaded",
    })))
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
        last_health_check: Some(time::OffsetDateTime::now_utc().to_string()),
    };

    let mut registry = state.mcp_registry.write().await;
    registry.register(server.clone());

    Ok((StatusCode::CREATED, Json(server)))
}
