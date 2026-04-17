//! Tool executor framework and registry.

pub mod file_ops;
pub mod file_write;
pub mod file_edit;
pub mod http_request;
pub mod shell_exec;
pub mod knowledge;
pub mod web_fetch;
pub mod glob;
pub mod grep;
pub mod spawn;
pub mod tool_search;
pub mod centrifugo;
pub mod mvs_tools;
pub mod dispatcher;

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
            Box::new(file_edit::FileEdit),
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

    /// Look up a tool executor by name.
    pub fn get(&self, name: &str) -> Option<&dyn ToolExecutor> {
        self.tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Trait-based registry (SPEC-tools aligned) ────────────────────────────────

use std::collections::HashMap;
use sera_types::tool::{Tool, ToolInput, ToolOutput, ToolError, ToolContext, ToolMetadata};

/// Spec-aligned tool registry using the Tool trait.
#[allow(dead_code)]
pub struct TraitToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl TraitToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool by its metadata name.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.metadata().name.clone();
        self.tools.insert(name, tool);
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Return metadata for all registered tools.
    pub fn list(&self) -> Vec<ToolMetadata> {
        self.tools.values().map(|t| t.metadata()).collect()
    }

    /// Return OpenAI-format definitions for all registered tools.
    pub fn definitions(&self) -> Vec<crate::types::ToolDefinition> {
        self.tools
            .values()
            .map(|t| {
                let meta = t.metadata();
                let schema = t.schema();
                crate::types::ToolDefinition {
                    tool_type: "function".to_string(),
                    function: crate::types::FunctionDefinition {
                        name: meta.name,
                        description: meta.description,
                        parameters: serde_json::to_value(&schema.parameters)
                            .unwrap_or(serde_json::Value::Null),
                    },
                }
            })
            .collect()
    }

    /// Execute a tool by name, checking policy first.
    pub async fn execute(
        &self,
        input: ToolInput,
        ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        if !ctx.policy.allows(&input.name) {
            return Err(ToolError::PolicyDenied(input.name.clone()));
        }
        let tool = self
            .tools
            .get(&input.name)
            .ok_or_else(|| ToolError::NotFound(input.name.clone()))?;
        tool.execute(input, ctx).await
    }
}

impl Default for TraitToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod trait_registry_tests {
    use super::TraitToolRegistry;
    use sera_types::tool::{
        AuditHandle, CredentialBag, ExecutionTarget, FunctionParameters, RiskLevel,
        SessionRef, Tool, ToolContext, ToolError, ToolInput, ToolMetadata, ToolOutput,
        ToolPolicy, ToolProfile, ToolSchema,
    };
    use sera_types::principal::{PrincipalId, PrincipalRef};
    use std::collections::HashMap;

    // ── Mock tool for testing ─────────────────────────────────────────────

    struct EchoTool;

    #[async_trait::async_trait]
    impl Tool for EchoTool {
        fn metadata(&self) -> ToolMetadata {
            ToolMetadata {
                name: "echo".to_string(),
                description: "Echoes the input back".to_string(),
                version: "1.0.0".to_string(),
                author: None,
                risk_level: RiskLevel::Read,
                execution_target: ExecutionTarget::InProcess,
                tags: vec![],
            }
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema {
                parameters: FunctionParameters {
                    schema_type: "object".to_string(),
                    properties: HashMap::new(),
                    required: vec![],
                },
            }
        }

        async fn execute(
            &self,
            input: ToolInput,
            _ctx: ToolContext,
        ) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::success(format!(
                "echo: {}",
                input.arguments
            )))
        }
    }

    fn make_ctx(policy: ToolPolicy) -> ToolContext {
        ToolContext {
            session: SessionRef::new("test-session"),
            principal: PrincipalRef {
                id: PrincipalId("agent-001".to_string()),
                kind: sera_types::principal::PrincipalKind::Agent,
            },
            credentials: CredentialBag::new(),
            policy,
            audit_handle: AuditHandle {
                trace_id: "trace-1".to_string(),
                span_id: "span-1".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn register_and_execute() {
        let mut registry = TraitToolRegistry::new();
        registry.register(Box::new(EchoTool));

        let input = ToolInput {
            name: "echo".to_string(),
            arguments: serde_json::json!({"msg": "hello"}),
            call_id: "call-1".to_string(),
        };
        let ctx = make_ctx(ToolPolicy::from_profile(ToolProfile::Full));
        let output = registry.execute(input, ctx).await.unwrap();
        assert!(!output.is_error);
        assert!(output.content.starts_with("echo:"));
    }

    #[tokio::test]
    async fn policy_denial() {
        let mut registry = TraitToolRegistry::new();
        registry.register(Box::new(EchoTool));

        let input = ToolInput {
            name: "echo".to_string(),
            arguments: serde_json::json!({}),
            call_id: "call-2".to_string(),
        };
        // Deny everything
        let policy = ToolPolicy {
            profile: None,
            allow_patterns: vec![],
            deny_patterns: vec!["*".to_string()],
        };
        let ctx = make_ctx(policy);
        let err = registry.execute(input, ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::PolicyDenied(_)));
    }

    #[tokio::test]
    async fn unknown_tool_returns_not_found() {
        let registry = TraitToolRegistry::new();

        let input = ToolInput {
            name: "nonexistent".to_string(),
            arguments: serde_json::json!({}),
            call_id: "call-3".to_string(),
        };
        let ctx = make_ctx(ToolPolicy::from_profile(ToolProfile::Full));
        let err = registry.execute(input, ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[test]
    fn list_and_definitions() {
        let mut registry = TraitToolRegistry::new();
        registry.register(Box::new(EchoTool));

        let list = registry.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "echo");

        let defs = registry.definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].function.name, "echo");
        assert_eq!(defs[0].tool_type, "function");
    }

    #[test]
    fn get_returns_tool() {
        let mut registry = TraitToolRegistry::new();
        registry.register(Box::new(EchoTool));
        assert!(registry.get("echo").is_some());
        assert!(registry.get("missing").is_none());
    }
}
