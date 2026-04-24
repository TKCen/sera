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
//!
//! # Tool hooks and permission escalation (sera-ddz / GH#544, GH#545)
//!
//! `RegistryDispatcher` holds **additive** optional layers:
//!
//! - An `Option<Arc<ToolHookRegistry>>` consulted before/after every call.
//!   Pre-hook aborts surface as [`ToolError::AbortedByHook`]. Post hooks are
//!   observation-only.
//! - An `Option<Arc<dyn EscalationAuthority>>` + a caller-mode / per-tool-mode
//!   pair that lets the dispatcher gate calls on [`PermissionMode`]. When the
//!   caller's mode is lower than the tool's requirement, the dispatcher asks
//!   the authority; denial surfaces as [`ToolError::PermissionDenied`].
//!
//! Both layers default to `None` — existing call sites keep the current
//! behaviour until they explicitly opt in with the `with_*` builder methods.

use std::sync::Arc;

use async_trait::async_trait;
use sera_types::tool::{ToolContext, ToolInput, ToolOutput};

use crate::permissions::{
    EscalationAuthority, EscalationRequest, EscalationScope, PermissionMode,
};
use crate::tool_hooks::{ToolCallCtx, ToolHookOutcome, ToolHookRegistry};
use crate::tools::TraitToolRegistry;
use crate::turn::{ToolDispatcher, ToolError};

/// Concrete ToolDispatcher that delegates to the trait-based TraitToolRegistry.
pub struct RegistryDispatcher {
    registry: Arc<TraitToolRegistry>,
    /// Optional pre/post tool hook registry (GH#544).
    hooks: Option<Arc<ToolHookRegistry>>,
    /// Optional escalation authority (GH#545). When `Some`, the dispatcher
    /// checks `caller_mode >= tool_required_mode` per call and asks the
    /// authority to escalate on mismatch.
    escalation_authority: Option<Arc<dyn EscalationAuthority>>,
    /// Caller's current permission mode, used only when `escalation_authority`
    /// is set. Defaults to `PermissionMode::Standard`.
    caller_mode: PermissionMode,
    /// Required permission mode for the tools in `registry`, used only when
    /// `escalation_authority` is set. Defaults to `PermissionMode::Standard`
    /// (i.e. no gating). Production wiring should derive this per-tool from
    /// [`sera_types::tool::RiskLevel`]; see `required_mode_for_risk` below.
    default_tool_mode: PermissionMode,
}

impl RegistryDispatcher {
    /// Create a new dispatcher backed by the given registry.
    pub fn new(registry: Arc<TraitToolRegistry>) -> Self {
        Self {
            registry,
            hooks: None,
            escalation_authority: None,
            caller_mode: PermissionMode::Standard,
            default_tool_mode: PermissionMode::Standard,
        }
    }

    /// Attach a [`ToolHookRegistry`] to this dispatcher. Subsequent
    /// `dispatch` calls consult the registry before + after each tool
    /// execution. See `crate::tool_hooks` for the hook contract.
    pub fn with_hooks(mut self, hooks: Arc<ToolHookRegistry>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    /// Attach a [`EscalationAuthority`] and caller / required permission
    /// modes. When `caller_mode >= required_mode` the gate is a no-op; when
    /// it isn't, the dispatcher asks the authority and either runs with the
    /// granted mode or surfaces [`ToolError::PermissionDenied`].
    pub fn with_escalation(
        mut self,
        authority: Arc<dyn EscalationAuthority>,
        caller_mode: PermissionMode,
        required_mode: PermissionMode,
    ) -> Self {
        self.escalation_authority = Some(authority);
        self.caller_mode = caller_mode;
        self.default_tool_mode = required_mode;
        self
    }
}

/// Map a tool risk level to the permission mode required to call it.
///
/// Shared so tests + production gates agree: `Read → Standard`,
/// `Write | Execute → Elevated`, `Admin → Admin`.
pub fn required_mode_for_risk(risk: sera_types::tool::RiskLevel) -> PermissionMode {
    use sera_types::tool::RiskLevel;
    match risk {
        RiskLevel::Read => PermissionMode::Standard,
        RiskLevel::Write | RiskLevel::Execute => PermissionMode::Elevated,
        RiskLevel::Admin => PermissionMode::Admin,
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

        let mut input = ToolInput {
            name: name.to_string(),
            arguments,
            call_id: tool_call_id.to_string(),
        };

        // ── Permission gate (GH#545) ─────────────────────────────────────
        if let Some(authority) = self.escalation_authority.as_ref() {
            // Per-tool required mode takes precedence when the tool is
            // registered; otherwise fall back to the dispatcher default.
            let required_mode = self
                .registry
                .get(&input.name)
                .map(|t| required_mode_for_risk(t.metadata().risk_level))
                .unwrap_or(self.default_tool_mode);
            if !self.caller_mode.satisfies(required_mode) {
                let req = EscalationRequest {
                    from: self.caller_mode,
                    to: required_mode,
                    reason: format!("tool '{}' requires {:?}", input.name, required_mode),
                    scope: EscalationScope::SingleCall,
                };
                match authority.request_escalation(req).await {
                    Ok(decision) if decision.granted => {
                        // Grant acknowledged; execution continues.
                    }
                    Ok(decision) => {
                        return Err(ToolError::PermissionDenied {
                            reason: decision
                                .denial_reason
                                .unwrap_or_else(|| "escalation denied".to_string()),
                        });
                    }
                    Err(e) => {
                        return Err(ToolError::PermissionDenied {
                            reason: e.to_string(),
                        });
                    }
                }
            }
        }

        // ── Pre-tool hooks (GH#544) ──────────────────────────────────────
        if let Some(hooks) = self.hooks.as_ref() {
            let call_ctx = ToolCallCtx::new(&input, ctx);
            match hooks.pre_all(&call_ctx).await {
                ToolHookOutcome::Continue => {}
                ToolHookOutcome::MutateInput(new_input) => {
                    input = new_input;
                }
                ToolHookOutcome::Abort(reason) => {
                    return Err(ToolError::AbortedByHook { reason });
                }
            }
        }

        // ── Execute ──────────────────────────────────────────────────────
        let exec_result = self.registry.execute(input.clone(), ctx.clone()).await;

        // ── Post-tool hooks (GH#544) ─────────────────────────────────────
        if let Some(hooks) = self.hooks.as_ref() {
            // Build a ToolOutput-shaped view of the result (including the
            // error case) so hooks observe a uniform type.
            let observed: ToolOutput = match &exec_result {
                Ok(out) => out.clone(),
                Err(e) => ToolOutput::error(e.to_string()),
            };
            let call_ctx = ToolCallCtx::new(&input, ctx);
            let _ = hooks.post_all(&call_ctx, &observed).await;
        }

        match exec_result {
            Ok(output) => Ok(serde_json::json!({
                "tool_call_id": tool_call_id,
                "role": "tool",
                "content": output.content,
            })),
            Err(e) => Err(e),
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
        assert!(
            matches!(err, ToolError::PolicyDenied(_) | ToolError::ExecutionFailed(_)),
            "expected PolicyDenied or ExecutionFailed, got {err:?}",
        );
    }

    // ── sera-ddz: tool hook + permission escalation integration ─────────

    use crate::permissions::{
        EscalationAuthority, EscalationDecision, EscalationError, EscalationRequest,
        PermissionMode, StubEscalationAuthority,
    };
    use crate::tool_hooks::{ToolCallCtx, ToolHook, ToolHookOutcome, ToolHookRegistry};
    use async_trait::async_trait;
    use sera_types::tool::ToolOutput;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn file_list_call(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "type": "function",
            "function": {
                "name": "file-list",
                "arguments": "{\"path\":\"/tmp\"}"
            }
        })
    }

    // A pre-only hook that counts pre invocations.
    struct PreCounter(Arc<AtomicUsize>);

    #[async_trait]
    impl ToolHook for PreCounter {
        fn id(&self) -> &str {
            "pre-counter"
        }
        async fn pre(&self, _ctx: &ToolCallCtx<'_>) -> ToolHookOutcome {
            self.0.fetch_add(1, Ordering::SeqCst);
            ToolHookOutcome::Continue
        }
    }

    // A pre hook that aborts when the tool name matches.
    struct AbortWhenNamed(&'static str);

    #[async_trait]
    impl ToolHook for AbortWhenNamed {
        fn id(&self) -> &str {
            "abort-when-named"
        }
        async fn pre(&self, ctx: &ToolCallCtx<'_>) -> ToolHookOutcome {
            if ctx.input.name == self.0 {
                ToolHookOutcome::Abort(format!("{} blocked", self.0))
            } else {
                ToolHookOutcome::Continue
            }
        }
    }

    #[tokio::test]
    async fn dispatcher_runs_pre_hook_for_every_call() {
        let counter = Arc::new(AtomicUsize::new(0));
        let hooks = Arc::new(ToolHookRegistry::new());
        hooks.register(Arc::new(PreCounter(counter.clone()))).await;

        let dispatcher = RegistryDispatcher::new(Arc::new(TraitToolRegistry::with_builtins()))
            .with_hooks(hooks);

        for i in 0..3 {
            let call = file_list_call(&format!("call-{i}"));
            let _ = dispatcher
                .dispatch(&call, &ToolContext::default())
                .await
                .expect("dispatch succeeds");
        }
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn dispatcher_surfaces_aborted_by_hook() {
        let hooks = Arc::new(ToolHookRegistry::new());
        hooks.register(Arc::new(AbortWhenNamed("file-list"))).await;

        let dispatcher = RegistryDispatcher::new(Arc::new(TraitToolRegistry::with_builtins()))
            .with_hooks(hooks);

        let call = file_list_call("call-abort-1");
        let err = dispatcher
            .dispatch(&call, &ToolContext::default())
            .await
            .unwrap_err();
        match err {
            ToolError::AbortedByHook { reason } => assert!(reason.contains("file-list")),
            other => panic!("expected AbortedByHook, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatcher_post_hook_does_not_change_result() {
        // Observation-only: a post hook that says Abort must not prevent the
        // caller from seeing the original successful result.
        struct PostAbort;
        #[async_trait]
        impl ToolHook for PostAbort {
            fn id(&self) -> &str {
                "post-abort"
            }
            async fn pre(&self, _ctx: &ToolCallCtx<'_>) -> ToolHookOutcome {
                ToolHookOutcome::Continue
            }
            async fn post(
                &self,
                _ctx: &ToolCallCtx<'_>,
                _result: &ToolOutput,
            ) -> ToolHookOutcome {
                ToolHookOutcome::Abort("observational".to_string())
            }
        }

        let hooks = Arc::new(ToolHookRegistry::new());
        hooks.register(Arc::new(PostAbort)).await;
        let dispatcher = RegistryDispatcher::new(Arc::new(TraitToolRegistry::with_builtins()))
            .with_hooks(hooks);

        let call = file_list_call("call-post-1");
        let result = dispatcher
            .dispatch(&call, &ToolContext::default())
            .await
            .expect("dispatch succeeds despite post-hook abort");
        assert_eq!(result["tool_call_id"], "call-post-1");
    }

    // ── Permission gate ─────────────────────────────────────────────────

    /// Authority that records the request it saw and always denies.
    #[derive(Default)]
    struct RecordingDenyAuthority {
        seen: std::sync::Mutex<Option<EscalationRequest>>,
    }

    #[async_trait]
    impl EscalationAuthority for RecordingDenyAuthority {
        async fn request_escalation(
            &self,
            req: EscalationRequest,
        ) -> Result<EscalationDecision, EscalationError> {
            *self.seen.lock().unwrap() = Some(req);
            Ok(EscalationDecision::deny("test-denied"))
        }
    }

    #[tokio::test]
    async fn dispatcher_permission_gate_denies_when_authority_denies() {
        // file-list is Read → required = Standard. Set the caller to
        // Standard and the *default* mode high (Admin). The gate will fall
        // back to the per-tool mode lookup first, so use a mode the built-in
        // does not satisfy by targeting `shell-exec` (Execute → Elevated).
        let authority = Arc::new(RecordingDenyAuthority::default());
        let dispatcher = RegistryDispatcher::new(Arc::new(TraitToolRegistry::with_builtins()))
            .with_escalation(
                authority.clone(),
                PermissionMode::Standard,
                PermissionMode::Standard,
            );

        let call = serde_json::json!({
            "id": "call-deny-1",
            "type": "function",
            "function": {
                "name": "shell-exec",
                "arguments": "{\"command\":\"echo hi\"}"
            }
        });
        let err = dispatcher
            .dispatch(&call, &ToolContext::default())
            .await
            .unwrap_err();
        match err {
            ToolError::PermissionDenied { reason } => {
                assert_eq!(reason, "test-denied");
            }
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
        // Authority was consulted with the right modes.
        let seen = authority.seen.lock().unwrap().clone().unwrap();
        assert_eq!(seen.from, PermissionMode::Standard);
        assert_eq!(seen.to, PermissionMode::Elevated);
    }

    #[tokio::test]
    async fn dispatcher_permission_gate_grants_via_stub_authority() {
        // Stub auto-approves — the call should succeed despite the
        // insufficient caller_mode.
        let dispatcher = RegistryDispatcher::new(Arc::new(TraitToolRegistry::with_builtins()))
            .with_escalation(
                Arc::new(StubEscalationAuthority),
                PermissionMode::Standard,
                PermissionMode::Standard,
            );

        let call = file_list_call("call-stub-1");
        let result = dispatcher
            .dispatch(&call, &ToolContext::default())
            .await
            .expect("stub grants + dispatch succeeds");
        assert_eq!(result["tool_call_id"], "call-stub-1");
    }

    #[tokio::test]
    async fn dispatcher_no_gate_when_mode_sufficient() {
        // Admin caller satisfies every built-in. Authority is recording-deny;
        // it must NOT be consulted because the gate is satisfied locally.
        let authority = Arc::new(RecordingDenyAuthority::default());
        let dispatcher = RegistryDispatcher::new(Arc::new(TraitToolRegistry::with_builtins()))
            .with_escalation(
                authority.clone(),
                PermissionMode::Admin,
                PermissionMode::Standard,
            );

        let call = file_list_call("call-admin-1");
        let result = dispatcher
            .dispatch(&call, &ToolContext::default())
            .await
            .expect("Admin mode satisfies every built-in");
        assert_eq!(result["tool_call_id"], "call-admin-1");
        assert!(
            authority.seen.lock().unwrap().is_none(),
            "authority must not be consulted when caller satisfies required mode"
        );
    }
}
