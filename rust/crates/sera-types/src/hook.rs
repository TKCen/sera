//! Hook types — the extensibility backbone of SERA.
//!
//! Hooks are chainable WASM pipelines that fire at 16 hook points across the system.
//! Every major operation (routing, tool execution, memory writes, session transitions)
//! can be intercepted and enriched by hook chains.
//!
//! Types live here in sera-types so any crate can reference them without pulling in
//! the wasmtime runtime. The actual WASM execution lives in sera-hooks.
//!
//! See SPEC-hooks for the full design and SPEC-gateway §4 for hook point definitions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

// ── Hook Points ──────────────────────────────────────────────────────────────

/// The 20 hook points where chains can fire across the SERA system.
/// SPEC-hooks: each point has a defined context shape and allowed result types.
///
/// Ordering follows the event lifecycle: route → turn → tool → deliver → memory → session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPoint {
    /// After ingress, before queue — content filtering, rate limiting.
    PreRoute,
    /// After routing decision, before enqueue — routing override, logging.
    PostRoute,
    /// After dequeue, before context assembly — context enrichment, policy.
    PreTurn,
    /// During persona assembly — persona switching, mode injection.
    ContextPersona,
    /// During memory injection — tier selection, RAG tuning.
    /// Hermes alias: `pre_agent_turn` (maps to Hermes `prefetch_all`).
    #[serde(alias = "pre_agent_turn")]
    ContextMemory,
    /// During skill injection — skill filtering, mode transitions.
    ContextSkill,
    /// During tool injection — tool filtering, capability policy.
    ContextTool,
    /// Before LLM call — prompt inspection, cost control, context trimming.
    #[serde(rename = "on_llm_start")]
    OnLlmStart,
    /// Before tool execution — approval gates, argument validation, secret injection.
    PreTool,
    /// After tool execution — result sanitization, audit, risk assessment.
    PostTool,
    /// After LLM response — response inspection, safety checks.
    #[serde(rename = "on_llm_end")]
    OnLlmEnd,
    /// After runtime, before delivery — response filtering, compliance, redaction.
    PostTurn,
    /// Fail-closed constitutional check — gates the turn on constitutional policy.
    #[serde(rename = "constitutional_gate")]
    ConstitutionalGate,
    /// Before delivery to client/channel — formatting, channel-specific transforms.
    PreDeliver,
    /// After delivery confirmed — analytics, notification triggers.
    PostDeliver,
    /// Before durable memory write — content policy, PII filtering.
    PreMemoryWrite,
    /// On session state machine transition — lifecycle, cleanup, notification.
    OnSessionTransition,
    /// When HITL approval triggered — routing to approver, escalation.
    OnApprovalRequest,
    /// When scheduled/triggered workflow fires — gating, context injection.
    OnWorkflowTrigger,
    /// When a change artifact is proposed — review gates, policy checks.
    #[serde(rename = "on_change_artifact_proposed")]
    OnChangeArtifactProposed,
}

impl HookPoint {
    /// All hook points in lifecycle order.
    pub const ALL: &[HookPoint] = &[
        HookPoint::PreRoute,
        HookPoint::PostRoute,
        HookPoint::PreTurn,
        HookPoint::ContextPersona,
        HookPoint::ContextMemory,
        HookPoint::ContextSkill,
        HookPoint::ContextTool,
        HookPoint::OnLlmStart,
        HookPoint::PreTool,
        HookPoint::PostTool,
        HookPoint::OnLlmEnd,
        HookPoint::PostTurn,
        HookPoint::ConstitutionalGate,
        HookPoint::PreDeliver,
        HookPoint::PostDeliver,
        HookPoint::PreMemoryWrite,
        HookPoint::OnSessionTransition,
        HookPoint::OnApprovalRequest,
        HookPoint::OnWorkflowTrigger,
        HookPoint::OnChangeArtifactProposed,
    ];
}

// ── Hook Chain ───────────────────────────────────────────────────────────────

/// A named chain of hook instances that execute sequentially at a hook point.
/// SPEC-hooks: one hook's output becomes the next hook's input. The chain
/// can short-circuit with Reject or Redirect.
///
/// Configured via YAML manifests (SPEC-config HookChain kind).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookChain {
    /// Unique name for this chain (e.g., "content-filter-chain").
    pub name: String,
    /// The hook point this chain fires at.
    pub point: HookPoint,
    /// Ordered list of hook instances in the chain.
    pub hooks: Vec<HookInstance>,
    /// Total chain timeout in milliseconds.
    #[serde(default = "default_chain_timeout_ms")]
    pub timeout_ms: u64,
    /// If true, chain continues when a hook fails. If false, failure = rejection.
    /// SPEC-hooks: fail_open vs fail_closed determines resilience vs safety.
    #[serde(default)]
    pub fail_open: bool,
}

fn default_chain_timeout_ms() -> u64 {
    5000
}

/// A single hook instance within a chain — references a WASM module with config.
/// SPEC-hooks: hooks are parameterized via per-instance config blocks, never hardcoded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInstance {
    /// Reference to the hook (WASM module name or path).
    pub hook_ref: String,
    /// Per-instance configuration — passed to Hook::init().
    #[serde(default)]
    pub config: serde_json::Value,
    /// Toggle without removing from chain.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

// ── Permission Overrides ─────────────────────────────────────────────────────

/// Permission adjustments a hook can request for downstream hooks in the same
/// chain (and, by extension, for the operation being gated by the chain).
///
/// SPEC-hooks: hooks occasionally need to grant or revoke capability tokens
/// (named permission strings) based on runtime context — for example, a policy
/// hook might grant `"tool:bash:write"` if the principal is in an on-call
/// group, or revoke `"memory:write"` if PII is detected in the input.
///
/// # Merge semantics
///
/// When a hook returns overrides, the [`ChainExecutor`] merges them into
/// [`HookContext::permissions`] with **additive union for grants, set
/// subtraction for revokes**:
///
/// - Each hook's `additional_permissions` are UNIONed with the currently-active
///   permission set — grants compound across hooks.
/// - Each hook's `revoked_permissions` are then removed from that set — a
///   revoke always wins over a grant **from an earlier hook in the same chain**.
/// - A later hook can re-grant a previously revoked permission.
///
/// Rationale: hook chains are cooperative pipelines. A preceding hook's grant
/// expresses "this context is safe for X"; a subsequent hook's revoke expresses
/// "but not after I've looked at it". Last-write-wins on a *per-permission*
/// basis preserves both intents. Full last-write-wins on the whole struct
/// would let a downstream hook accidentally erase a grant it never considered.
///
/// TTL, when set on an override, is advisory — the executor tracks the
/// set-wall-clock at which each grant expires; permissions older than their
/// TTL are pruned before the next hook runs.
///
/// `[`ChainExecutor`]: crate::hook
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionOverrides {
    /// Capability tokens to grant. Unioned into the active set.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_permissions: Vec<String>,

    /// Capability tokens to revoke. Removed from the active set after grants.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub revoked_permissions: Vec<String>,

    /// Optional expiry for the granted permissions. When `Some`, the executor
    /// will prune grants older than `ttl` before invoking the next hook.
    /// Revokes are *not* subject to TTL — they are immediate and durable.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "duration_millis_opt")]
    pub ttl: Option<Duration>,
}

impl PermissionOverrides {
    /// Shorthand for a grant-only override.
    pub fn grant<I, S>(perms: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            additional_permissions: perms.into_iter().map(Into::into).collect(),
            revoked_permissions: Vec::new(),
            ttl: None,
        }
    }

    /// Shorthand for a revoke-only override.
    pub fn revoke<I, S>(perms: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            additional_permissions: Vec::new(),
            revoked_permissions: perms.into_iter().map(Into::into).collect(),
            ttl: None,
        }
    }

    /// Attach a TTL to this override.
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    /// True if there are no grants and no revokes — merging is a no-op.
    pub fn is_empty(&self) -> bool {
        self.additional_permissions.is_empty() && self.revoked_permissions.is_empty()
    }
}

/// Serde helper for `Option<Duration>` as millis on the wire.
mod duration_millis_opt {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(
        value: &Option<Duration>,
        ser: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(d) => ser.serialize_some(&(d.as_millis() as u64)),
            None => ser.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        de: D,
    ) -> Result<Option<Duration>, D::Error> {
        let opt = Option::<u64>::deserialize(de)?;
        Ok(opt.map(Duration::from_millis))
    }
}

// ── Hook Execution Types ─────────────────────────────────────────────────────

/// The result of executing a single hook in a chain.
/// SPEC-hooks: Continue passes (possibly modified) context to next hook.
/// Reject and Redirect short-circuit the chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HookResult {
    /// Pass through (possibly modified context) to next hook in chain.
    Continue {
        /// Modified context — merged into the ongoing HookContext.
        #[serde(default)]
        context_updates: HashMap<String, serde_json::Value>,
        /// Optionally replace the input value for downstream hooks.
        #[serde(skip_serializing_if = "Option::is_none")]
        updated_input: Option<serde_json::Value>,
        /// Capability-token adjustments the hook wants applied before the
        /// next hook runs. Merged into [`HookContext::permissions`] per the
        /// additive-grant / subtractive-revoke rules documented on
        /// [`PermissionOverrides`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permission_overrides: Option<PermissionOverrides>,
    },
    /// Short-circuit: block the operation.
    Reject {
        /// Human-readable reason for rejection.
        reason: String,
        /// Machine-readable error code.
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    /// Short-circuit: reroute to a different target.
    Redirect {
        /// Target to redirect to (agent ID, URL, etc.).
        target: String,
        /// Reason for redirect.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl HookResult {
    /// Create a Continue result with no modifications.
    pub fn pass() -> Self {
        HookResult::Continue {
            context_updates: HashMap::new(),
            updated_input: None,
            permission_overrides: None,
        }
    }

    /// Create a Continue result with context modifications.
    pub fn pass_with(updates: HashMap<String, serde_json::Value>) -> Self {
        HookResult::Continue {
            context_updates: updates,
            updated_input: None,
            permission_overrides: None,
        }
    }

    /// Create a Continue result that transforms the input value passed to
    /// downstream hooks in the same chain.
    pub fn pass_with_input(updated_input: serde_json::Value) -> Self {
        HookResult::Continue {
            context_updates: HashMap::new(),
            updated_input: Some(updated_input),
            permission_overrides: None,
        }
    }

    /// Create a Continue result that requests permission overrides for
    /// downstream hooks in the same chain.
    pub fn pass_with_permissions(overrides: PermissionOverrides) -> Self {
        HookResult::Continue {
            context_updates: HashMap::new(),
            updated_input: None,
            permission_overrides: Some(overrides),
        }
    }

    /// Create a Reject result.
    pub fn reject(reason: impl Into<String>) -> Self {
        HookResult::Reject {
            reason: reason.into(),
            code: None,
        }
    }

    /// Create a Reject result with an error code.
    pub fn reject_with_code(reason: impl Into<String>, code: impl Into<String>) -> Self {
        HookResult::Reject {
            reason: reason.into(),
            code: Some(code.into()),
        }
    }

    /// Create a Redirect result.
    pub fn redirect(target: impl Into<String>) -> Self {
        HookResult::Redirect {
            target: target.into(),
            reason: None,
        }
    }

    /// Whether this result allows the chain to continue.
    pub fn is_continue(&self) -> bool {
        matches!(self, HookResult::Continue { .. })
    }

    /// Whether this result short-circuits the chain.
    pub fn is_terminal(&self) -> bool {
        !self.is_continue()
    }
}

/// Context passed to hooks — contains event, session, tool call, and metadata.
/// SPEC-hooks: the context shape varies by hook point, but this struct covers
/// all fields. Hooks read what they need and ignore the rest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookContext {
    /// The hook point being executed.
    pub point: HookPoint,
    /// Event being processed (present for route/turn hooks).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<serde_json::Value>,
    /// Session info (present for turn/tool/memory hooks).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<serde_json::Value>,
    /// Tool call being executed (present for pre_tool/post_tool).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<serde_json::Value>,
    /// Tool result (present for post_tool only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<serde_json::Value>,
    /// Principal performing the action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal: Option<serde_json::Value>,
    /// Arbitrary metadata — hooks can read and modify this.
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Change artifact associated with this hook invocation (present for evolution hooks).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_artifact: Option<crate::evolution::ChangeArtifactId>,
}

/// Reserved metadata key: JSON array of active permission tokens accumulated
/// from upstream hooks' [`PermissionOverrides`] merges in the current chain.
///
/// Stored in [`HookContext::metadata`] instead of a dedicated field so that
/// `HookContext` remains struct-literal-compatible with existing downstream
/// callers. Prefer the typed accessors
/// [`HookContext::active_permissions`] / [`HookContext::apply_permission_overrides`].
pub const HOOK_CTX_PERMISSIONS_KEY: &str = "__sera_hook_permissions";

/// Reserved metadata key: the most recent `updated_input` value produced by
/// an upstream hook in the current chain. Use
/// [`HookContext::updated_input`] to read it.
pub const HOOK_CTX_UPDATED_INPUT_KEY: &str = "__sera_hook_updated_input";

impl HookContext {
    /// Create a minimal context for a given hook point.
    pub fn new(point: HookPoint) -> Self {
        Self {
            point,
            event: None,
            session: None,
            tool_call: None,
            tool_result: None,
            principal: None,
            metadata: HashMap::new(),
            change_artifact: None,
        }
    }

    /// Apply context updates from a Continue result.
    pub fn apply_updates(&mut self, updates: HashMap<String, serde_json::Value>) {
        self.metadata.extend(updates);
    }

    /// Return the active capability-token set accumulated from upstream
    /// hooks' [`PermissionOverrides`]. Empty when no overrides have been
    /// applied in this chain.
    ///
    /// This is *not* the authoritative permission set for authorization —
    /// the gateway still performs its own check — but lets downstream hooks
    /// in the same chain observe what previous hooks have granted or revoked.
    pub fn active_permissions(&self) -> Vec<String> {
        match self.metadata.get(HOOK_CTX_PERMISSIONS_KEY) {
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// True iff `perm` is present in the active permission set.
    pub fn has_permission(&self, perm: &str) -> bool {
        match self.metadata.get(HOOK_CTX_PERMISSIONS_KEY) {
            Some(serde_json::Value::Array(arr)) => {
                arr.iter().any(|v| v.as_str() == Some(perm))
            }
            _ => false,
        }
    }

    /// Merge a [`PermissionOverrides`] into the active permission set.
    ///
    /// Semantics (see [`PermissionOverrides`] for the full rationale):
    /// 1. `additional_permissions` are UNIONed in.
    /// 2. `revoked_permissions` are then REMOVED.
    ///
    /// Returns `true` if the set changed.
    pub fn apply_permission_overrides(&mut self, overrides: &PermissionOverrides) -> bool {
        let mut set: Vec<String> = self.active_permissions();
        let before = set.len();
        for grant in &overrides.additional_permissions {
            if !set.iter().any(|p| p == grant) {
                set.push(grant.clone());
            }
        }
        set.retain(|p| !overrides.revoked_permissions.contains(p));
        let changed = set.len() != before
            || !overrides.additional_permissions.is_empty()
            || !overrides.revoked_permissions.is_empty();
        self.metadata.insert(
            HOOK_CTX_PERMISSIONS_KEY.to_string(),
            serde_json::Value::Array(set.into_iter().map(serde_json::Value::String).collect()),
        );
        changed
    }

    /// Return the most-recent `updated_input` value propagated through the
    /// current chain, if any. Downstream hooks should prefer this value over
    /// the original input stored in `event` / `tool_call` when set.
    pub fn updated_input(&self) -> Option<&serde_json::Value> {
        self.metadata.get(HOOK_CTX_UPDATED_INPUT_KEY)
    }

    /// Set the `updated_input` slot in the chain-scoped metadata. Typically
    /// only the executor calls this; hooks return
    /// `HookResult::pass_with_input` instead.
    pub fn set_updated_input(&mut self, value: serde_json::Value) {
        self.metadata
            .insert(HOOK_CTX_UPDATED_INPUT_KEY.to_string(), value);
    }
}

/// Metadata describing a hook module's identity and capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookMetadata {
    /// Unique name of the hook module.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Semantic version string.
    pub version: String,
    /// Which hook points this module can be used at.
    pub supported_points: Vec<HookPoint>,
    /// Author/organization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

// ── WASM Runtime Configuration ───────────────────────────────────────────────

/// Configuration for the WASM hook runtime.
/// SPEC-hooks: fuel metering, memory caps, timeouts for safe execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmConfig {
    /// Computation budget per hook invocation (wasmtime fuel units).
    #[serde(default = "default_fuel_limit")]
    pub fuel_limit: u64,
    /// Memory cap per hook instance in MB.
    #[serde(default = "default_memory_limit_mb")]
    pub memory_limit_mb: u32,
    /// Per-hook execution timeout in milliseconds.
    #[serde(default = "default_hook_timeout_ms")]
    pub timeout_ms: u64,
    /// Watch hook directory for .wasm file changes and hot-reload.
    #[serde(default)]
    pub hot_reload: bool,
    /// Directory where .wasm hook modules are stored.
    #[serde(default = "default_hook_directory")]
    pub hook_directory: String,
}

fn default_fuel_limit() -> u64 {
    1_000_000
}

fn default_memory_limit_mb() -> u32 {
    64
}

fn default_hook_timeout_ms() -> u64 {
    1000
}

fn default_hook_directory() -> String {
    "hooks".to_string()
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            fuel_limit: default_fuel_limit(),
            memory_limit_mb: default_memory_limit_mb(),
            timeout_ms: default_hook_timeout_ms(),
            hot_reload: false,
            hook_directory: default_hook_directory(),
        }
    }
}

// ── Chain Execution Result ───────────────────────────────────────────────────

/// The result of executing an entire hook chain.
#[derive(Debug, Clone)]
pub struct ChainResult {
    /// Final context after all hooks ran.
    pub context: HookContext,
    /// The terminal result (Continue for full chain, or the short-circuit result).
    pub outcome: HookResult,
    /// Number of hooks that executed before the chain completed.
    pub hooks_executed: usize,
    /// Total execution time in milliseconds.
    pub duration_ms: u64,
    /// Input transformation from a Continue result, if any hook modified the input.
    /// Last hook in the chain that sets this wins — subsequent hooks in the same
    /// chain receive the transformed input via `HookContext`.
    pub updated_input: Option<serde_json::Value>,
}

impl ChainResult {
    /// Whether the chain completed without short-circuiting.
    pub fn is_success(&self) -> bool {
        self.outcome.is_continue()
    }

    /// Whether the chain was rejected.
    pub fn is_rejected(&self) -> bool {
        matches!(self.outcome, HookResult::Reject { .. })
    }

    /// Whether the chain was redirected.
    pub fn is_redirected(&self) -> bool {
        matches!(self.outcome, HookResult::Redirect { .. })
    }

    /// Whether the chain was aborted by an external cancellation signal.
    ///
    /// Cancellation is modeled via the `Reject` outcome with the reserved
    /// machine-readable code `"chain_aborted"`; see
    /// `sera_hooks::HookCancellation` for the signal mechanism.
    pub fn is_aborted(&self) -> bool {
        matches!(
            &self.outcome,
            HookResult::Reject { code: Some(c), .. } if c == CHAIN_ABORTED_CODE
        )
    }
}

/// Reserved error code on `HookResult::Reject` that indicates the chain was
/// cancelled by an external [`HookCancellation`](crate::hook) signal rather
/// than rejected by a hook's business logic.
pub const CHAIN_ABORTED_CODE: &str = "chain_aborted";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_point_count() {
        assert_eq!(HookPoint::ALL.len(), 20, "spec defines 20 hook points");
    }

    #[test]
    fn hook_point_serde() {
        let json = serde_json::to_string(&HookPoint::PreRoute).unwrap();
        assert_eq!(json, "\"pre_route\"");

        let parsed: HookPoint = serde_json::from_str("\"post_tool\"").unwrap();
        assert_eq!(parsed, HookPoint::PostTool);
    }

    #[test]
    fn hook_point_all_variants_serde() {
        for point in HookPoint::ALL {
            let json = serde_json::to_string(point).unwrap();
            let parsed: HookPoint = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, point);
        }
    }

    #[test]
    fn hook_chain_serde_roundtrip() {
        let chain = HookChain {
            name: "content-filter".to_string(),
            point: HookPoint::PreRoute,
            hooks: vec![
                HookInstance {
                    hook_ref: "rate-limiter".to_string(),
                    config: serde_json::json!({"requests_per_minute": 60}),
                    enabled: true,
                },
                HookInstance {
                    hook_ref: "content-filter".to_string(),
                    config: serde_json::json!({"blocked_patterns": ["spam"]}),
                    enabled: true,
                },
            ],
            timeout_ms: 5000,
            fail_open: false,
        };

        let json = serde_json::to_string(&chain).unwrap();
        let parsed: HookChain = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "content-filter");
        assert_eq!(parsed.hooks.len(), 2);
        assert_eq!(parsed.point, HookPoint::PreRoute);
    }

    #[test]
    fn hook_instance_defaults() {
        let json = r#"{"hook_ref":"my-hook"}"#;
        let instance: HookInstance = serde_json::from_str(json).unwrap();
        assert!(instance.enabled);
        assert_eq!(instance.config, serde_json::Value::Null);
    }

    #[test]
    fn hook_result_continue() {
        let result = HookResult::pass();
        assert!(result.is_continue());
        assert!(!result.is_terminal());
    }

    #[test]
    fn hook_result_continue_with_updates() {
        let mut updates = HashMap::new();
        updates.insert("filtered".to_string(), serde_json::json!(true));
        let result = HookResult::pass_with(updates);
        assert!(result.is_continue());
        if let HookResult::Continue { context_updates, .. } = &result {
            assert!(context_updates.contains_key("filtered"));
        }
    }

    #[test]
    fn hook_result_reject() {
        let result = HookResult::reject("blocked by content filter");
        assert!(result.is_terminal());
        assert!(!result.is_continue());
        if let HookResult::Reject { reason, code } = &result {
            assert_eq!(reason, "blocked by content filter");
            assert!(code.is_none());
        }
    }

    #[test]
    fn hook_result_reject_with_code() {
        let result = HookResult::reject_with_code("rate limited", "RATE_LIMIT_EXCEEDED");
        if let HookResult::Reject { reason, code } = &result {
            assert_eq!(reason, "rate limited");
            assert_eq!(code.as_deref(), Some("RATE_LIMIT_EXCEEDED"));
        }
    }

    #[test]
    fn hook_result_redirect() {
        let result = HookResult::redirect("agent:fallback");
        assert!(result.is_terminal());
        if let HookResult::Redirect { target, .. } = &result {
            assert_eq!(target, "agent:fallback");
        }
    }

    #[test]
    fn hook_result_serde_roundtrip() {
        let cases = vec![
            HookResult::pass(),
            HookResult::reject("denied"),
            HookResult::redirect("elsewhere"),
        ];

        for result in cases {
            let json = serde_json::to_string(&result).unwrap();
            let parsed: HookResult = serde_json::from_str(&json).unwrap();
            // Verify the action tag is preserved
            match (&result, &parsed) {
                (HookResult::Continue { .. }, HookResult::Continue { .. }) => {}
                (HookResult::Reject { reason: a, .. }, HookResult::Reject { reason: b, .. }) => {
                    assert_eq!(a, b);
                }
                (
                    HookResult::Redirect { target: a, .. },
                    HookResult::Redirect { target: b, .. },
                ) => {
                    assert_eq!(a, b);
                }
                _ => panic!("serde changed the variant"),
            }
        }
    }

    #[test]
    fn hook_context_new() {
        let ctx = HookContext::new(HookPoint::PreTool);
        assert_eq!(ctx.point, HookPoint::PreTool);
        assert!(ctx.event.is_none());
        assert!(ctx.tool_call.is_none());
        assert!(ctx.metadata.is_empty());
    }

    #[test]
    fn hook_context_apply_updates() {
        let mut ctx = HookContext::new(HookPoint::PostTool);
        let mut updates = HashMap::new();
        updates.insert("sanitized".to_string(), serde_json::json!(true));
        updates.insert("risk_score".to_string(), serde_json::json!(0.7));
        ctx.apply_updates(updates);
        assert_eq!(ctx.metadata.len(), 2);
        assert_eq!(ctx.metadata["sanitized"], serde_json::json!(true));
    }

    #[test]
    fn hook_metadata_serde() {
        let meta = HookMetadata {
            name: "content-filter".to_string(),
            description: "Filters inappropriate content".to_string(),
            version: "1.0.0".to_string(),
            supported_points: vec![HookPoint::PreRoute, HookPoint::PostTurn],
            author: Some("sera-team".to_string()),
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: HookMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "content-filter");
        assert_eq!(parsed.supported_points.len(), 2);
    }

    #[test]
    fn wasm_config_defaults() {
        let config = WasmConfig::default();
        assert_eq!(config.fuel_limit, 1_000_000);
        assert_eq!(config.memory_limit_mb, 64);
        assert_eq!(config.timeout_ms, 1000);
        assert!(!config.hot_reload);
        assert_eq!(config.hook_directory, "hooks");
    }

    #[test]
    fn wasm_config_serde_with_defaults() {
        let json = r#"{"fuel_limit":500000}"#;
        let config: WasmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.fuel_limit, 500_000);
        assert_eq!(config.memory_limit_mb, 64); // default
        assert_eq!(config.timeout_ms, 1000); // default
    }

    #[test]
    fn chain_result_success() {
        let result = ChainResult {
            context: HookContext::new(HookPoint::PreRoute),
            outcome: HookResult::pass(),
            hooks_executed: 3,
            duration_ms: 12,
            updated_input: None,
        };
        assert!(result.is_success());
        assert!(!result.is_rejected());
        assert!(!result.is_redirected());
    }

    #[test]
    fn chain_result_rejected() {
        let result = ChainResult {
            context: HookContext::new(HookPoint::PreRoute),
            outcome: HookResult::reject("nope"),
            hooks_executed: 1,
            duration_ms: 2,
            updated_input: None,
        };
        assert!(!result.is_success());
        assert!(result.is_rejected());
        assert!(!result.is_redirected());
    }

    #[test]
    fn chain_result_redirected() {
        let result = ChainResult {
            context: HookContext::new(HookPoint::PreRoute),
            outcome: HookResult::redirect("other-agent"),
            hooks_executed: 2,
            duration_ms: 5,
            updated_input: None,
        };
        assert!(!result.is_success());
        assert!(!result.is_rejected());
        assert!(result.is_redirected());
    }

    #[test]
    fn hook_chain_default_timeout() {
        let json = r#"{"name":"test","point":"pre_route","hooks":[]}"#;
        let chain: HookChain = serde_json::from_str(json).unwrap();
        assert_eq!(chain.timeout_ms, 5000);
        assert!(!chain.fail_open);
    }

    #[test]
    fn hook_chain_with_disabled_hook() {
        let chain = HookChain {
            name: "test".to_string(),
            point: HookPoint::PreTool,
            hooks: vec![
                HookInstance {
                    hook_ref: "active-hook".to_string(),
                    config: serde_json::Value::Null,
                    enabled: true,
                },
                HookInstance {
                    hook_ref: "disabled-hook".to_string(),
                    config: serde_json::Value::Null,
                    enabled: false,
                },
            ],
            timeout_ms: 3000,
            fail_open: true,
        };
        let enabled_hooks: Vec<_> = chain.hooks.iter().filter(|h| h.enabled).collect();
        assert_eq!(enabled_hooks.len(), 1);
        assert_eq!(enabled_hooks[0].hook_ref, "active-hook");
    }
}
