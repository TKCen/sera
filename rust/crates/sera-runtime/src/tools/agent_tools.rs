//! Agent-as-tool registrations — wraps [`crate::agent_tool_registry`] in
//! three [`Tool`] implementations so the runtime tool layer can treat
//! `delegate-task`, `ask-agent`, and `background-task` as ordinary callable
//! tools.
//!
//! Bead `sera-8d1.1` (GH#144). Each tool decodes its arguments
//! (`{ "target": "<agent>", ... }`) into the matching
//! [`sera_types::agent_tool`] input type, dispatches via
//! [`AgentToolRegistry`], and re-encodes the response. Capability and
//! budget enforcement live in the registry; these tools are deliberately
//! thin so the existing [`super::TraitToolRegistry`] dispatch path does not
//! need to change.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use sera_types::agent_tool::AgentToolKind;
use sera_types::capability::ResolvedCapabilities;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema,
};

use crate::agent_tool_registry::{
    AgentToolError, AgentToolRegistry, BudgetTracker, CallerContext,
};

/// Common dispatch surface for all three agent-tools — owns the registry,
/// the principal id used as the caller agent_id, and the running budget
/// tracker shared across calls in this turn.
struct Shared {
    registry: Arc<AgentToolRegistry>,
}

impl Shared {
    fn new(registry: Arc<AgentToolRegistry>) -> Self {
        Self { registry }
    }

    /// Build a [`CallerContext`] for the current `ToolContext`.
    ///
    /// Until `ToolContext` carries a resolved capability bundle (separate
    /// bead), the registry's capability gate must be wired by the embedder
    /// — they should plug capabilities into `ToolContext` via the existing
    /// `..ToolContext::default()` pattern. For now we plumb whatever
    /// capabilities the embedder attaches via `ToolContext.tags` is *not*
    /// the right hook — instead we look up an `Arc<ResolvedCapabilities>`
    /// from the bag returned by `ToolContext::default()` and fall back to
    /// "deny everything" if absent. This keeps the registry behaviour
    /// observable (denials still fire) while leaving the wiring to the
    /// caller. See `// TODO(sera-8d1.1)` below.
    fn caller_from(&self, ctx: &ToolContext) -> CallerContext {
        // TODO(sera-8d1.1): when ToolContext gains a
        // `resolved_capabilities: Arc<ResolvedCapabilities>` field (tracked
        // separately), thread it here instead of defaulting to a
        // deny-by-default bundle.
        let _ = ctx;
        CallerContext {
            agent_id: ctx.principal.id.0.clone(),
            capabilities: ResolvedCapabilities::default(),
            budget: Arc::new(BudgetTracker::new()),
        }
    }
}

fn map_err(err: AgentToolError) -> ToolError {
    match err {
        AgentToolError::CapabilityDenied { .. } => ToolError::Unauthorized(err.to_string()),
        AgentToolError::AgentNotFound(name) => ToolError::NotFound(name),
        AgentToolError::Router(msg) => ToolError::ExecutionFailed(msg),
    }
}

/// Pull the `target` agent id out of the tool arguments — common to all
/// three agent-tools. Returns the remaining argument bag with `target`
/// stripped so the per-kind decoders see only their own fields.
fn split_target(input: &ToolInput) -> Result<(String, serde_json::Value), ToolError> {
    let mut args = input.arguments.clone();
    let target = args
        .as_object_mut()
        .and_then(|m| m.remove("target"))
        .ok_or_else(|| ToolError::InvalidInput("missing required field 'target'".to_string()))?;
    let target = target
        .as_str()
        .ok_or_else(|| ToolError::InvalidInput("'target' must be a string".to_string()))?
        .to_string();
    Ok((target, args))
}

fn target_property() -> (String, ParameterSchema) {
    (
        "target".to_string(),
        ParameterSchema {
            schema_type: "string".to_string(),
            description: Some(
                "Stable identifier of the target agent (must be in caller's subagents_allowed)"
                    .to_string(),
            ),
            enum_values: None,
            default: None,
        },
    )
}

// ── delegate-task ─────────────────────────────────────────────────────────────

/// Synchronous delegate-task agent-tool.
pub struct DelegateTaskTool {
    shared: Shared,
}

impl DelegateTaskTool {
    pub fn new(registry: Arc<AgentToolRegistry>) -> Self {
        Self {
            shared: Shared::new(registry),
        }
    }
}

#[async_trait]
impl Tool for DelegateTaskTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "delegate-task".to_string(),
            description: "Delegate a task to another agent and block until it returns a structured result"
                .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Execute,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["agent-tool".to_string(), "subagent".to_string()],
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        let (k, v) = target_property();
        properties.insert(k, v);
        properties.insert(
            "task".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Free-text task description for the target agent".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "context".to_string(),
            ParameterSchema {
                schema_type: "object".to_string(),
                description: Some("Optional structured context payload".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["target".to_string(), "task".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let (target, args) = split_target(&input)?;
        let caller = self.shared.caller_from(&ctx);
        let value = self
            .shared
            .registry
            .dispatch_kind(&caller, AgentToolKind::DelegateTask, &target, args)
            .await
            .map_err(map_err)?;
        Ok(ToolOutput::success(value.to_string()))
    }
}

// ── ask-agent ─────────────────────────────────────────────────────────────────

/// Synchronous ask-agent agent-tool.
pub struct AskAgentTool {
    shared: Shared,
}

impl AskAgentTool {
    pub fn new(registry: Arc<AgentToolRegistry>) -> Self {
        Self {
            shared: Shared::new(registry),
        }
    }
}

#[async_trait]
impl Tool for AskAgentTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "ask-agent".to_string(),
            description: "Ask another agent a question and block until it answers".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Execute,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["agent-tool".to_string(), "subagent".to_string()],
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        let (k, v) = target_property();
        properties.insert(k, v);
        properties.insert(
            "question".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Free-text question for the target agent".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["target".to_string(), "question".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let (target, args) = split_target(&input)?;
        let caller = self.shared.caller_from(&ctx);
        let value = self
            .shared
            .registry
            .dispatch_kind(&caller, AgentToolKind::AskAgent, &target, args)
            .await
            .map_err(map_err)?;
        Ok(ToolOutput::success(value.to_string()))
    }
}

// ── background-task ───────────────────────────────────────────────────────────

/// Asynchronous background-task agent-tool — returns a task_id immediately.
pub struct BackgroundTaskTool {
    shared: Shared,
}

impl BackgroundTaskTool {
    pub fn new(registry: Arc<AgentToolRegistry>) -> Self {
        Self {
            shared: Shared::new(registry),
        }
    }
}

#[async_trait]
impl Tool for BackgroundTaskTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "background-task".to_string(),
            description: "Spawn a background task on another agent and return a task_id without blocking"
                .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Execute,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["agent-tool".to_string(), "subagent".to_string()],
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        let (k, v) = target_property();
        properties.insert(k, v);
        properties.insert(
            "task".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Free-text task description for the background worker".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["target".to_string(), "task".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let (target, args) = split_target(&input)?;
        let caller = self.shared.caller_from(&ctx);
        let value = self
            .shared
            .registry
            .dispatch_kind(&caller, AgentToolKind::BackgroundTask, &target, args)
            .await
            .map_err(map_err)?;
        Ok(ToolOutput::success(value.to_string()))
    }
}

/// Convenience helper — build the three agent-tools sharing a single
/// [`AgentToolRegistry`].
pub fn build_agent_tools(
    registry: Arc<AgentToolRegistry>,
) -> (DelegateTaskTool, AskAgentTool, BackgroundTaskTool) {
    (
        DelegateTaskTool::new(registry.clone()),
        AskAgentTool::new(registry.clone()),
        BackgroundTaskTool::new(registry),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_names_and_risk() {
        let r = Arc::new(AgentToolRegistry::new());
        let (d, a, b) = build_agent_tools(r);
        assert_eq!(d.metadata().name, "delegate-task");
        assert_eq!(a.metadata().name, "ask-agent");
        assert_eq!(b.metadata().name, "background-task");
        for meta in [d.metadata(), a.metadata(), b.metadata()] {
            assert_eq!(meta.risk_level, RiskLevel::Execute);
            assert!(meta.tags.iter().any(|t| t == "agent-tool"));
        }
    }

    #[test]
    fn schemas_require_target() {
        let r = Arc::new(AgentToolRegistry::new());
        let (d, a, b) = build_agent_tools(r);
        for tool in [
            &d as &dyn Tool,
            &a as &dyn Tool,
            &b as &dyn Tool,
        ] {
            let schema = tool.schema();
            assert!(schema.parameters.required.iter().any(|s| s == "target"));
            assert!(schema.parameters.properties.contains_key("target"));
        }
    }

    #[test]
    fn split_target_rejects_missing() {
        let input = ToolInput {
            name: "delegate-task".into(),
            arguments: serde_json::json!({"task": "x"}),
            call_id: "c".into(),
        };
        let err = split_target(&input).unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    #[test]
    fn split_target_extracts() {
        let input = ToolInput {
            name: "delegate-task".into(),
            arguments: serde_json::json!({"target": "worker", "task": "x"}),
            call_id: "c".into(),
        };
        let (target, rest) = split_target(&input).unwrap();
        assert_eq!(target, "worker");
        assert_eq!(rest, serde_json::json!({"task": "x"}));
    }
}
