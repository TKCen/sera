//! Bridge from the ToolDispatcher trait to TraitToolRegistry.
//!
//! Translates OpenAI-format tool_call JSON into `ToolInput` structs and
//! delegates to `TraitToolRegistry::execute`, which enforces `ToolPolicy`
//! before calling through to the underlying `Tool` impl.
//!
//! # Policy enforcement
//!
//! `RegistryDispatcher` now uses `TraitToolRegistry` (trait-based, full policy
//! enforcement via `ToolContext` + `ToolPolicy`). The `ToolContext` passed to
//! `dispatch` is forwarded directly to `TraitToolRegistry::execute`.

use std::sync::Arc;

use async_trait::async_trait;
use sera_types::tool::{ToolContext, ToolInput};

use crate::tools::TraitToolRegistry;
use crate::turn::{ToolDispatcher, ToolError};

/// Concrete ToolDispatcher that delegates to the trait-based TraitToolRegistry.
pub struct RegistryDispatcher {
    registry: Arc<TraitToolRegistry>,
}

impl RegistryDispatcher {
    /// Create a new dispatcher backed by the given registry.
    pub fn new(registry: Arc<TraitToolRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl ToolDispatcher for RegistryDispatcher {
    /// Dispatch a tool call in OpenAI format to the registry.
    ///
    /// Expected input format:
    /// ```json
    /// {"id": "call_xxx", "type": "function", "function": {"name": "...", "arguments": "..."}}
    /// ```
    ///
    /// Returns:
    /// ```json
    /// {"tool_call_id": "call_xxx", "role": "tool", "content": "..."}
    /// ```
    async fn dispatch(
        &self,
        tool_call: &serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        // Extract tool_call_id
        let tool_call_id = tool_call
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        // Extract function name
        let function = tool_call
            .get("function")
            .ok_or_else(|| ToolError::InvalidArguments("missing 'function' field".to_string()))?;

        let name = function
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("missing 'function.name' field".to_string()))?;

        // Extract and parse arguments (arguments is a JSON string, not an object)
        let args_str = function
            .get("arguments")
            .and_then(|v| v.as_str())
            .unwrap_or("{}");

        let arguments: serde_json::Value = serde_json::from_str(args_str)
            .map_err(|e| ToolError::InvalidArguments(format!("failed to parse arguments: {e}")))?;

        let input = ToolInput {
            name: name.to_string(),
            arguments,
            call_id: tool_call_id.to_string(),
        };

        match self.registry.execute(input, ctx.clone()).await {
            Ok(output) => Ok(serde_json::json!({
                "tool_call_id": tool_call_id,
                "role": "tool",
                "content": output.content,
            })),
            Err(sera_types::tool::ToolError::NotFound(n)) => Err(ToolError::NotFound(n)),
            Err(sera_types::tool::ToolError::PolicyDenied(n)) => {
                Err(ToolError::ExecutionFailed(format!("policy denied: {n}")))
            }
            Err(e) => Err(ToolError::ExecutionFailed(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::TraitToolRegistry;
    use sera_types::tool::ToolPolicy;

    fn make_dispatcher() -> RegistryDispatcher {
        RegistryDispatcher::new(Arc::new(TraitToolRegistry::with_builtins()))
    }

    #[tokio::test]
    async fn dispatch_valid_tool_call() {
        let dispatcher = make_dispatcher();
        // Use file-list on a known directory
        let tool_call = serde_json::json!({
            "id": "call_1",
            "type": "function",
            "function": {
                "name": "file-list",
                "arguments": "{\"path\":\"/tmp\"}"
            }
        });
        let result = dispatcher.dispatch(&tool_call, &ToolContext::default()).await.unwrap();
        assert_eq!(result["tool_call_id"], "call_1");
        assert_eq!(result["role"], "tool");
        assert!(result["content"].is_string());
    }

    #[tokio::test]
    async fn dispatch_unknown_tool() {
        let dispatcher = make_dispatcher();
        let tool_call = serde_json::json!({
            "id": "call_2",
            "type": "function",
            "function": {
                "name": "nonexistent-tool",
                "arguments": "{}"
            }
        });
        let err = dispatcher.dispatch(&tool_call, &ToolContext::default()).await.unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn dispatch_malformed_arguments() {
        let dispatcher = make_dispatcher();
        let tool_call = serde_json::json!({
            "id": "call_3",
            "type": "function",
            "function": {
                "name": "file-read",
                "arguments": "not valid json{{"
            }
        });
        let err = dispatcher.dispatch(&tool_call, &ToolContext::default()).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }

    #[tokio::test]
    async fn dispatch_missing_function_name() {
        let dispatcher = make_dispatcher();
        let tool_call = serde_json::json!({
            "id": "call_4",
            "type": "function"
        });
        let err = dispatcher.dispatch(&tool_call, &ToolContext::default()).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }

    #[tokio::test]
    async fn dispatch_missing_function_field() {
        let dispatcher = make_dispatcher();
        let tool_call = serde_json::json!({
            "id": "call_5"
        });
        let err = dispatcher.dispatch(&tool_call, &ToolContext::default()).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }

    #[test]
    fn registry_get_works() {
        let registry = TraitToolRegistry::with_builtins();
        assert!(registry.get("shell-exec").is_some());
        assert!(registry.get("file-read").is_some());
        assert!(registry.get("file-write").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    /// Validate that all tool definitions survive the serde round-trip from
    /// crate::types::ToolDefinition → serde_json::Value → sera_types::tool::ToolDefinition.
    /// This catches schema incompatibilities between the two ToolDefinition types.
    #[test]
    fn all_tool_definitions_round_trip() -> Result<(), String> {
        let registry = TraitToolRegistry::with_builtins();
        let defs = registry.definitions();
        assert!(!defs.is_empty(), "registry should have tools");

        for def in &defs {
            let value = serde_json::to_value(def)
                .map_err(|e| format!("failed to serialize tool '{}': {e}", def.function.name))?;
            let _typed: sera_types::tool::ToolDefinition = serde_json::from_value(value)
                .map_err(|e| format!("failed to round-trip tool '{}': {e}", def.function.name))?;
        }
        Ok(())
    }

    #[tokio::test]
    #[allow(clippy::field_reassign_with_default)]
    async fn dispatch_policy_denied() {
        let dispatcher = make_dispatcher();
        let tool_call = serde_json::json!({
            "id": "call_6",
            "type": "function",
            "function": {
                "name": "file-list",
                "arguments": "{\"path\":\"/tmp\"}"
            }
        });
        // Build a ctx that denies everything
        let mut ctx = ToolContext::default();
        ctx.policy = ToolPolicy {
            profile: None,
            allow_patterns: vec![],
            deny_patterns: vec!["*".to_string()],
        };
        let err = dispatcher.dispatch(&tool_call, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed(_)));
    }
}
