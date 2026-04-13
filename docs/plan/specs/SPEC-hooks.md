# SPEC: Hook System (`sera-hooks`)

> **Status:** DRAFT
> **Source:** PRD §5 (all subsections), §14 (invariants 8, 11), plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §6 (wasmtime 43 runtime, `WasiHttpView::send_request` allow-list, NO extism), §10.1 (claw-code `updated_input` on `HookRunResult`, `HookAbortSignal`, subprocess stdin/stdout JSON pattern), §10.2 (Codex 5-hook-point alignment, `HookToolInput`/`HookToolKind` discrimination), §10.3 (Paperclip `PluginEvent` envelope + two-tier internal vs plugin hook bus + anti-spoofing), §10.5 (openclaw 29-hook reference set incl. `subagent_delivery_target` and `InternalHookEvent` vs `PluginHookName` split), §10.8 (NemoClaw `enforcement: enforce | audit` mode), [SPEC-self-evolution](SPEC-self-evolution.md) §5.3 (`constitutional_gate` hook point — fail-closed, no `fail_open`)
> **Crate:** `sera-hooks`
> **Priority:** Phase 1

---

## 1. Overview

The hook system is SERA's **extensibility backbone**. Hooks are chainable WASM-based processing pipelines that fire at defined points throughout the event lifecycle. They enable operators and developers to inject custom logic — content filtering, rate limiting, PII redaction, secret injection, risk assessment, compliance checks — without modifying core code.

Hooks are:
- **Chainable** — one hook's output feeds into the next
- **Parameterized** — each hook instance has its own configuration block
- **Sandboxed** — they run in a WASM runtime with fuel metering and memory caps
- **Short-circuitable** — any hook can `Reject` or `Redirect` to stop the chain

---

## 1a. Layer Assignment — Harness-Side vs Gateway-Side

> **Design decision — 2026-04-13.** Hooks are NOT all equivalent. They split cleanly by which layer owns them and why.

Conflating these two groups leads to incorrect security models — harness-side hooks can never enforce policy because the harness is untrusted cattle; gateway-side hooks must be gateway-owned because they are policy, not convention.

### Harness-side hooks (operational, no security relevance)

These hooks run in the harness process. They affect how the harness formats, assembles, and packages information — not whether something is permitted. A compromised or misbehaving harness can ignore them without violating the system's security invariants.

| Hook Point | Layer | Purpose |
|---|---|---|
| `context_persona` | Harness | How to format persona injection, how to condense when context is full |
| `context_memory` | Harness | Memory tier selection, RAG tuning for context assembly |
| `context_skill` | Harness | Skill filtering, mode transitions |
| `context_tool` | Harness | Tool schema formatting for context injection |
| `pre_turn` | Harness | Context enrichment before context assembly begins |
| `on_llm_start` | Harness | Immediately before the model call — token budget check, warm-up |
| `on_llm_end` | Harness | Immediately after the model call, before tool dispatch — response validation |
| `post_turn` | Harness | After runtime completes, before result is returned to gateway |

### Gateway-side hooks (policy, security-critical)

These hooks run in the gateway process. They enforce policy decisions that the harness cannot override. They apply equally to all connected harnesses — whether the default embedded runtime, a BYOH harness, or an external agent.

| Hook Point | Layer | Purpose |
|---|---|---|
| `constitutional_gate` | Gateway | Fail-closed constitutional invariant enforcement; fires before all others |
| `pre_route` | Gateway | Content filtering, rate limiting, classification — before queue |
| `post_route` | Gateway | Routing override, logging — after routing decision |
| `pre_tool` | **Gateway** | **AuthZ enforcement, secret injection, approval gates** — harness never executes tools |
| `post_tool` | **Gateway** | **Audit, result sanitization, risk assessment of result** |
| `pre_deliver` | Gateway | Final content filtering before delivery to client/channel |
| `on_approval_request` | Gateway | HITL routing — routes to the correct approver, escalation logic |
| `on_session_transition` | Gateway | Lifecycle hooks, cleanup, notification |
| `pre_memory_write` | Gateway | Content policy, PII filtering before durable memory write |
| `on_workflow_trigger` | Gateway | Workflow gating, context injection |
| `on_change_artifact_proposed` | Gateway | Observability, meta-approval routing |

> [!IMPORTANT]
> `pre_tool` and `post_tool` are **gateway-side**. The harness forwards tool call requests to the gateway; the gateway runs these hook chains before and after dispatching to the tool executor. The harness never runs tool hooks directly and cannot bypass them.

---

## 2. Hook Chain Architecture

### 2.1 Chain Structure

```rust
pub struct HookChain {
    pub name: String,
    pub hooks: Vec<HookInstance>,       // Ordered — output of N feeds into N+1
    pub timeout: Duration,              // Total chain timeout
    pub fail_open: bool,                // If a hook fails: true = continue, false = reject
}

pub struct HookInstance {
    pub hook_ref: HookRef,              // Reference to the WASM module
    pub config: serde_json::Value,      // Per-instance configuration (parameters)
    pub enabled: bool,                  // Can be toggled without removing from chain
}
```

### 2.2 Hook Trait (WASM Interface)

```rust
pub trait Hook {
    fn metadata(&self) -> HookMetadata;
    /// Initialize with configuration — called once on load
    fn init(&mut self, config: serde_json::Value) -> Result<(), HookError>;
    /// Execute with context — called per invocation
    fn execute(&self, ctx: HookContext) -> HookResult;
}

/// Hook result — hooks are input transformers, not just observers.
/// Source: SPEC-dependencies §10.1 claw-code `HookRunResult`.
pub struct HookResult {
    pub outcome: HookOutcome,
    pub messages: Vec<String>,                       // Diagnostic messages surfaced to audit
    pub denied: bool,
    pub permission_overrides: Option<PermissionOverrides>,
    /// If Some, rewrites the tool call's input before execution.
    /// This is the key insight from claw-code: hooks can TRANSFORM inputs, not just gate them.
    pub updated_input: Option<serde_json::Value>,
}

pub enum HookOutcome {
    Continue(HookContext),       // Pass through (possibly modified) to next in chain
    Reject(RejectReason),        // Short-circuit: block the event/action
    Redirect(RedirectTarget),    // Short-circuit: reroute to different session/agent
}

/// Thread-safe cancellation primitive for async hook chains.
/// Source: SPEC-dependencies §10.1 claw-code `HookAbortSignal`.
pub struct HookAbortSignal {
    inner: Arc<AtomicBool>,
}

impl HookAbortSignal {
    pub fn new() -> Self { /* ... */ }
    pub fn abort(&self) { /* ... */ }
    pub fn is_aborted(&self) -> bool { /* ... */ }
}
```

### 2.3 Hook Context

The `HookContext` carries the current event/action data and accumulated state through the chain. Each hook can read and modify it.

```rust
pub struct HookContext {
    pub event: Option<Event>,                     // Present for event-related hooks
    pub turn: Option<TurnContext>,                // Present for turn-related hooks
    pub tool_call: Option<ToolCall>,              // Present for tool-related hooks
    pub tool_input: HookToolInput,                // Discriminated by HookToolKind (codex pattern)
    pub principal: PrincipalRef,
    pub session: Option<SessionRef>,
    /// If set, the current event is part of a self-evolution flow.
    /// Hooks can inspect this and apply change-artifact-aware policies.
    /// Source: SPEC-self-evolution §5.3.
    pub change_artifact: Option<ChangeArtifactId>,
    pub abort_signal: HookAbortSignal,
    pub metadata: HashMap<String, serde_json::Value>,  // Accumulated chain state
}

/// Hooks discriminate tool kinds so different tools can get different policies in one hook.
/// Source: SPEC-dependencies §10.2 Codex `HookToolInput` + `HookToolKind`.
pub enum HookToolKind {
    Shell,
    Patch,
    Mcp { server: String },
    WebSearch,
    FileRead,
    FileWrite,
    Memory,
    Custom(String),
}

pub struct HookToolInput {
    pub kind: HookToolKind,
    pub payload: serde_json::Value,
}
```

### 2.4 Constitutional Gate (NEW — fail-closed)

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §5.3, §6.

The `constitutional_gate` hook point is fired **before any other hook chain** on any Submission carrying a `ChangeArtifactId`. Its chain:

- Always uses `fail_open: false` — the setting is **compiled in and cannot be overridden** at runtime
- Reads the active `ConstitutionalRule` set from `sera_meta::constitution::CONSTITUTIONAL_RULES`
- Rejects any event whose Change Artifact would violate an active rule (tool-loss, approval self-loop, audit-log mutation, constitutional self-modification, etc.)
- Emits a high-priority audit event on every rejection

Chains at this point may not `Redirect` — only `Continue` or `Reject` — because constitutional gates are strictly pre-dispatch. Redirects would re-enter the dispatch pipeline bypassing the gate.

### 2.5 Two-Tier Hook Bus

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.3 Paperclip `PluginEvent` envelope + §10.5 openclaw `InternalHookEvent` vs `PluginHookName` split.

SERA maintains **two separate hook buses** that share shape but differ in access control and lifetime:

| Bus | Scope | Visibility | Access |
|---|---|---|---|
| **InternalHookBus** | Gateway-internal signaling (session transitions, lifecycle, dispatch) | Not visible to plugins | Only core crates can subscribe |
| **PluginHookBus** | Plugin-facing contract (content filtering, tool policy, observability, self-evolution notifications) | Visible to WASM plugins via `sera-plugin-sdk` | Versioned contract; governed by SPEC-versioning |

Separating the two means:

1. **Internal-only events** (like `session_transition_validated`, `harness_supports_negotiated`) never appear in the plugin contract and can change freely without plugin API breakage.
2. **The plugin contract** is a stable, versioned surface — plugins compiled against an old contract keep working across internal refactors.
3. **Plugins cannot spoof internal events** — the internal bus is not exposed via the plugin SDK at all.

The `PluginEvent` envelope shape is:

```rust
pub struct PluginEvent {
    pub event_id: EventId,
    pub event_type: String,                       // e.g. "issue.created", "plugin.acme.custom"
    pub correlation_id: Option<CorrelationId>,    // For tracing DAG execution chains (Paperclip gap filled here)
    pub circle_id: Option<CircleId>,
    pub session_key: Option<SessionKey>,
    pub occurred_at: DateTime<Utc>,
    pub entity_id: Option<String>,
    pub entity_type: Option<String>,
    pub payload: serde_json::Value,
    pub actor_type: ActorType,                    // Principal | Plugin | System
    pub actor_id: String,
}

/// Anti-spoofing invariant: plugins cannot emit events with an event_type starting with "plugin."
/// unless the prefix matches their own registered namespace.
pub fn validate_plugin_event_namespace(plugin: &PluginRef, event: &PluginEvent) -> Result<(), SpoofError>;
```

### 2.6 Subprocess Hooks (Language-Agnostic)

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.1 claw-code subprocess pattern.

In addition to WASM hooks, SERA supports **subprocess hooks** for language-agnostic extension. A subprocess hook is launched as a child process; SERA pipes the hook context as JSON to stdin and reads a structured `HookRunResult` from stdout. This covers use cases where WASM is impractical (legacy shell scripts, Python/Node scripts, language-specific SDKs not yet componentized).

```rust
pub struct CommandHook {
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub timeout: Duration,
}
```

Subprocess hooks share the same `HookResult` return shape as WASM hooks — including `updated_input` — so a chain can mix WASM and subprocess hooks freely.

---

## 3. Hook Points (Comprehensive)

| Hook Point | Fires When | Use Cases | Codex alignment |
|---|---|---|---|
| **`constitutional_gate`** | Before every other hook, on any Submission carrying a Change Artifact | Constitutional invariant enforcement (§2.4) | — |
| `pre_route` | After event ingress, before queue | Content filtering, rate limiting, classification | — |
| `post_route` | After routing decision, before enqueue | Routing override, logging | — |
| `session_start` | When a session enters `Created` | Session-level observability, priming | `SessionStart` |
| `pre_turn` | After queue dequeue, before context assembly | Context enrichment, policy injection | `UserPromptSubmit` |
| `context_persona` | During persona assembly step | Persona switching, mode injection | — |
| `context_memory` | During memory injection step | Memory tier selection, RAG tuning | — |
| `context_skill` | During skill injection step | Skill filtering, mode transitions | — |
| `context_tool` | During tool injection step | Tool filtering, capability policy | — |
| **`on_llm_start`** | Immediately before the model call | Token budget check, reasoning model warm-up | — (new) |
| **`on_llm_end`** | Immediately after the model call, before tool dispatch | Response validation, reasoning extraction | — (new) |
| `pre_tool` | Before tool execution | Approval gates, argument validation, **secret injection**, `updated_input` rewriting | `PreToolUse` |
| `post_tool` | After tool execution | Result sanitization, audit, **risk assessment** | `PostToolUse` |
| **`subagent_delivery_target`** | Between subagent completion and parent session delivery | Result transformation, fan-in aggregation (integration point for `ResultAggregator`) | — |
| `post_turn` | After runtime, before response delivery | Response filtering, compliance, redaction | `Stop` |
| `pre_deliver` | Before response delivery to client/channel | Final formatting, channel-specific transforms | — |
| `post_deliver` | After response delivery confirmed | Analytics, notification triggers | — |
| `pre_memory_write` | Before durable memory write | Content policy, PII filtering | — |
| `on_session_transition` | On session state machine transition | Lifecycle hooks, cleanup, notification | — |
| `on_approval_request` | When HITL approval is triggered | Routing to correct approver, escalation logic | — |
| `on_workflow_trigger` | When a scheduled/triggered workflow fires | Workflow gating, context injection | — |
| `on_change_artifact_proposed` | When a self-evolution Change Artifact is proposed | Observability, meta-approval routing (SPEC-self-evolution §5.3) | — |

### 3.1 Per-Hook Enforcement Mode

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.8 NemoClaw `enforcement: enforce | audit`.

Every hook instance in every chain carries an `enforcement` mode:

| Mode | Behavior |
|---|---|
| `enforce` (default) | Hook decisions are binding. `Reject` blocks the action. |
| `audit` | Hook decisions are logged but **not enforced**. Enables incremental rollout of new policies before promoting to enforce. |

Audit mode is essential for operator confidence when introducing new security policies. A `PolicyDraftAdvisor` (see SPEC-tools §6a, NemoClaw AI-assisted policy advisor pattern) can aggregate audit-mode denials and propose promoting specific rules to `enforce`.

---

## 4. Hook Configuration

Hooks are configured in chains, per hook point, with per-instance parameters:

```yaml
hooks:
  chains:
    pre_route:
      - hook: "content-filter"
        config:
          blocked_patterns: ["spam_regex_1", "spam_regex_2"]
          action: "reject"
          log_level: "warn"
      - hook: "rate-limiter"
        config:
          requests_per_minute: 60
          burst: 10
          scope: "per-principal"        # per-principal | per-session | global

    pre_tool:
      - hook: "secret-injector"
        config:
          provider: "vault"
          mappings:
            GITHUB_TOKEN: "secrets/github/token"
            SLACK_WEBHOOK: "secrets/slack/webhook"
      - hook: "risk-checker"
        config:
          max_risk_level: "write"
          require_approval_above: "execute"

    post_turn:
      - hook: "pii-redactor"
        config:
          patterns: ["email", "phone", "ssn"]
          action: "mask"                # mask | remove | flag
```

The same hook module can appear multiple times in the same chain (or different chains) with different configurations. Each instance is independently configured via `init()`.

Hook chains are defined as `HookChain` config manifests (see [SPEC-config](SPEC-config.md) §2.2):

```yaml
apiVersion: sera.dev/v1
kind: HookChain
metadata:
  name: "pre-route-default"
spec:
  hook_point: "pre_route"
  hooks:
    - hook: "content-filter"
      config:
        blocked_patterns: ["spam_regex_1"]
        action: "reject"
    - hook: "rate-limiter"
      config:
        requests_per_minute: 60
```

---

## 5. WASM Runtime

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §6 — `wasmtime` 43 with Component Model + WASI Preview 2, `wasmtime-wasi-http` allow-list. **`extism` is explicitly rejected** (pinned to wasmtime <31, non-WIT proprietary ABI). See SPEC-dependencies §6 for the full rationale.

### 5.1 Runtime Configuration

```rust
pub struct WasmConfig {
    pub fuel_limit: u64,              // Computation budget per hook invocation
    pub memory_limit_mb: u32,         // Memory cap per hook instance (via wasmtime ResourceLimiter)
    pub timeout: Duration,            // Per-hook timeout (wraps async host calls via tokio::time::timeout)
    pub hot_reload: bool,             // Watch hook directory via `notify` 8.2 crate
    pub hook_directory: PathBuf,      // Where .wasm files live
    pub allow_list: HostAllowList,    // Per-chain outbound HTTP allow list (§5.4)
}
```

### 5.2 Sandbox Guarantees

- **Fuel metering** — each hook invocation has a computation budget via `wasmtime::Store::set_fuel()`; exceeded = abort
- **Memory cap** — each hook instance has a memory ceiling via `ResourceLimiter`; exceeded = abort
- **Timeout** — per-hook and per-chain timeouts; async host calls wrapped with `tokio::time::timeout`
- **No ambient host access** — hooks cannot access the filesystem or network **except through the proxied `wasi:http` surface** (§5.4)
- **Deterministic termination** — fuel + timeout ensure hooks always terminate

### 5.3 Fail-Open vs. Fail-Closed

Each chain has a `fail_open` setting:
- `fail_open: true` — if a hook errors, skip it and continue the chain (suitable for non-critical hooks like analytics)
- `fail_open: false` — if a hook errors, the entire chain fails and the action is rejected (suitable for security hooks)

**Exception:** the `constitutional_gate` chain (§2.4) ALWAYS runs fail-closed. The `fail_open` field is ignored for this chain and cannot be set via config.

### 5.4 Host API Proxying — `wasi:http` Allow List

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §6.1.

Hooks call `wasi:http/outgoing-handler.handle(...)`. The host implements `wasmtime_wasi_http::WasiHttpView::send_request`, inspects the target URL against SERA's per-chain allow-list, and either forwards the request through SERA's egress proxy or returns `HttpError::Forbidden`.

```rust
impl WasiHttpView for SeraHookState {
    async fn send_request(
        &mut self,
        request: OutgoingRequest,
        config: RequestOptions,
    ) -> Result<IncomingResponse, HttpError> {
        let url = request.uri();
        if !self.allow_list.permits(&url) {
            self.audit.emit_denied(url.clone(), DenialReason::NotInAllowList);
            return Err(HttpError::Forbidden);
        }
        self.sera_egress_client.execute(request, config).await
    }
}
```

This is the **only** outbound-network path available to hooks. Raw socket access is never granted. The allow-list is part of the `HookChain` configuration and can be hot-reloaded.

### 5.5 Known caveats

- **Fuel metering is bytecode-only.** Hooks blocked in a host call are not subject to fuel; host functions wrap with `tokio::time::timeout`.
- **`Store<T>` is `!Send`.** Thread-pool dispatch requires either a per-thread store pool or `Arc<Mutex<Store>>`.
- **Fuel counted per basic block** ([wasmtime#4109](https://github.com/bytecodealliance/wasmtime/issues/4109)) — slight overshoot is possible; affects precision, not security.

---

## 6. Hook Authoring

### 6.1 Toolchains

Hooks are compiled to **WASM Components** using standard toolchains:

| Language | Toolchain | SDK Crate |
|---|---|---|
| Rust | Standard `cargo` + `wasm32-wasip2` target | `sera-hook-sdk` |
| Python | `componentize-py` | `sera-hook-sdk-python` |
| TypeScript | `ComponentizeJS` / `jco` | `sera-hook-sdk-ts` |

### 6.2 SDK Crates

SERA provides **lightweight interface SDK crates** that define the `Hook` trait/interface for each language. Authors implement the trait and compile to WASM.

These SDKs are:
- **Separate publishable crates** (on crates.io / npm / pypi)
- **Same monorepo** for now (under `sdk/hooks/` or similar)
- Minimal dependencies — just the trait definition, context types, and result types

### 6.3 Hook Distribution

Hooks are distributed as `.wasm` files placed in the configured `hook_directory`. They can be:
- Shipped with SERA (built-in hooks)
- Added manually by operators
- Hot-reloaded when the directory changes (if `hot_reload: true`)

---

## 7. Invariants

| # | Invariant | Enforcement |
|---|---|---|
| 8 | Hooks are sandboxed | WASM runtime with fuel metering and memory caps |
| 11 | Hook chains are ordered | Config-driven ordering; execution follows chain order |

---

## 8. Configuration

```yaml
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: "my-sera"
spec:
  hooks:
    wasm:
      fuel_limit: 1000000
      memory_limit_mb: 64
      timeout_ms: 100
      hot_reload: true
      hook_directory: "./hooks"
```

Individual hook chains are defined as separate `HookChain` manifests (see §4 above).

---

## 9. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Hook chains fire at gateway routing points |
| `sera-runtime` | [SPEC-runtime](SPEC-runtime.md) | Hook chains fire at turn lifecycle points |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | Pre/post tool hooks |
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Secret-injector hook pattern |
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | Hooks can contribute authz checks |
| `sera-config` | [SPEC-config](SPEC-config.md) | Hook chain config as HookChain manifests |
| Versioning | [SPEC-versioning](SPEC-versioning.md) | Hook WIT interface versioning (§6) |

---

## 10. Open Questions

1. **Hook-to-hook state passing** — Can hooks pass state to subsequent hooks in the chain beyond modifying HookContext? Is the `metadata` HashMap sufficient?
2. **Hook capabilities (WASM component model)** — Which host capabilities can be granted to hooks? Network? File system? Specific APIs?
3. **Hook lifecycle events** — Are hooks notified of system lifecycle events (startup, shutdown, config change)?
4. ~~**Hook versioning**~~ — Resolved: See [SPEC-versioning](SPEC-versioning.md) §6. WIT package versioning with compatibility matrix.
5. **Hook testing** — What's the DX for testing hooks locally before deploying?

---

## 11. Success Criteria

| Metric | Target |
|---|---|
| Hook chain overhead | < 5ms per WASM hook invocation, chains additive |
| Extension authoring | < 1 hour for a WASM hook |
