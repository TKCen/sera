//! MCP Server Manager — lifecycle management for external MCP server processes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use time::OffsetDateTime;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Error type for MCP server management operations.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("failed to spawn process: {0}")]
    ProcessSpawn(String),
    #[error("process is not running")]
    ProcessDead,
    #[error("tool call failed: {0}")]
    ToolCall(String),
    #[error("server not found: {0}")]
    NotFound(String),
}

/// Transport method for MCP server communication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    /// Standard I/O pipes
    Stdio,
    /// Server-Sent Events
    Sse,
}

/// Configuration for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Name of the MCP server
    pub name: String,
    /// Command to execute the server
    pub command: String,
    /// Command-line arguments
    pub args: Vec<String>,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Transport method for communication
    pub transport: McpTransport,
}

/// Current status of an MCP server process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpServerStatus {
    /// Server process is running
    Running,
    /// Server process is stopped
    Stopped,
    /// Server encountered an error
    Error,
}

/// Information about a registered MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    /// Unique server ID
    pub id: Uuid,
    /// Server name
    pub name: String,
    /// Current server status
    pub status: McpServerStatus,
    /// When the server was registered
    pub registered_at: OffsetDateTime,
}

/// Internal storage for an MCP server entry.
#[derive(Debug, Clone)]
struct McpServerEntry {
    /// Server configuration
    config: McpServerConfig,
    /// Server information
    info: McpServerInfo,
}

/// Manages lifecycle of external MCP server processes.
pub struct McpServerManager {
    servers: Arc<RwLock<HashMap<Uuid, McpServerEntry>>>,
}

impl McpServerManager {
    /// Create a new MCP server manager.
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new MCP server configuration.
    ///
    /// Stores the configuration and returns a unique server ID.
    pub async fn register_server(&self, config: McpServerConfig) -> Result<Uuid, McpError> {
        let id = Uuid::new_v4();
        let registered_at = OffsetDateTime::now_utc();

        let info = McpServerInfo {
            id,
            name: config.name.clone(),
            status: McpServerStatus::Stopped,
            registered_at,
        };

        let entry = McpServerEntry { config, info };

        let mut servers = self.servers.write().await;
        servers.insert(id, entry);

        Ok(id)
    }

    /// Unregister an MCP server by ID.
    ///
    /// Removes the server from the registry. In a production system,
    /// this would also terminate the process if running.
    pub async fn unregister_server(&self, id: Uuid) -> Result<(), McpError> {
        let mut servers = self.servers.write().await;
        servers
            .remove(&id)
            .ok_or_else(|| McpError::NotFound(id.to_string()))?;
        Ok(())
    }

    /// List all registered MCP servers with their current status.
    pub async fn list_servers(&self) -> Vec<McpServerInfo> {
        let servers = self.servers.read().await;
        servers.values().map(|entry| entry.info.clone()).collect()
    }

    /// Call a tool on a specific MCP server.
    ///
    /// This is a stub implementation. A full implementation would
    /// communicate with the actual MCP server process.
    pub async fn call_tool(
        &self,
        server_id: Uuid,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let servers = self.servers.read().await;
        let entry = servers
            .get(&server_id)
            .ok_or_else(|| McpError::NotFound(server_id.to_string()))?;

        // Verify server is running
        if entry.info.status != McpServerStatus::Running {
            return Err(McpError::ProcessDead);
        }

        // Stub: in a full implementation, this would send a request to the MCP server
        // and await the response. For now, we return a success with the input args.
        Ok(serde_json::json!({
            "tool": tool_name,
            "args": args,
            "status": "success"
        }))
    }

    /// Check health of all registered servers.
    ///
    /// Verifies that processes are running and responsive.
    /// Currently a no-op stub; full implementation would ping each server.
    pub async fn check_health(&self) -> Result<(), McpError> {
        let servers = self.servers.read().await;

        // Stub: full implementation would verify each server's health
        // by attempting to communicate with it
        let _count = servers.len();

        Ok(())
    }

    /// Get configuration for a specific server.
    pub async fn get_config(&self, server_id: Uuid) -> Result<McpServerConfig, McpError> {
        let servers = self.servers.read().await;
        let entry = servers
            .get(&server_id)
            .ok_or_else(|| McpError::NotFound(server_id.to_string()))?;
        Ok(entry.config.clone())
    }

    /// Update the status of a server.
    pub async fn update_status(
        &self,
        server_id: Uuid,
        status: McpServerStatus,
    ) -> Result<(), McpError> {
        let mut servers = self.servers.write().await;
        let entry = servers
            .get_mut(&server_id)
            .ok_or_else(|| McpError::NotFound(server_id.to_string()))?;
        entry.info.status = status;
        Ok(())
    }
}

impl Default for McpServerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_server() {
        let manager = McpServerManager::new();
        let config = McpServerConfig {
            name: "test-server".to_string(),
            command: "test-cmd".to_string(),
            args: vec!["arg1".to_string(), "arg2".to_string()],
            env: HashMap::new(),
            transport: McpTransport::Stdio,
        };

        let id = manager
            .register_server(config)
            .await
            .expect("should register");
        assert!(!id.is_nil());
    }

    #[tokio::test]
    async fn test_register_multiple_servers() {
        let manager = McpServerManager::new();

        let id1 = manager
            .register_server(McpServerConfig {
                name: "server1".to_string(),
                command: "cmd1".to_string(),
                args: vec![],
                env: HashMap::new(),
                transport: McpTransport::Stdio,
            })
            .await
            .expect("should register");

        let id2 = manager
            .register_server(McpServerConfig {
                name: "server2".to_string(),
                command: "cmd2".to_string(),
                args: vec![],
                env: HashMap::new(),
                transport: McpTransport::Sse,
            })
            .await
            .expect("should register");

        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn test_list_servers() {
        let manager = McpServerManager::new();

        manager
            .register_server(McpServerConfig {
                name: "server1".to_string(),
                command: "cmd1".to_string(),
                args: vec![],
                env: HashMap::new(),
                transport: McpTransport::Stdio,
            })
            .await
            .expect("should register");

        manager
            .register_server(McpServerConfig {
                name: "server2".to_string(),
                command: "cmd2".to_string(),
                args: vec![],
                env: HashMap::new(),
                transport: McpTransport::Sse,
            })
            .await
            .expect("should register");

        let servers = manager.list_servers().await;
        assert_eq!(servers.len(), 2);
        assert!(servers.iter().any(|s| s.name == "server1"));
        assert!(servers.iter().any(|s| s.name == "server2"));
        assert!(servers.iter().all(|s| s.status == McpServerStatus::Stopped));
    }

    #[tokio::test]
    async fn test_unregister_server() {
        let manager = McpServerManager::new();

        let id = manager
            .register_server(McpServerConfig {
                name: "test-server".to_string(),
                command: "test-cmd".to_string(),
                args: vec![],
                env: HashMap::new(),
                transport: McpTransport::Stdio,
            })
            .await
            .expect("should register");

        let servers = manager.list_servers().await;
        assert_eq!(servers.len(), 1);

        manager
            .unregister_server(id)
            .await
            .expect("should unregister");

        let servers = manager.list_servers().await;
        assert_eq!(servers.len(), 0);
    }

    #[tokio::test]
    async fn test_unregister_nonexistent_server() {
        let manager = McpServerManager::new();
        let fake_id = Uuid::new_v4();

        let result = manager.unregister_server(fake_id).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), McpError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_get_config() {
        let manager = McpServerManager::new();

        let config = McpServerConfig {
            name: "test-server".to_string(),
            command: "test-cmd".to_string(),
            args: vec!["arg1".to_string()],
            env: {
                let mut m = HashMap::new();
                m.insert("KEY".to_string(), "value".to_string());
                m
            },
            transport: McpTransport::Stdio,
        };

        let id = manager
            .register_server(config.clone())
            .await
            .expect("should register");

        let retrieved = manager.get_config(id).await.expect("should get config");

        assert_eq!(retrieved.name, config.name);
        assert_eq!(retrieved.command, config.command);
        assert_eq!(retrieved.args, config.args);
        assert_eq!(retrieved.transport, config.transport);
    }

    #[tokio::test]
    async fn test_update_status() {
        let manager = McpServerManager::new();

        let id = manager
            .register_server(McpServerConfig {
                name: "test-server".to_string(),
                command: "test-cmd".to_string(),
                args: vec![],
                env: HashMap::new(),
                transport: McpTransport::Stdio,
            })
            .await
            .expect("should register");

        let servers = manager.list_servers().await;
        assert!(servers[0].status == McpServerStatus::Stopped);

        manager
            .update_status(id, McpServerStatus::Running)
            .await
            .expect("should update status");

        let servers = manager.list_servers().await;
        assert!(servers[0].status == McpServerStatus::Running);
    }

    #[tokio::test]
    async fn test_call_tool_on_running_server() {
        let manager = McpServerManager::new();

        let id = manager
            .register_server(McpServerConfig {
                name: "test-server".to_string(),
                command: "test-cmd".to_string(),
                args: vec![],
                env: HashMap::new(),
                transport: McpTransport::Stdio,
            })
            .await
            .expect("should register");

        manager
            .update_status(id, McpServerStatus::Running)
            .await
            .expect("should update status");

        let args = serde_json::json!({"key": "value"});
        let result = manager
            .call_tool(id, "test_tool", args.clone())
            .await
            .expect("should call tool");

        assert_eq!(
            result.get("tool").and_then(|v| v.as_str()),
            Some("test_tool")
        );
    }

    #[tokio::test]
    async fn test_call_tool_on_stopped_server() {
        let manager = McpServerManager::new();

        let id = manager
            .register_server(McpServerConfig {
                name: "test-server".to_string(),
                command: "test-cmd".to_string(),
                args: vec![],
                env: HashMap::new(),
                transport: McpTransport::Stdio,
            })
            .await
            .expect("should register");

        let result = manager
            .call_tool(id, "test_tool", serde_json::json!({}))
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), McpError::ProcessDead));
    }

    #[tokio::test]
    async fn test_config_serialization() {
        let config = McpServerConfig {
            name: "test-server".to_string(),
            command: "test-cmd".to_string(),
            args: vec!["arg1".to_string(), "arg2".to_string()],
            env: {
                let mut m = HashMap::new();
                m.insert("KEY".to_string(), "value".to_string());
                m
            },
            transport: McpTransport::Stdio,
        };

        let json = serde_json::to_string(&config).expect("should serialize");
        let deserialized: McpServerConfig =
            serde_json::from_str(&json).expect("should deserialize");

        assert_eq!(deserialized.name, config.name);
        assert_eq!(deserialized.command, config.command);
        assert_eq!(deserialized.args, config.args);
        assert_eq!(deserialized.transport, config.transport);
    }

    #[tokio::test]
    async fn test_server_info_serialization() {
        let info = McpServerInfo {
            id: Uuid::new_v4(),
            name: "test-server".to_string(),
            status: McpServerStatus::Running,
            registered_at: OffsetDateTime::now_utc(),
        };

        let json = serde_json::to_string(&info).expect("should serialize");
        let deserialized: McpServerInfo = serde_json::from_str(&json).expect("should deserialize");

        assert_eq!(deserialized.id, info.id);
        assert_eq!(deserialized.name, info.name);
        assert_eq!(deserialized.status, info.status);
    }

    #[tokio::test]
    async fn test_check_health() {
        let manager = McpServerManager::new();

        manager
            .register_server(McpServerConfig {
                name: "server1".to_string(),
                command: "cmd1".to_string(),
                args: vec![],
                env: HashMap::new(),
                transport: McpTransport::Stdio,
            })
            .await
            .expect("should register");

        let result = manager.check_health().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_mcp_transport_enum() {
        let stdio_json = serde_json::json!("stdio");
        let sse_json = serde_json::json!("sse");

        let stdio: McpTransport = serde_json::from_value(stdio_json).expect("should deserialize");
        let sse: McpTransport = serde_json::from_value(sse_json).expect("should deserialize");

        assert_eq!(stdio, McpTransport::Stdio);
        assert_eq!(sse, McpTransport::Sse);
    }

    #[test]
    fn test_mcp_server_status_enum() {
        let running_json = serde_json::json!("running");
        let stopped_json = serde_json::json!("stopped");
        let error_json = serde_json::json!("error");

        let running: McpServerStatus =
            serde_json::from_value(running_json).expect("should deserialize");
        let stopped: McpServerStatus =
            serde_json::from_value(stopped_json).expect("should deserialize");
        let error: McpServerStatus =
            serde_json::from_value(error_json).expect("should deserialize");

        assert_eq!(running, McpServerStatus::Running);
        assert_eq!(stopped, McpServerStatus::Stopped);
        assert_eq!(error, McpServerStatus::Error);
    }
}
