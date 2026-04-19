//! Agent-as-tool dispatch registry — bead `sera-8d1.1` (GH#144).
//!
//! Exposes sub-agent invocation as a first-class registry that the runtime's
//! tool layer can consume. Three dispatch entry points map to the three
//! [`sera_types::agent_tool::AgentToolKind`] variants:
//!
//! - [`AgentToolRegistry::dispatch_delegate`] — synchronous task delegation.
//! - [`AgentToolRegistry::dispatch_ask`] — synchronous Q&A.
//! - [`AgentToolRegistry::dispatch_background`] — fire-and-forget background
//!   task; returns a `task_id` immediately without blocking.
//!
//! Cross-cutting features:
//!
//! - **Capability gating** — every dispatch consults
//!   [`sera_types::capability::ResolvedCapabilities::subagents_allowed`] on the
//!   caller before dispatching.  Unknown or denied targets fail with
//!   [`AgentToolError::CapabilityDenied`] before reaching the [`AgentRouter`].
//! - **Budget tracking** — synchronous dispatches credit `tokens_used` from
//!   the response back to the caller's [`BudgetTracker`] so delegated work
//!   counts against the parent's token budget.
//! - **Coordinator hook** — an optional [`CoordinatorHook`] is invoked on
//!   delegate dispatches so the workflow coordinator can observe sub-agent
//!   activity (see `sera_workflow::coordination` for the consumer side).
//!
//! No real sub-agent transport exists yet for this bead — the default
//! [`InMemoryAgentRouter`] returns [`AgentToolError::AgentNotFound`] until a
//! production router (e.g. the InProcRouter from `sera-a2a`) is wired in.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use sera_types::agent_tool::{
    AgentToolKind, AskAgentInput, AskAgentOutput, BackgroundTaskInput, BackgroundTaskOutput,
    DelegateTaskInput, DelegateTaskOutput,
};
use sera_types::capability::ResolvedCapabilities;
use tokio::sync::RwLock;

// ── Caller context ────────────────────────────────────────────────────────────

/// Lightweight token-budget accumulator used to credit delegated work back
/// to the calling agent.
///
/// The runtime does not yet have a centralized budget tracker — `sera-8d1.1`
/// introduces the smallest viable interface so the registry can credit
/// `tokens_used` from a delegated agent against the caller. When the broader
/// budget system lands (separate bead) this type can be replaced by — or
/// re-exported from — that authoritative struct.
#[derive(Debug, Default)]
pub struct BudgetTracker {
    token_used: AtomicU64,
}

impl BudgetTracker {
    /// Create a fresh tracker with a zero used-token count.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `n` to the running total. Returns the new total.
    pub fn add_tokens(&self, n: u64) -> u64 {
        self.token_used.fetch_add(n, Ordering::SeqCst) + n
    }

    /// Read the current running total.
    pub fn token_used(&self) -> u64 {
        self.token_used.load(Ordering::SeqCst)
    }
}

/// Caller-side context passed into every dispatch call.
///
/// The capability check uses
/// [`ResolvedCapabilities::subagents_allowed`]; the budget tracker is
/// credited with `tokens_used` from successful synchronous dispatches.
pub struct CallerContext {
    /// Stable identifier of the calling agent — for logs and the
    /// coordinator hook.
    pub agent_id: String,
    /// Resolved capabilities for the caller. The registry only inspects the
    /// `subagents_allowed` field here.
    pub capabilities: ResolvedCapabilities,
    /// Per-call budget tracker. Shared via `Arc` so multiple concurrent
    /// dispatches against the same caller credit the same tally.
    pub budget: Arc<BudgetTracker>,
}

impl CallerContext {
    /// Helper: caller with the given id and capabilities, plus a fresh
    /// (empty) budget tracker.
    pub fn new(agent_id: impl Into<String>, capabilities: ResolvedCapabilities) -> Self {
        Self {
            agent_id: agent_id.into(),
            capabilities,
            budget: Arc::new(BudgetTracker::new()),
        }
    }
}

// ── Router trait ──────────────────────────────────────────────────────────────

/// Errors returned by the agent-tool registry.
#[derive(Debug, thiserror::Error)]
pub enum AgentToolError {
    /// The caller's capabilities do not list the requested target id under
    /// `subagents_allowed`.
    #[error("capability denied: caller '{caller}' may not invoke sub-agent '{target}'")]
    CapabilityDenied { caller: String, target: String },
    /// No router handle is registered for the requested target id.
    #[error("agent not found: '{0}'")]
    AgentNotFound(String),
    /// The underlying router rejected the dispatch (transport / runtime error).
    #[error("router error: {0}")]
    Router(String),
}

/// Pluggable cross-agent dispatch transport.
///
/// Implementations route a request to a target agent and return a structured
/// response. The default in-process implementation returns
/// `AgentNotFound` for every call — a real router (e.g. `sera-a2a`'s
/// `InProcRouter` or a remote A2A bridge) supplies the runtime behaviour.
#[async_trait]
pub trait AgentRouter: Send + Sync + 'static {
    /// Dispatch a synchronous "delegate task" request.
    async fn dispatch_delegate(
        &self,
        target: &str,
        input: DelegateTaskInput,
    ) -> Result<DelegateTaskOutput, AgentToolError>;

    /// Dispatch a synchronous "ask agent" request.
    async fn dispatch_ask(
        &self,
        target: &str,
        input: AskAgentInput,
    ) -> Result<AskAgentOutput, AgentToolError>;

    /// Spawn a background task and return its identifier without waiting.
    async fn dispatch_background(
        &self,
        target: &str,
        input: BackgroundTaskInput,
    ) -> Result<BackgroundTaskOutput, AgentToolError>;
}

/// Default no-op router: returns [`AgentToolError::AgentNotFound`] for every
/// dispatch. Wire in a real router (e.g. via `AgentToolRegistry::with_router`)
/// once cross-agent transport is available.
#[derive(Debug, Default)]
pub struct InMemoryAgentRouter;

#[async_trait]
impl AgentRouter for InMemoryAgentRouter {
    async fn dispatch_delegate(
        &self,
        target: &str,
        _input: DelegateTaskInput,
    ) -> Result<DelegateTaskOutput, AgentToolError> {
        Err(AgentToolError::AgentNotFound(target.to_string()))
    }

    async fn dispatch_ask(
        &self,
        target: &str,
        _input: AskAgentInput,
    ) -> Result<AskAgentOutput, AgentToolError> {
        Err(AgentToolError::AgentNotFound(target.to_string()))
    }

    async fn dispatch_background(
        &self,
        target: &str,
        _input: BackgroundTaskInput,
    ) -> Result<BackgroundTaskOutput, AgentToolError> {
        Err(AgentToolError::AgentNotFound(target.to_string()))
    }
}

// ── Coordinator hook ──────────────────────────────────────────────────────────

/// Notification a coordinator (e.g. `sera_workflow::coordination::Coordinator`)
/// receives when an agent dispatches a delegate-task call. The hook fires
/// _after_ a successful dispatch.
#[derive(Debug, Clone)]
pub struct DelegationNotice {
    /// The agent that issued the delegate-task call.
    pub caller: String,
    /// The agent that received the delegated task.
    pub target: String,
    /// Tokens credited back to the caller's budget for this dispatch.
    pub tokens_used: u64,
}

/// Trait implemented by the workflow coordinator (or any consumer) to
/// observe successful delegate-task dispatches.
///
/// See `sera_workflow::coordination` for the ultimate sink. Wire-up is
/// optional — a registry without a coordinator hook still functions.
pub trait CoordinatorHook: Send + Sync + 'static {
    /// Called after a successful synchronous delegate-task dispatch.
    fn on_delegate(&self, notice: DelegationNotice);
}

// ── Registry ──────────────────────────────────────────────────────────────────

/// Opaque handle to a registered agent route.
///
/// Today this is just the agent id, kept opaque so the registry can later
/// hold a richer object (e.g. a sender into a per-agent task queue) without
/// breaking callers.
#[derive(Debug, Clone)]
pub struct AgentRouteHandle {
    pub agent_id: String,
}

/// Registry of agent routes plus the dispatch entry points.
pub struct AgentToolRegistry {
    agents: Arc<RwLock<HashMap<String, AgentRouteHandle>>>,
    router: Arc<dyn AgentRouter>,
    coordinator: Option<Arc<dyn CoordinatorHook>>,
}

impl AgentToolRegistry {
    /// Build an empty registry backed by [`InMemoryAgentRouter`].
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            router: Arc::new(InMemoryAgentRouter),
            coordinator: None,
        }
    }

    /// Build a registry backed by a specific [`AgentRouter`] implementation.
    pub fn with_router(router: Arc<dyn AgentRouter>) -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            router,
            coordinator: None,
        }
    }

    /// Attach a coordinator hook. Replaces any previously installed hook.
    pub fn set_coordinator(&mut self, hook: Arc<dyn CoordinatorHook>) {
        self.coordinator = Some(hook);
    }

    /// Register a target agent under `name`. The handle is opaque — callers
    /// pass only the registered name to the dispatch methods.
    pub async fn register(&self, name: impl Into<String>, handle: AgentRouteHandle) {
        let name = name.into();
        self.agents.write().await.insert(name, handle);
    }

    /// Return the list of currently registered agent names.
    pub async fn registered(&self) -> Vec<String> {
        let mut names: Vec<String> = self.agents.read().await.keys().cloned().collect();
        names.sort();
        names
    }

    /// Synchronous "delegate this task" dispatch. Blocks until the target
    /// returns a structured result. Credits the response's `tokens_used`
    /// back to `caller.budget`.
    pub async fn dispatch_delegate(
        &self,
        caller: &CallerContext,
        target: &str,
        input: DelegateTaskInput,
    ) -> Result<DelegateTaskOutput, AgentToolError> {
        self.gate_capability(caller, target)?;
        let output = self.router.dispatch_delegate(target, input).await?;
        caller.budget.add_tokens(output.tokens_used);
        if let Some(hook) = &self.coordinator {
            hook.on_delegate(DelegationNotice {
                caller: caller.agent_id.clone(),
                target: target.to_string(),
                tokens_used: output.tokens_used,
            });
        }
        Ok(output)
    }

    /// Synchronous "ask this agent" dispatch. Blocks until the target
    /// produces an answer. Credits `tokens_used` to `caller.budget`.
    pub async fn dispatch_ask(
        &self,
        caller: &CallerContext,
        target: &str,
        input: AskAgentInput,
    ) -> Result<AskAgentOutput, AgentToolError> {
        self.gate_capability(caller, target)?;
        let output = self.router.dispatch_ask(target, input).await?;
        caller.budget.add_tokens(output.tokens_used);
        Ok(output)
    }

    /// Asynchronous "background task" dispatch. Returns immediately with a
    /// task id; no tokens are credited because no work has happened yet.
    pub async fn dispatch_background(
        &self,
        caller: &CallerContext,
        target: &str,
        input: BackgroundTaskInput,
    ) -> Result<BackgroundTaskOutput, AgentToolError> {
        self.gate_capability(caller, target)?;
        self.router.dispatch_background(target, input).await
    }

    /// Helper used by every kind. Capability gate based on
    /// `caller.capabilities.subagents_allowed` — the registry intentionally
    /// does not require pre-registration of the target name; only the
    /// capability allow-list is consulted, matching the spec's
    /// "DO NOT invent a new" capability struct guidance.
    fn gate_capability(
        &self,
        caller: &CallerContext,
        target: &str,
    ) -> Result<(), AgentToolError> {
        let allowed = caller
            .capabilities
            .subagents_allowed
            .as_ref()
            .is_some_and(|list| list.iter().any(|t| t == target));
        if allowed {
            Ok(())
        } else {
            Err(AgentToolError::CapabilityDenied {
                caller: caller.agent_id.clone(),
                target: target.to_string(),
            })
        }
    }

    /// Map an [`AgentToolKind`] to the matching dispatch helper. Returned
    /// payload is JSON-encoded so it can be threaded through `ToolOutput`.
    pub async fn dispatch_kind(
        &self,
        caller: &CallerContext,
        kind: AgentToolKind,
        target: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, AgentToolError> {
        match kind {
            AgentToolKind::DelegateTask => {
                let input: DelegateTaskInput = serde_json::from_value(arguments)
                    .map_err(|e| AgentToolError::Router(format!("decode delegate-task input: {e}")))?;
                let out = self.dispatch_delegate(caller, target, input).await?;
                serde_json::to_value(out)
                    .map_err(|e| AgentToolError::Router(format!("encode delegate-task output: {e}")))
            }
            AgentToolKind::AskAgent => {
                let input: AskAgentInput = serde_json::from_value(arguments)
                    .map_err(|e| AgentToolError::Router(format!("decode ask-agent input: {e}")))?;
                let out = self.dispatch_ask(caller, target, input).await?;
                serde_json::to_value(out)
                    .map_err(|e| AgentToolError::Router(format!("encode ask-agent output: {e}")))
            }
            AgentToolKind::BackgroundTask => {
                let input: BackgroundTaskInput = serde_json::from_value(arguments).map_err(|e| {
                    AgentToolError::Router(format!("decode background-task input: {e}"))
                })?;
                let out = self.dispatch_background(caller, target, input).await?;
                serde_json::to_value(out).map_err(|e| {
                    AgentToolError::Router(format!("encode background-task output: {e}"))
                })
            }
        }
    }
}

impl Default for AgentToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Test-only fixture router ─────────────────────────────────────────────────

/// In-test router that records every dispatch and returns canned responses.
/// Lives in the production tree (gated `#[cfg(any(test, feature = ...))]`
/// would also work) so the corresponding agent-tools in `tools/agent_tools`
/// can re-use it from their own tests.
#[doc(hidden)]
pub mod test_router {
    use super::*;
    use std::sync::Mutex;

    /// Recorded dispatches — useful for asserting in-test hook behaviour.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum DispatchRecord {
        Delegate { target: String, task: String },
        Ask { target: String, question: String },
        Background { target: String, task: String },
    }

    /// Test router: returns canned `tokens_used` and records each dispatch.
    pub struct CannedAgentRouter {
        pub tokens_per_call: u64,
        pub records: Arc<Mutex<Vec<DispatchRecord>>>,
    }

    impl CannedAgentRouter {
        pub fn new(tokens_per_call: u64) -> Self {
            Self {
                tokens_per_call,
                records: Arc::new(Mutex::new(Vec::new())),
            }
        }

        pub fn records(&self) -> Vec<DispatchRecord> {
            self.records.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl AgentRouter for CannedAgentRouter {
        async fn dispatch_delegate(
            &self,
            target: &str,
            input: DelegateTaskInput,
        ) -> Result<DelegateTaskOutput, AgentToolError> {
            self.records.lock().unwrap().push(DispatchRecord::Delegate {
                target: target.to_string(),
                task: input.task.clone(),
            });
            Ok(DelegateTaskOutput {
                result: serde_json::json!({"echo": input.task}),
                tokens_used: self.tokens_per_call,
            })
        }

        async fn dispatch_ask(
            &self,
            target: &str,
            input: AskAgentInput,
        ) -> Result<AskAgentOutput, AgentToolError> {
            self.records.lock().unwrap().push(DispatchRecord::Ask {
                target: target.to_string(),
                question: input.question.clone(),
            });
            Ok(AskAgentOutput {
                answer: format!("answer-to:{}", input.question),
                tokens_used: self.tokens_per_call,
            })
        }

        async fn dispatch_background(
            &self,
            target: &str,
            input: BackgroundTaskInput,
        ) -> Result<BackgroundTaskOutput, AgentToolError> {
            self.records
                .lock()
                .unwrap()
                .push(DispatchRecord::Background {
                    target: target.to_string(),
                    task: input.task.clone(),
                });
            Ok(BackgroundTaskOutput {
                task_id: format!("bg-{}-{}", target, input.task.len()),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_router::{CannedAgentRouter, DispatchRecord};
    use super::*;
    use std::sync::Mutex;

    fn caller(id: &str, allow: Option<Vec<&str>>) -> CallerContext {
        let caps = ResolvedCapabilities {
            subagents_allowed: allow.map(|v| v.into_iter().map(String::from).collect()),
            ..Default::default()
        };
        CallerContext::new(id, caps)
    }

    #[tokio::test]
    async fn delegate_task_blocks_until_result_returned() {
        let router = Arc::new(CannedAgentRouter::new(42));
        let registry = AgentToolRegistry::with_router(router.clone());
        let caller = caller("parent", Some(vec!["worker"]));

        let out = registry
            .dispatch_delegate(
                &caller,
                "worker",
                DelegateTaskInput {
                    task: "do-it".into(),
                    context: None,
                },
            )
            .await
            .expect("delegate dispatch ok");

        assert_eq!(out.tokens_used, 42);
        assert_eq!(out.result["echo"], "do-it");
        assert_eq!(
            router.records(),
            vec![DispatchRecord::Delegate {
                target: "worker".into(),
                task: "do-it".into(),
            }]
        );
    }

    #[tokio::test]
    async fn ask_agent_returns_answer() {
        let router = Arc::new(CannedAgentRouter::new(7));
        let registry = AgentToolRegistry::with_router(router);
        let caller = caller("parent", Some(vec!["sage"]));

        let out = registry
            .dispatch_ask(
                &caller,
                "sage",
                AskAgentInput {
                    question: "why?".into(),
                },
            )
            .await
            .expect("ask dispatch ok");

        assert_eq!(out.answer, "answer-to:why?");
        assert_eq!(out.tokens_used, 7);
    }

    #[tokio::test]
    async fn capability_denial_blocks_dispatch() {
        let router = Arc::new(CannedAgentRouter::new(1));
        let registry = AgentToolRegistry::with_router(router.clone());
        // Caller is allowed to call "alpha", NOT "beta".
        let caller = caller("parent", Some(vec!["alpha"]));

        let err = registry
            .dispatch_delegate(
                &caller,
                "beta",
                DelegateTaskInput {
                    task: "x".into(),
                    context: None,
                },
            )
            .await
            .expect_err("must deny");
        assert!(matches!(
            err,
            AgentToolError::CapabilityDenied { ref target, .. } if target == "beta"
        ));
        // Router was never called.
        assert!(router.records().is_empty());
    }

    #[tokio::test]
    async fn capability_denial_when_no_allowlist() {
        let router = Arc::new(CannedAgentRouter::new(1));
        let registry = AgentToolRegistry::with_router(router);
        // No `subagents_allowed` set at all → all targets denied.
        let caller = caller("parent", None);

        let err = registry
            .dispatch_ask(
                &caller,
                "anyone",
                AskAgentInput {
                    question: "?".into(),
                },
            )
            .await
            .expect_err("no allow-list = deny all");
        assert!(matches!(err, AgentToolError::CapabilityDenied { .. }));
    }

    #[tokio::test]
    async fn budget_tracker_increments_by_tokens_used() {
        let router = Arc::new(CannedAgentRouter::new(100));
        let registry = AgentToolRegistry::with_router(router);
        let caller = caller("parent", Some(vec!["worker"]));
        assert_eq!(caller.budget.token_used(), 0);

        let _ = registry
            .dispatch_delegate(
                &caller,
                "worker",
                DelegateTaskInput {
                    task: "first".into(),
                    context: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(caller.budget.token_used(), 100);

        let _ = registry
            .dispatch_ask(
                &caller,
                "worker",
                AskAgentInput {
                    question: "second".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(caller.budget.token_used(), 200);
    }

    #[tokio::test]
    async fn background_task_returns_id_without_blocking() {
        let router = Arc::new(CannedAgentRouter::new(999));
        let registry = AgentToolRegistry::with_router(router);
        let caller = caller("parent", Some(vec!["bg"]));
        let before = caller.budget.token_used();

        let out = registry
            .dispatch_background(
                &caller,
                "bg",
                BackgroundTaskInput {
                    task: "rebuild".into(),
                },
            )
            .await
            .expect("background dispatch ok");

        assert!(out.task_id.starts_with("bg-bg-"));
        // Background dispatch must NOT credit tokens — the work hasn't run.
        assert_eq!(caller.budget.token_used(), before);
    }

    #[tokio::test]
    async fn coordinator_hook_observes_delegate() {
        struct RecordingHook(Arc<Mutex<Vec<DelegationNotice>>>);
        impl CoordinatorHook for RecordingHook {
            fn on_delegate(&self, notice: DelegationNotice) {
                self.0.lock().unwrap().push(notice);
            }
        }
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut registry = AgentToolRegistry::with_router(Arc::new(CannedAgentRouter::new(5)));
        registry.set_coordinator(Arc::new(RecordingHook(log.clone())));
        let caller = caller("parent", Some(vec!["worker"]));

        let _ = registry
            .dispatch_delegate(
                &caller,
                "worker",
                DelegateTaskInput {
                    task: "go".into(),
                    context: None,
                },
            )
            .await
            .unwrap();

        let notices = log.lock().unwrap().clone();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].caller, "parent");
        assert_eq!(notices[0].target, "worker");
        assert_eq!(notices[0].tokens_used, 5);
    }

    #[tokio::test]
    async fn dispatch_kind_routes_each_variant() {
        let router = Arc::new(CannedAgentRouter::new(3));
        let registry = AgentToolRegistry::with_router(router);
        let caller = caller("parent", Some(vec!["w"]));

        let v = registry
            .dispatch_kind(
                &caller,
                AgentToolKind::DelegateTask,
                "w",
                serde_json::json!({"task": "t"}),
            )
            .await
            .unwrap();
        assert_eq!(v["tokens_used"], 3);

        let v = registry
            .dispatch_kind(
                &caller,
                AgentToolKind::AskAgent,
                "w",
                serde_json::json!({"question": "q"}),
            )
            .await
            .unwrap();
        assert_eq!(v["answer"], "answer-to:q");

        let v = registry
            .dispatch_kind(
                &caller,
                AgentToolKind::BackgroundTask,
                "w",
                serde_json::json!({"task": "bg"}),
            )
            .await
            .unwrap();
        assert!(v["task_id"].as_str().unwrap().starts_with("bg-w-"));
    }

    #[tokio::test]
    async fn default_in_memory_router_returns_not_found() {
        let registry = AgentToolRegistry::new();
        let caller = caller("parent", Some(vec!["nobody"]));
        let err = registry
            .dispatch_delegate(
                &caller,
                "nobody",
                DelegateTaskInput {
                    task: "t".into(),
                    context: None,
                },
            )
            .await
            .expect_err("default router has no agents");
        assert!(matches!(err, AgentToolError::AgentNotFound(_)));
    }

    #[tokio::test]
    async fn register_and_list() {
        let registry = AgentToolRegistry::new();
        registry
            .register(
                "alpha",
                AgentRouteHandle {
                    agent_id: "alpha".into(),
                },
            )
            .await;
        registry
            .register(
                "beta",
                AgentRouteHandle {
                    agent_id: "beta".into(),
                },
            )
            .await;
        assert_eq!(registry.registered().await, vec!["alpha", "beta"]);
    }
}
