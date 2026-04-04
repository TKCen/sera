//! MCP server endpoints (stub — MCP registry is in-memory, not DB-backed).

use axum::Json;

/// GET /api/mcp-servers — list registered MCP servers (stub).
pub async fn list_mcp_servers() -> Json<Vec<serde_json::Value>> {
    // MCP servers are managed in-memory by the MCP registry.
    // Full implementation requires porting McpRegistry from TS.
    Json(vec![])
}
