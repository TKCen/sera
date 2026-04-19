//! Tool registry and dispatcher for the SERA runtime.
//!
//! Every production tool implements the spec-aligned [`sera_types::tool::Tool`]
//! trait directly (bead sera-ttrm-5). The adapter-first migration chain
//! (beads ilk2 → 26me → h7dn → sebr → ttrm-5) is complete: the legacy
//! `ToolExecutor` trait and `ToolExecutorAdapter` have been removed entirely.
//! New tools should implement `Tool` directly and be registered via
//! [`TraitToolRegistry::register`].

pub mod agent_tools;
pub mod centrifugo;
pub mod correction_hook;
pub mod delegation;
pub mod dispatcher;
pub mod file_edit;
pub mod file_ops;
pub mod file_write;
pub mod glob;
pub mod grep;
pub mod http_request;
pub mod knowledge;
pub mod memory_search;
pub mod mvs_tools;
pub mod propose_correction;
pub mod shell_exec;
pub mod spawn;
pub mod tool_search;
pub mod web_fetch;

// ── Trait-based registry (SPEC-tools aligned) ────────────────────────────────

use std::collections::HashMap;
use sera_types::tool::{Tool, ToolContext, ToolError, ToolInput, ToolMetadata, ToolOutput};

/// Spec-aligned tool registry using the Tool trait.
pub struct TraitToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    /// When `true`, a PDP check via `ToolContext::authz` is performed in
    /// `execute` before the `ToolPolicy` check. Defaults to `false`.
    /// Controlled by `RuntimeConfig::tool_authz_enabled`.
    tool_authz_enabled: bool,
}

impl TraitToolRegistry {
    /// Create an empty registry with authz enforcement disabled.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            tool_authz_enabled: false,
        }
    }

    /// Create an empty registry with the given authz enforcement flag.
    pub fn new_with_authz(tool_authz_enabled: bool) -> Self {
        Self {
            tools: HashMap::new(),
            tool_authz_enabled,
        }
    }

    /// Populate the registry with the 14 built-in production tools — each
    /// registered as a native [`Tool`] impl with a per-tool [`sera_types::tool::RiskLevel`]:
    /// - `Read`: file-read, file-list, glob, grep, knowledge-query, web-fetch,
    ///   tool-search, skill-search
    /// - `Write`: file-write, file-edit, knowledge-store
    /// - `Execute`: shell-exec, http-request, spawn-ephemeral
    ///
    /// Note: `memory_search` is NOT included here — it needs an embedding
    /// service + semantic store injected via [`Self::with_memory_search`].
    /// `centrifugo::CentrifugoPublisher` is a publisher helper used
    /// elsewhere in the runtime and is not registered here.
    fn register_builtins(&mut self) {
        self.register(Box::new(file_ops::FileRead));
        self.register(Box::new(file_ops::FileWrite));
        self.register(Box::new(file_ops::FileList));
        self.register(Box::new(file_edit::FileEdit));
        self.register(Box::new(shell_exec::ShellExec));
        self.register(Box::new(http_request::HttpRequest));
        self.register(Box::new(knowledge::KnowledgeStore));
        self.register(Box::new(knowledge::KnowledgeQuery));
        self.register(Box::new(web_fetch::WebFetch));
        self.register(Box::new(glob::Glob));
        self.register(Box::new(grep::Grep));
        self.register(Box::new(spawn::SpawnEphemeral));
        self.register(Box::new(tool_search::ToolSearch));
        self.register(Box::new(tool_search::SkillSearch));
    }

    /// Create a registry populated with the 14 built-in tools (authz
    /// disabled).
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register_builtins();
        registry
    }

    /// Create a registry populated with the 14 built-in tools and the
    /// supplied authz kill-switch flag.
    pub fn with_builtins_and_authz(tool_authz_enabled: bool) -> Self {
        let mut registry = Self::new_with_authz(tool_authz_enabled);
        registry.register_builtins();
        registry
    }

    /// Register a tool by its metadata name.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.metadata().name.clone();
        self.tools.insert(name, tool);
    }

    /// Register the [`memory_search::MemorySearchTool`] built on top of
    /// the given embedding service + semantic store.
    ///
    /// Separate from [`Self::with_builtins`] because the tool needs
    /// runtime-supplied dependencies (`Arc<dyn EmbeddingService>` and
    /// `Arc<dyn SemanticMemoryStore>`). Call this after
    /// `with_builtins()` when the runtime has decided which backends to
    /// wire in. See bead sera-tier2-d for the full Tier-2 story.
    pub fn with_memory_search(
        mut self,
        embedding: std::sync::Arc<dyn sera_types::EmbeddingService>,
        store: std::sync::Arc<dyn sera_types::SemanticMemoryStore>,
    ) -> Self {
        self.register(Box::new(memory_search::MemorySearchTool::new(
            embedding, store,
        )));
        self
    }

    /// Register the three delegation tools (`session_spawn`, `session_yield`,
    /// `session_send`) bound to the supplied [`crate::delegation_bus::DelegationBus`].
    ///
    /// Separate from [`Self::with_builtins`] because these tools need a
    /// runtime-supplied bus instance so spawner/yielder/sender share the same
    /// subscriber registry. See bead sera-a1u.
    pub fn with_delegation(mut self, bus: crate::delegation_bus::DelegationBus) -> Self {
        let (spawn_tool, yield_tool, send_tool) =
            delegation::build_delegation_tools(bus);
        self.register(Box::new(spawn_tool));
        self.register(Box::new(yield_tool));
        self.register(Box::new(send_tool));
        self
    }

    /// Register the `propose-correction` meta-tool bound to a shared
    /// correction catalog. The agent uses it to submit new anti-pattern rules
    /// (written to `proposed/`, never auto-promoted). Pair this with the
    /// corresponding [`crate::tools::correction_hook::CorrectionHook`] to get
    /// the full tool-layer reinforcement loop.
    pub fn with_corrections(
        mut self,
        catalog: std::sync::Arc<sera_tools::corrections::CorrectionCatalog>,
    ) -> Self {
        self.register(Box::new(propose_correction::ProposeCorrection::new(catalog)));
        self
    }

    /// Register the three agent-as-tool entries (`delegate-task`,
    /// `ask-agent`, `background-task`) bound to the supplied
    /// [`crate::agent_tool_registry::AgentToolRegistry`].
    ///
    /// Separate from [`Self::with_builtins`] because the registry holds a
    /// pluggable [`crate::agent_tool_registry::AgentRouter`] and an optional
    /// coordinator hook that the embedder must provide. See bead
    /// `sera-8d1.1` (GH#144).
    pub fn with_agent_tools(
        mut self,
        registry: std::sync::Arc<crate::agent_tool_registry::AgentToolRegistry>,
    ) -> Self {
        let (delegate, ask, background) = agent_tools::build_agent_tools(registry);
        self.register(Box::new(delegate));
        self.register(Box::new(ask));
        self.register(Box::new(background));
        self
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

    /// Execute a tool by name.
    ///
    /// Check order:
    /// 1. If `tool_authz_enabled`, run a PDP check via `ctx.authz`. Denial
    ///    surfaces as [`ToolError::Unauthorized`].
    /// 2. Check `ctx.policy`. Denial surfaces as [`ToolError::PolicyDenied`].
    /// 3. Dispatch to the concrete tool impl.
    pub async fn execute(
        &self,
        input: ToolInput,
        ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        // Step 1 — per-tool PDP check (kill-switch gated).
        if self.tool_authz_enabled {
            use sera_types::tool::AuthzDecisionKind;
            let decision = ctx.authz.check(&ctx.principal, &input.name, &input.name).await;
            match decision {
                AuthzDecisionKind::Allow => {}
                AuthzDecisionKind::Deny(reason) => {
                    return Err(ToolError::Unauthorized(reason));
                }
                AuthzDecisionKind::NeedsApproval(hint) => {
                    return Err(ToolError::Unauthorized(format!(
                        "needs_approval:{hint}"
                    )));
                }
            }
        }

        // Step 2 — policy check.
        if !ctx.policy.allows(&input.name) {
            return Err(ToolError::PolicyDenied(input.name.clone()));
        }

        // Step 3 — dispatch.
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
            ..ToolContext::default()
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

    // ── Authz tests ───────────────────────────────────────────────────────

    /// A deny-all authz stub that rejects every check.
    #[derive(Debug)]
    struct DenyAllAuthz;

    #[async_trait::async_trait]
    impl sera_types::tool::AuthzProviderHandle for DenyAllAuthz {
        async fn check(
            &self,
            _principal: &sera_types::principal::PrincipalRef,
            _action: &str,
            _resource: &str,
        ) -> sera_types::tool::AuthzDecisionKind {
            sera_types::tool::AuthzDecisionKind::Deny("test_deny".to_string())
        }
    }

    /// When `tool_authz_enabled = true` and the authz provider denies, execute
    /// must return `ToolError::Unauthorized`.
    #[tokio::test]
    async fn denied_tool_returns_unauthorized() {
        let mut registry = TraitToolRegistry::new_with_authz(true);
        registry.register(Box::new(EchoTool));

        let input = ToolInput {
            name: "echo".to_string(),
            arguments: serde_json::json!({}),
            call_id: "call-authz-1".to_string(),
        };
        let ctx = ToolContext {
            authz: std::sync::Arc::new(DenyAllAuthz),
            ..make_ctx(ToolPolicy::from_profile(ToolProfile::Full))
        };
        let err = registry.execute(input, ctx).await.unwrap_err();
        assert!(
            matches!(err, ToolError::Unauthorized(_)),
            "expected Unauthorized, got {err:?}"
        );
        if let ToolError::Unauthorized(reason) = err {
            assert_eq!(reason, "test_deny");
        }
    }

    /// When `tool_authz_enabled = false` (kill-switch off), a deny-all authz
    /// provider must NOT prevent execution — the tool succeeds.
    #[tokio::test]
    async fn authz_killswitch_disabled_allows_execution() {
        let mut registry = TraitToolRegistry::new_with_authz(false);
        registry.register(Box::new(EchoTool));

        let input = ToolInput {
            name: "echo".to_string(),
            arguments: serde_json::json!({"msg": "hi"}),
            call_id: "call-authz-2".to_string(),
        };
        let ctx = ToolContext {
            authz: std::sync::Arc::new(DenyAllAuthz),
            ..make_ctx(ToolPolicy::from_profile(ToolProfile::Full))
        };
        // With kill-switch off, deny-all authz is bypassed → tool succeeds.
        let output = registry.execute(input, ctx).await.unwrap();
        assert!(!output.is_error);
    }

    #[test]
    fn with_builtins_registers_14() {
        let registry = TraitToolRegistry::with_builtins();
        let list = registry.list();
        assert_eq!(
            list.len(),
            14,
            "with_builtins() should register exactly 14 tools; got {}: {:?}",
            list.len(),
            list.iter().map(|m| &m.name).collect::<Vec<_>>()
        );

        // Every built-in tool must be reachable by its registered name.
        let expected = [
            "file-read",
            "file-write",
            "file-list",
            "file-edit",
            "shell-exec",
            "http-request",
            "knowledge-store",
            "knowledge-query",
            "web-fetch",
            "glob",
            "grep",
            "spawn-ephemeral",
            "tool-search",
            "skill-search",
        ];
        for name in expected {
            assert!(
                registry.get(name).is_some(),
                "built-in tool '{name}' not found in TraitToolRegistry::with_builtins()"
            );
        }
    }

    /// Per-tool risk level assignments (bead sera-ttrm-5). This test is the
    /// contract: if someone changes a built-in's risk class, they must
    /// update this assertion AND the policy routing that depends on it.
    #[test]
    fn with_builtins_risk_levels_are_correct() {
        let registry = TraitToolRegistry::with_builtins();

        let expected: &[(&str, RiskLevel)] = &[
            ("file-read", RiskLevel::Read),
            ("file-list", RiskLevel::Read),
            ("glob", RiskLevel::Read),
            ("grep", RiskLevel::Read),
            ("knowledge-query", RiskLevel::Read),
            ("web-fetch", RiskLevel::Read),
            ("tool-search", RiskLevel::Read),
            ("skill-search", RiskLevel::Read),
            ("file-write", RiskLevel::Write),
            ("file-edit", RiskLevel::Write),
            ("knowledge-store", RiskLevel::Write),
            ("shell-exec", RiskLevel::Execute),
            ("http-request", RiskLevel::Execute),
            ("spawn-ephemeral", RiskLevel::Execute),
        ];
        for (name, expected_risk) in expected {
            let tool = registry
                .get(name)
                .unwrap_or_else(|| panic!("built-in tool '{name}' missing"));
            assert_eq!(
                tool.metadata().risk_level,
                *expected_risk,
                "risk_level mismatch for tool '{name}'"
            );
        }
    }

    /// `with_builtins_and_authz` must produce the same set of tools as
    /// `with_builtins`, only toggling the authz kill-switch.
    #[test]
    fn with_builtins_and_authz_parity() {
        let off = TraitToolRegistry::with_builtins_and_authz(false);
        let on = TraitToolRegistry::with_builtins_and_authz(true);
        assert_eq!(off.list().len(), 14);
        assert_eq!(on.list().len(), 14);
    }

    /// `with_delegation` adds exactly three tools (`session_spawn`,
    /// `session_yield`, `session_send`) to the registry — bead sera-a1u.
    #[test]
    fn with_delegation_adds_three_session_tools() {
        use crate::delegation_bus::DelegationBus;
        let bus = DelegationBus::new();
        let registry = TraitToolRegistry::with_builtins().with_delegation(bus);
        let list = registry.list();
        assert_eq!(
            list.len(),
            14 + 3,
            "expected 14 builtins + 3 delegation tools"
        );
        for name in ["session_spawn", "session_yield", "session_send"] {
            assert!(
                registry.get(name).is_some(),
                "{name} not registered"
            );
        }
    }

    /// `with_agent_tools` adds exactly three agent-as-tool entries
    /// (`delegate-task`, `ask-agent`, `background-task`) — bead
    /// sera-8d1.1 (GH#144).
    #[test]
    fn with_agent_tools_adds_three_agent_tools() {
        use crate::agent_tool_registry::AgentToolRegistry;
        use std::sync::Arc;
        let agents = Arc::new(AgentToolRegistry::new());
        let registry = TraitToolRegistry::with_builtins().with_agent_tools(agents);
        let list = registry.list();
        assert_eq!(
            list.len(),
            14 + 3,
            "expected 14 builtins + 3 agent-as-tool entries"
        );
        for name in ["delegate-task", "ask-agent", "background-task"] {
            assert!(registry.get(name).is_some(), "{name} not registered");
        }
    }
}
