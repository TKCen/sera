//! Tool executor framework and registry.

pub mod file_ops;
pub mod http_request;
pub mod shell_exec;
pub mod knowledge;
pub mod web_fetch;
pub mod glob;
pub mod grep;
pub mod spawn;
pub mod tool_search;
pub mod centrifugo;

use crate::types::{FunctionDefinition, ToolDefinition};

/// Trait for tool executors.
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String>;
}

/// Registry of available tools.
pub struct ToolRegistry {
    tools: Vec<Box<dyn ToolExecutor>>,
}

impl ToolRegistry {
    /// Create a registry with all built-in tools.
    pub fn new() -> Self {
        let tools: Vec<Box<dyn ToolExecutor>> = vec![
            Box::new(file_ops::FileRead),
            Box::new(file_ops::FileWrite),
            Box::new(file_ops::FileList),
            Box::new(shell_exec::ShellExec),
            Box::new(http_request::HttpRequest),
            Box::new(knowledge::KnowledgeStore),
            Box::new(knowledge::KnowledgeQuery),
            Box::new(web_fetch::WebFetch),
            Box::new(glob::Glob),
            Box::new(grep::Grep),
            Box::new(spawn::SpawnEphemeral),
            Box::new(tool_search::ToolSearch),
            Box::new(tool_search::SkillSearch),
        ];
        Self { tools }
    }

    /// Get tool definitions for the LLM.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .map(|t| ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    parameters: t.parameters(),
                },
            })
            .collect()
    }

    /// Execute a tool by name.
    pub async fn execute(&self, name: &str, args: &serde_json::Value) -> anyhow::Result<String> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {name}"))?;
        tool.execute(args).await
    }
}
