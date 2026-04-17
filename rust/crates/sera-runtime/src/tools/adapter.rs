//! Adapter that wraps the legacy [`ToolExecutor`] trait into the spec-aligned
//! [`Tool`] trait so existing tool implementations can be registered in
//! [`TraitToolRegistry`] without rewriting every tool.
//!
//! This is the migration bridge for sera-ttrm-2. Bead 5 (sera-cdan) replaces
//! selected adapter wrappers with direct `Tool` impls and refines per-tool
//! risk levels; until then every tool flows through `ToolExecutorAdapter`
//! with the conservative `RiskLevel::Execute` default.
//!
//! The adapter is deliberately thin:
//! - `Tool::metadata` is built from `ToolExecutor::{name, description}` with
//!   conservative defaults (`RiskLevel::Execute`, `ExecutionTarget::InProcess`).
//! - `Tool::schema` deserializes `ToolExecutor::parameters()` (a JSON value
//!   shaped like `{type, properties, required}`) into `FunctionParameters`.
//! - `Tool::execute` forwards to `ToolExecutor::execute`, discarding the
//!   `ToolContext`. Bead 4 (sera-sebr) threads `ctx` into authz checks.
//!
//! # Example
//!
//! ```ignore
//! use sera_runtime::tools::{ToolExecutorAdapter, TraitToolRegistry};
//! use sera_runtime::tools::file_ops::FileRead;
//!
//! let mut registry = TraitToolRegistry::new();
//! registry.register(Box::new(ToolExecutorAdapter::new(FileRead)));
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, RiskLevel, Tool, ToolContext, ToolError, ToolInput,
    ToolMetadata, ToolOutput, ToolSchema,
};

use super::ToolExecutor;

/// Adapter version reported in `ToolMetadata::version` for every wrapped
/// executor. Per-tool versions land with the direct `Tool` impls in bead 5.
const ADAPTER_VERSION: &str = "0.0.0-adapter";

/// Wraps any [`ToolExecutor`] implementation as a spec-aligned [`Tool`].
///
/// See the module-level docs for the defaults applied (risk level, execution
/// target) and the ctx-forwarding strategy.
pub struct ToolExecutorAdapter<T: ToolExecutor> {
    inner: Arc<T>,
}

impl<T: ToolExecutor> ToolExecutorAdapter<T> {
    /// Wrap `inner` as a `Tool` trait object.
    pub fn new(inner: T) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }
}

#[async_trait]
impl<T> Tool for ToolExecutorAdapter<T>
where
    T: ToolExecutor + 'static,
{
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: self.inner.name().to_string(),
            description: self.inner.description().to_string(),
            version: ADAPTER_VERSION.to_string(),
            author: None,
            // Conservative default — bead 5 (sera-cdan) refines per-tool.
            risk_level: RiskLevel::Execute,
            execution_target: ExecutionTarget::InProcess,
            tags: vec![],
        }
    }

    fn schema(&self) -> ToolSchema {
        let raw = self.inner.parameters();
        // `ToolExecutor::parameters()` returns a JSON value matching the
        // `FunctionParameters` shape (`{type, properties, required}`). If a
        // legacy tool ever returns something else, fall back to an empty
        // object schema rather than panicking — the LLM will still see the
        // tool name and description.
        let parameters = serde_json::from_value::<FunctionParameters>(raw).unwrap_or_else(|_| {
            FunctionParameters {
                schema_type: "object".to_string(),
                properties: Default::default(),
                required: vec![],
            }
        });
        ToolSchema { parameters }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        // bead 4 (sera-sebr) wires ctx.authz here.
        match self.inner.execute(&input.arguments).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(err) => Err(ToolError::ExecutionFailed(err.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolExecutor;

    // ── Trivial in-test executor that asserts `execute()` gets called with
    // the exact arguments it received. Used by `adapter_forwards_execute`.
    struct RecordingExecutor;

    #[async_trait]
    impl ToolExecutor for RecordingExecutor {
        fn name(&self) -> &str {
            "test-recorder"
        }
        fn description(&self) -> &str {
            "records the args it was called with"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "msg": {"type": "string", "description": "input message"}
                },
                "required": ["msg"]
            })
        }
        async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
            let msg = args["msg"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing msg"))?;
            Ok(format!("recorded: {msg}"))
        }
    }

    struct FailingExecutor;

    #[async_trait]
    impl ToolExecutor for FailingExecutor {
        fn name(&self) -> &str {
            "failer"
        }
        fn description(&self) -> &str {
            "always fails"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _args: &serde_json::Value) -> anyhow::Result<String> {
            Err(anyhow::anyhow!("intentional failure"))
        }
    }

    #[test]
    fn adapter_metadata_maps_name_and_description() {
        let adapter = ToolExecutorAdapter::new(RecordingExecutor);
        let meta = adapter.metadata();
        assert_eq!(meta.name, "test-recorder");
        assert_eq!(meta.description, "records the args it was called with");
        assert_eq!(meta.risk_level, RiskLevel::Execute);
        assert_eq!(meta.execution_target, ExecutionTarget::InProcess);
    }

    #[test]
    fn adapter_schema_deserializes_parameters() {
        let adapter = ToolExecutorAdapter::new(RecordingExecutor);
        let schema = adapter.schema();
        assert_eq!(schema.parameters.schema_type, "object");
        assert!(schema.parameters.properties.contains_key("msg"));
        assert_eq!(schema.parameters.required, vec!["msg".to_string()]);
    }

    #[tokio::test]
    async fn adapter_forwards_execute() {
        let adapter = ToolExecutorAdapter::new(RecordingExecutor);
        let input = ToolInput {
            name: "test-recorder".to_string(),
            arguments: serde_json::json!({"msg": "hello"}),
            call_id: "call-1".to_string(),
        };
        let out = adapter.execute(input, ToolContext::default()).await.unwrap();
        assert!(!out.is_error);
        assert_eq!(out.content, "recorded: hello");
    }

    #[tokio::test]
    async fn adapter_maps_executor_error_to_tool_error() {
        let adapter = ToolExecutorAdapter::new(FailingExecutor);
        let input = ToolInput {
            name: "failer".to_string(),
            arguments: serde_json::json!({}),
            call_id: "call-2".to_string(),
        };
        let err = adapter.execute(input, ToolContext::default()).await.unwrap_err();
        match err {
            ToolError::ExecutionFailed(msg) => assert!(msg.contains("intentional failure")),
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn adapter_roundtrips_tool_definition() {
        use crate::tools::file_ops::FileRead;

        let adapter = ToolExecutorAdapter::new(FileRead);
        let meta = adapter.metadata();
        assert_eq!(meta.name, "file-read");

        let schema = adapter.schema();
        assert_eq!(schema.parameters.schema_type, "object");
        assert!(schema.parameters.properties.contains_key("path"));
        assert_eq!(schema.parameters.required, vec!["path".to_string()]);

        // The OpenAI-compatible definition shape produced through the
        // TraitToolRegistry path should match what FileRead exports directly.
        let mut registry = crate::tools::TraitToolRegistry::new();
        registry.register(Box::new(adapter));
        let defs = registry.definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].tool_type, "function");
        assert_eq!(defs[0].function.name, "file-read");
        assert_eq!(defs[0].function.description, "Read the contents of a file");

        // The parameters JSON produced by the adapter must match what FileRead
        // emits via its ToolExecutor::parameters() surface.
        let raw_expected = FileRead.parameters();
        let raw_actual = defs[0].function.parameters.clone();
        assert_eq!(raw_actual, raw_expected);
    }
}
