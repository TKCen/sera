//! Bridge from the ToolDispatcher trait to the existing ToolRegistry.
//!
//! Translates OpenAI-format tool_call JSON into ToolRegistry::execute() calls
//! and formats the results back into tool result messages.
//!
//! # Policy enforcement — current state
//!
//! `RegistryDispatcher` uses `ToolRegistry` (executor-based, no per-call policy).
//! `TraitToolRegistry` (same module) provides full policy enforcement via
//! `ToolContext` + `ToolPolicy`, but migrating to it requires:
//!
//! 1. **Trait signature change** — `ToolDispatcher::dispatch` must accept a
//!    `ToolContext` parameter so that `TraitToolRegistry::execute(input, ctx)` can
//!    be called.  That propagates through `turn.rs`, `default_runtime.rs`, and
//!    every downstream caller.
//!
//! 2. **Tool re-implementation** — all 14+ `ToolExecutor` impls must be wrapped
//!    or re-written as `Tool`-trait impls so they can be registered in
//!    `TraitToolRegistry`.
//!
//! Track this as a dedicated bead:
//! > "Thread `ToolContext` through `ToolDispatcher::dispatch`, update all
//! > callers in `default_runtime.rs`, and wrap existing `ToolExecutor` impls
//! > as `Tool`-trait adapters so `RegistryDispatcher` can delegate to
//! > `TraitToolRegistry::execute`.  Tests must verify `ToolPolicy::allows`
//! > rejects denied tools and passes allowed ones."

use std::sync::Arc;

use async_trait::async_trait;

use crate::tools::ToolRegistry;
use crate::turn::{ToolDispatcher, ToolError};

/// Concrete ToolDispatcher that delegates to the ToolExecutor-based ToolRegistry.
pub struct RegistryDispatcher {
    registry: Arc<ToolRegistry>,
}

impl RegistryDispatcher {
    /// Create a new dispatcher backed by the given registry.
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
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
    async fn dispatch(&self, tool_call: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
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

        // Check tool exists before attempting execution
        if self.registry.get(name).is_none() {
            return Err(ToolError::NotFound(name.to_string()));
        }

        // Extract and parse arguments (arguments is a JSON string, not an object)
        let args_str = function
            .get("arguments")
            .and_then(|v| v.as_str())
            .unwrap_or("{}");

        let args: serde_json::Value = serde_json::from_str(args_str)
            .map_err(|e| ToolError::InvalidArguments(format!("failed to parse arguments: {e}")))?;

        // Execute via registry
        match self.registry.execute(name, &args).await {
            Ok(content) => Ok(serde_json::json!({
                "tool_call_id": tool_call_id,
                "role": "tool",
                "content": content,
            })),
            Err(e) => Err(ToolError::ExecutionFailed(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dispatcher() -> RegistryDispatcher {
        RegistryDispatcher::new(Arc::new(ToolRegistry::new()))
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
        let result = dispatcher.dispatch(&tool_call).await.unwrap();
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
        let err = dispatcher.dispatch(&tool_call).await.unwrap_err();
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
        let err = dispatcher.dispatch(&tool_call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }

    #[tokio::test]
    async fn dispatch_missing_function_name() {
        let dispatcher = make_dispatcher();
        let tool_call = serde_json::json!({
            "id": "call_4",
            "type": "function"
        });
        let err = dispatcher.dispatch(&tool_call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }

    #[tokio::test]
    async fn dispatch_missing_function_field() {
        let dispatcher = make_dispatcher();
        let tool_call = serde_json::json!({
            "id": "call_5"
        });
        let err = dispatcher.dispatch(&tool_call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }

    #[test]
    fn registry_get_works() {
        let registry = ToolRegistry::new();
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
        let registry = ToolRegistry::new();
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
}
