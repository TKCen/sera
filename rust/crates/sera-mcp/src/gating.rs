//! MCP context gating — runtime filter that decides which tools from connected
//! MCP servers are injected into the model's context window for a given turn.
//!
//! Gating is **pre-authorization display filtering only** — it does not replace
//! or modify the authorization check performed at tool-call time by
//! `sera-auth`. Tools that pass gating are still subject to the full
//! authorization pipeline when invoked.
//!
//! See `docs/plan/AGENTSKILLS-MCP-RESEARCH.md` §7 for the canonical design.

use crate::McpToolDescriptor;

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

/// Context provided to a gating policy when deciding tool visibility for a
/// given agent turn.
///
/// All fields are owned/plain types so that `sera-mcp` does not need to depend
/// on `sera-skills` (the crate graph stays a DAG).
#[derive(Debug, Clone, Default)]
pub struct ToolGatingContext {
    /// The agent's identity.
    pub agent_id: String,
    /// Currently active skill names (as resolved by the runtime from a
    /// `SkillRegistry`-equivalent source).
    pub active_skills: Vec<String>,
    /// Tool bindings declared by the active skills (fully-qualified names or
    /// server prefixes, e.g. `"github.create_issue"` or `"github"`).
    pub skill_tool_bindings: Vec<String>,
    /// Task classification hint (e.g. `"code-review"`, `"planning"`).
    pub task_class: Option<String>,
    /// Maximum number of tool schemas to inject this turn. `usize::MAX` means
    /// no truncation.
    pub max_tools: usize,
}

// ---------------------------------------------------------------------------
// Gate trait
// ---------------------------------------------------------------------------

/// A filter that decides which MCP tools are visible in a given context.
///
/// Gates are composable — multiple gates can be chained with [`AndGate`] /
/// [`OrGate`].
pub trait McpToolGate: Send + Sync + 'static {
    /// Returns true if the tool should be visible in this context.
    fn is_visible(&self, tool: &McpToolDescriptor, ctx: &ToolGatingContext) -> bool;
}

// ---------------------------------------------------------------------------
// Built-in gates
// ---------------------------------------------------------------------------

/// Trivial gate that passes every tool through. Useful as a default when no
/// policy is configured and in tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct AlwaysVisibleGate;

impl McpToolGate for AlwaysVisibleGate {
    fn is_visible(&self, _tool: &McpToolDescriptor, _ctx: &ToolGatingContext) -> bool {
        true
    }
}

/// Gate: only show tools listed in the active skills' `tool_bindings`.
///
/// Matches a tool if its fully-qualified name equals a binding or starts with
/// `"{binding}."` (so a binding of `"github"` matches every `github.*` tool).
///
/// If no bindings are declared, allow all tools (backward-compat — agents
/// without skill-scoped tool constraints keep their current behavior).
#[derive(Debug, Clone, Copy, Default)]
pub struct SkillBoundGate;

impl McpToolGate for SkillBoundGate {
    fn is_visible(&self, tool: &McpToolDescriptor, ctx: &ToolGatingContext) -> bool {
        if ctx.skill_tool_bindings.is_empty() {
            return true;
        }
        ctx.skill_tool_bindings.iter().any(|binding| {
            tool.name == *binding || tool.name.starts_with(&format!("{binding}."))
        })
    }
}

/// Gate: only show tools from explicitly allowed MCP server namespaces.
///
/// The SERA namespacing convention is `"server_name.tool_name"`. Un-namespaced
/// tools (no `.`) are treated as built-in and pass through.
#[derive(Debug, Clone, Default)]
pub struct AllowedServerGate {
    pub allowed_servers: Vec<String>,
}

impl McpToolGate for AllowedServerGate {
    fn is_visible(&self, tool: &McpToolDescriptor, _ctx: &ToolGatingContext) -> bool {
        if let Some((server, _)) = tool.name.split_once('.') {
            self.allowed_servers.iter().any(|s| s == server)
        } else {
            true
        }
    }
}

/// Compose two gates with AND semantics — a tool is visible only if both
/// gates accept it.
#[derive(Debug, Clone, Copy, Default)]
pub struct AndGate<A: McpToolGate, B: McpToolGate> {
    pub a: A,
    pub b: B,
}

impl<A: McpToolGate, B: McpToolGate> AndGate<A, B> {
    pub fn new(a: A, b: B) -> Self {
        Self { a, b }
    }
}

impl<A: McpToolGate, B: McpToolGate> McpToolGate for AndGate<A, B> {
    fn is_visible(&self, tool: &McpToolDescriptor, ctx: &ToolGatingContext) -> bool {
        self.a.is_visible(tool, ctx) && self.b.is_visible(tool, ctx)
    }
}

/// Compose two gates with OR semantics — a tool is visible if either gate
/// accepts it.
#[derive(Debug, Clone, Copy, Default)]
pub struct OrGate<A: McpToolGate, B: McpToolGate> {
    pub a: A,
    pub b: B,
}

impl<A: McpToolGate, B: McpToolGate> OrGate<A, B> {
    pub fn new(a: A, b: B) -> Self {
        Self { a, b }
    }
}

impl<A: McpToolGate, B: McpToolGate> McpToolGate for OrGate<A, B> {
    fn is_visible(&self, tool: &McpToolDescriptor, ctx: &ToolGatingContext) -> bool {
        self.a.is_visible(tool, ctx) || self.b.is_visible(tool, ctx)
    }
}

// ---------------------------------------------------------------------------
// Gated client bridge
// ---------------------------------------------------------------------------

use crate::{McpClientBridge, McpError, McpServerConfig, McpToolResult};
use async_trait::async_trait;

/// A gated MCP client bridge wraps an inner bridge with a visibility filter.
///
/// All methods delegate to the inner bridge unchanged; only
/// [`McpClientBridge::list_tools_for_context`] applies the gate and truncates
/// to `ctx.max_tools`.
pub struct GatedMcpClientBridge<B, G>
where
    B: McpClientBridge + Send + Sync,
    G: McpToolGate + Send + Sync,
{
    pub inner: B,
    pub gate: G,
}

impl<B, G> GatedMcpClientBridge<B, G>
where
    B: McpClientBridge + Send + Sync,
    G: McpToolGate + Send + Sync,
{
    pub fn new(inner: B, gate: G) -> Self {
        Self { inner, gate }
    }
}

#[async_trait]
impl<B, G> McpClientBridge for GatedMcpClientBridge<B, G>
where
    B: McpClientBridge + Send + Sync + 'static,
    G: McpToolGate + Send + Sync + 'static,
{
    async fn connect(&self, config: &McpServerConfig) -> Result<(), McpError> {
        self.inner.connect(config).await
    }

    async fn disconnect(&self, server_name: &str) -> Result<(), McpError> {
        self.inner.disconnect(server_name).await
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        // Return all tools unfiltered — callers that want gating must call
        // `list_tools_for_context`.
        self.inner.list_tools().await
    }

    async fn list_server_tools(
        &self,
        server_name: &str,
    ) -> Result<Vec<McpToolDescriptor>, McpError> {
        self.inner.list_server_tools(server_name).await
    }

    async fn call_tool(
        &self,
        namespaced_tool: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolResult, McpError> {
        self.inner.call_tool(namespaced_tool, arguments).await
    }

    async fn list_tools_for_context(
        &self,
        ctx: &ToolGatingContext,
    ) -> Result<Vec<McpToolDescriptor>, McpError> {
        let all = self.inner.list_tools().await?;
        let visible: Vec<_> = all
            .into_iter()
            .filter(|t| self.gate.is_visible(t, ctx))
            .take(ctx.max_tools)
            .collect();
        Ok(visible)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn tool(name: &str) -> McpToolDescriptor {
        McpToolDescriptor {
            name: name.to_string(),
            description: format!("desc for {name}"),
            input_schema: serde_json::json!({}),
        }
    }

    fn ctx_with(bindings: Vec<&str>, max_tools: usize) -> ToolGatingContext {
        ToolGatingContext {
            agent_id: "agent-1".into(),
            active_skills: vec![],
            skill_tool_bindings: bindings.into_iter().map(String::from).collect(),
            task_class: None,
            max_tools,
        }
    }

    // --- SkillBoundGate ---------------------------------------------------

    #[test]
    fn skill_bound_empty_bindings_allows_all() {
        let gate = SkillBoundGate;
        let ctx = ctx_with(vec![], usize::MAX);
        assert!(gate.is_visible(&tool("github.create_issue"), &ctx));
        assert!(gate.is_visible(&tool("filesystem.read"), &ctx));
        assert!(gate.is_visible(&tool("builtin_tool"), &ctx));
    }

    #[test]
    fn skill_bound_exact_name_match() {
        let gate = SkillBoundGate;
        let ctx = ctx_with(vec!["github.create_issue"], usize::MAX);
        assert!(gate.is_visible(&tool("github.create_issue"), &ctx));
        assert!(!gate.is_visible(&tool("github.list_issues"), &ctx));
    }

    #[test]
    fn skill_bound_prefix_match() {
        let gate = SkillBoundGate;
        let ctx = ctx_with(vec!["github"], usize::MAX);
        assert!(gate.is_visible(&tool("github.create_issue"), &ctx));
        assert!(gate.is_visible(&tool("github.list_issues"), &ctx));
        assert!(!gate.is_visible(&tool("filesystem.read"), &ctx));
    }

    #[test]
    fn skill_bound_prefix_does_not_match_substring() {
        // "git" must NOT match "github.*" — only dot-boundary matches count.
        let gate = SkillBoundGate;
        let ctx = ctx_with(vec!["git"], usize::MAX);
        assert!(!gate.is_visible(&tool("github.create_issue"), &ctx));
        assert!(gate.is_visible(&tool("git.status"), &ctx));
    }

    // --- AllowedServerGate ------------------------------------------------

    #[test]
    fn allowed_server_filters_by_namespace() {
        let gate = AllowedServerGate {
            allowed_servers: vec!["github".into(), "filesystem".into()],
        };
        let ctx = ctx_with(vec![], usize::MAX);
        assert!(gate.is_visible(&tool("github.create_issue"), &ctx));
        assert!(gate.is_visible(&tool("filesystem.read"), &ctx));
        assert!(!gate.is_visible(&tool("slack.send_message"), &ctx));
    }

    #[test]
    fn allowed_server_passes_through_unnamespaced_tools() {
        let gate = AllowedServerGate {
            allowed_servers: vec!["github".into()],
        };
        let ctx = ctx_with(vec![], usize::MAX);
        // Built-in tools without a "." are treated as un-namespaced and pass.
        assert!(gate.is_visible(&tool("builtin_tool"), &ctx));
    }

    // --- AndGate / OrGate -------------------------------------------------

    #[test]
    fn and_gate_requires_both() {
        let gate = AndGate::new(
            SkillBoundGate,
            AllowedServerGate {
                allowed_servers: vec!["github".into()],
            },
        );
        let ctx = ctx_with(vec!["github"], usize::MAX);
        assert!(gate.is_visible(&tool("github.create_issue"), &ctx));
        // SkillBoundGate rejects: not in bindings
        assert!(!gate.is_visible(&tool("filesystem.read"), &ctx));
        // AllowedServerGate rejects: wrong namespace
        let ctx2 = ctx_with(vec!["slack"], usize::MAX);
        assert!(!gate.is_visible(&tool("slack.send"), &ctx2));
    }

    #[test]
    fn or_gate_requires_either() {
        let gate = OrGate::new(
            AllowedServerGate {
                allowed_servers: vec!["github".into()],
            },
            AllowedServerGate {
                allowed_servers: vec!["slack".into()],
            },
        );
        let ctx = ctx_with(vec![], usize::MAX);
        assert!(gate.is_visible(&tool("github.create_issue"), &ctx));
        assert!(gate.is_visible(&tool("slack.send"), &ctx));
        assert!(!gate.is_visible(&tool("filesystem.read"), &ctx));
    }

    // --- AlwaysVisibleGate ------------------------------------------------

    #[test]
    fn always_visible_passes_everything() {
        let gate = AlwaysVisibleGate;
        let ctx = ctx_with(vec!["anything"], 0);
        assert!(gate.is_visible(&tool("github.create_issue"), &ctx));
        assert!(gate.is_visible(&tool("some_random_tool"), &ctx));
    }

    // --- GatedMcpClientBridge --------------------------------------------

    /// Fake bridge that returns a fixed tool list and counts invocations.
    struct FakeBridge {
        tools: Vec<McpToolDescriptor>,
        list_calls: Mutex<usize>,
    }

    impl FakeBridge {
        fn new(tools: Vec<McpToolDescriptor>) -> Self {
            Self {
                tools,
                list_calls: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl McpClientBridge for FakeBridge {
        async fn connect(&self, _config: &McpServerConfig) -> Result<(), McpError> {
            Ok(())
        }
        async fn disconnect(&self, _server_name: &str) -> Result<(), McpError> {
            Ok(())
        }
        async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
            *self.list_calls.lock().unwrap() += 1;
            Ok(self.tools.clone())
        }
        async fn list_server_tools(
            &self,
            _server_name: &str,
        ) -> Result<Vec<McpToolDescriptor>, McpError> {
            Ok(self.tools.clone())
        }
        async fn call_tool(
            &self,
            _namespaced_tool: &str,
            _arguments: serde_json::Value,
        ) -> Result<McpToolResult, McpError> {
            Ok(McpToolResult {
                content: serde_json::json!({}),
                is_error: false,
            })
        }
    }

    #[tokio::test]
    async fn default_list_tools_for_context_returns_same_as_list_tools() {
        // Default impl on the base trait should fall back to list_tools()
        // unfiltered, so a non-gated bridge returns everything.
        let bridge = FakeBridge::new(vec![
            tool("github.create_issue"),
            tool("slack.send"),
            tool("filesystem.read"),
        ]);
        let ctx = ctx_with(vec!["github"], 1); // bindings/max_tools are ignored by default impl
        let all = bridge.list_tools().await.unwrap();
        let via_ctx = bridge.list_tools_for_context(&ctx).await.unwrap();
        assert_eq!(all.len(), via_ctx.len());
        assert_eq!(
            all.iter().map(|t| &t.name).collect::<Vec<_>>(),
            via_ctx.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn gated_bridge_applies_gate_and_truncates() {
        let bridge = FakeBridge::new(vec![
            tool("github.create_issue"),
            tool("github.list_issues"),
            tool("github.close_issue"),
            tool("slack.send"),
            tool("filesystem.read"),
        ]);
        let gated = GatedMcpClientBridge::new(
            bridge,
            AllowedServerGate {
                allowed_servers: vec!["github".into()],
            },
        );
        let ctx = ctx_with(vec![], 2); // max_tools = 2
        let out = gated.list_tools_for_context(&ctx).await.unwrap();
        assert_eq!(out.len(), 2);
        // All returned tools must be in the github namespace.
        for t in &out {
            assert!(t.name.starts_with("github."));
        }
    }

    #[tokio::test]
    async fn gated_bridge_max_tools_zero_returns_empty() {
        let bridge = FakeBridge::new(vec![tool("github.create_issue")]);
        let gated = GatedMcpClientBridge::new(bridge, AlwaysVisibleGate);
        let ctx = ctx_with(vec![], 0);
        let out = gated.list_tools_for_context(&ctx).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn gated_bridge_list_tools_unfiltered() {
        // Plain `list_tools` on the gated bridge returns everything — only
        // `list_tools_for_context` applies the gate.
        let bridge = FakeBridge::new(vec![tool("github.x"), tool("slack.y")]);
        let gated = GatedMcpClientBridge::new(
            bridge,
            AllowedServerGate {
                allowed_servers: vec!["github".into()],
            },
        );
        let out = gated.list_tools().await.unwrap();
        assert_eq!(out.len(), 2);
    }

    // --- Error propagation through GatedMcpClientBridge ------------------

    struct ErrorBridge;

    #[async_trait]
    impl McpClientBridge for ErrorBridge {
        async fn connect(&self, _config: &McpServerConfig) -> Result<(), McpError> {
            Err(McpError::ConnectionFailed {
                reason: "injected".into(),
            })
        }
        async fn disconnect(&self, _server_name: &str) -> Result<(), McpError> {
            Err(McpError::UnknownServer("injected".into()))
        }
        async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
            Err(McpError::NotConnected)
        }
        async fn list_server_tools(
            &self,
            _server_name: &str,
        ) -> Result<Vec<McpToolDescriptor>, McpError> {
            Err(McpError::NotConnected)
        }
        async fn call_tool(
            &self,
            _namespaced_tool: &str,
            _arguments: serde_json::Value,
        ) -> Result<McpToolResult, McpError> {
            Err(McpError::Transport {
                reason: "injected".into(),
            })
        }
    }

    #[tokio::test]
    async fn gated_bridge_propagates_list_tools_error() {
        let gated = GatedMcpClientBridge::new(ErrorBridge, AlwaysVisibleGate);
        let ctx = ctx_with(vec![], usize::MAX);
        let err = gated.list_tools_for_context(&ctx).await.unwrap_err();
        assert!(matches!(err, McpError::NotConnected));
    }

    #[tokio::test]
    async fn gated_bridge_propagates_connect_error() {
        let gated = GatedMcpClientBridge::new(
            ErrorBridge,
            AlwaysVisibleGate,
        );
        use crate::{McpTransport, McpServerConfig};
        use std::collections::HashMap;
        let cfg = McpServerConfig {
            name: "x".into(),
            url: None,
            command: Some("true".into()),
            args: vec![],
            transport: McpTransport::Stdio,
            env: HashMap::new(),
        };
        let err = gated.connect(&cfg).await.unwrap_err();
        assert!(matches!(err, McpError::ConnectionFailed { .. }));
    }

    #[tokio::test]
    async fn gated_bridge_propagates_call_tool_error() {
        let gated = GatedMcpClientBridge::new(ErrorBridge, AlwaysVisibleGate);
        let err = gated
            .call_tool("srv.tool", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::Transport { .. }));
    }

    // --- ToolGatingContext fields ----------------------------------------

    #[test]
    fn tool_gating_context_task_class_field() {
        let ctx = ToolGatingContext {
            agent_id: "a1".into(),
            active_skills: vec!["code-review".into()],
            skill_tool_bindings: vec!["github".into()],
            task_class: Some("code-review".into()),
            max_tools: 10,
        };
        assert_eq!(ctx.task_class.as_deref(), Some("code-review"));
        assert_eq!(ctx.active_skills.len(), 1);
    }

    #[test]
    fn tool_gating_context_default_is_empty() {
        let ctx = ToolGatingContext::default();
        assert!(ctx.agent_id.is_empty());
        assert!(ctx.active_skills.is_empty());
        assert!(ctx.skill_tool_bindings.is_empty());
        assert!(ctx.task_class.is_none());
        assert_eq!(ctx.max_tools, 0);
    }

    // --- AndGate / OrGate Default impls ----------------------------------

    #[test]
    fn and_gate_default_two_always_visible_passes_all() {
        let gate: AndGate<AlwaysVisibleGate, AlwaysVisibleGate> = AndGate::default();
        let ctx = ctx_with(vec![], usize::MAX);
        assert!(gate.is_visible(&tool("anything"), &ctx));
    }

    #[test]
    fn skill_bound_gate_exact_match_and_prefix_overlap() {
        // Verify both exact and prefix bindings coexist correctly in a single
        // context: "github.create_issue" (exact) + "slack" (prefix).
        let gate = SkillBoundGate;
        let ctx = ctx_with(vec!["github.create_issue", "slack"], usize::MAX);
        assert!(gate.is_visible(&tool("github.create_issue"), &ctx));
        assert!(!gate.is_visible(&tool("github.list_issues"), &ctx));
        assert!(gate.is_visible(&tool("slack.send"), &ctx));
        assert!(gate.is_visible(&tool("slack.dm"), &ctx));
        assert!(!gate.is_visible(&tool("filesystem.read"), &ctx));
    }

    #[test]
    fn allowed_server_gate_empty_allowed_list_blocks_all_namespaced() {
        let gate = AllowedServerGate {
            allowed_servers: vec![],
        };
        let ctx = ctx_with(vec![], usize::MAX);
        // Namespaced tools are blocked when allowed list is empty.
        assert!(!gate.is_visible(&tool("github.create_issue"), &ctx));
        // Un-namespaced tools still pass through.
        assert!(gate.is_visible(&tool("builtin"), &ctx));
    }
}
