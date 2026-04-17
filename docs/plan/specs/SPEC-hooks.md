# SPEC: Hook System (`sera-hooks`)

> **Status:** DRAFT
> **Source:** PRD §5 (all subsections), §14 (invariants 8, 11), plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §6 (wasmtime 43 runtime, `WasiHttpView::send_request` allow-list, NO extism), §10.1 (claw-code `updated_input` on `HookRunResult`, `HookAbortSignal`, subprocess stdin/stdout JSON pattern), §10.2 (Codex 5-hook-point alignment, `HookToolInput`/`HookToolKind` discrimination), §10.3 (Paperclip `PluginEvent` envelope + two-tier internal vs plugin hook bus + anti-spoofing), §10.5 (openclaw 29-hook reference set incl. `subagent_delivery_target` and `InternalHookEvent` vs `PluginHookName` split), §10.8 (NemoClaw `enforcement: enforce | audit` mode), [SPEC-self-evolution](SPEC-self-evolution.md) §5.3 (`constitutional_gate` hook point — fail-closed, no `fail_open`)
> **Crate:** `sera-hooks`
> **Priority:** Phase 1

---

## 1. Overview

The hook system is SERA's **extensibility backbone**. Hooks are chainable processing pipelines that fire at defined points throughout the event lifecycle. Built-in hooks are **plain Rust trait implementations** (`impl Hook for MyHook`); WASM sandboxing is **opt-in for third-party isolation only**. They enable operators and developers to inject custom logic — content filtering, rate limiting, PII redaction, secret injection, risk assessment, compliance checks — without modifying core code.

Hooks are:
- **Chainable** — one hook's output feeds into the next
- **Parameterized** — each hook instance has its own configuration block
- **Optionally sandboxed** — third-party hooks run in a WASM runtime with fuel metering and memory caps; built-in hooks run in-process as native Rust
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

## 1b. Hook Implementation Strategy — Native-First, WASM Opt-In

> **Design decision — 2026-04-16.** SERA originally specified WASM hooks at every stage. Hermes comparison revealed this is over-engineered. Built-in hooks should be plain Rust trait methods for performance and simplicity; WASM should be reserved for third-party isolation.

### Native Rust Hooks (Default)

All hooks that ship with SERA are implemented as plain Rust structs that implement the `Hook` trait (§2.2). They run in-process with zero serialization overhead. Examples:

- `ContentFilter` — built-in content filtering
- `RateLimiter` — built-in rate limiting
- `SecretInjector` — credential injection from secret providers
- `PiiRedactor` — PII detection and masking
- `RiskChecker` — risk assessment for tool calls

These hooks are registered at startup and participate in chains identically to WASM hooks — the `ChainExecutor` is agnostic to the implementation.

### WASM Hooks (Opt-In for Third-Party Isolation)

WASM hooks are used **only** when:
1. The hook author is a third party who should not have access to the gateway process
2. The hook needs to be hot-reloaded without restarting the gateway
3. The operator requires sandboxing guarantees (fuel metering, memory caps) for untrusted code

The `WasmHookAdapter` wraps a WASM component and implements the same `Hook` trait — the chain executor doesn't distinguish between native and WASM hooks.

### Subprocess Hooks (Language-Agnostic Extension)

As described in §2.6, subprocess hooks provide a third option for cases where neither native Rust nor WASM is practical.

### Decision Matrix

| Scenario | Implementation |
|---|---|
| SERA ships the hook | Native Rust (`impl Hook`) |
| Operator writes a custom hook in Rust | Native Rust, compiled into a custom build |
| Third-party publishes a hook | WASM component (sandboxed) |
| Legacy script needs to be a hook | Subprocess hook (§2.6) |
| Hot-reload without restart needed | WASM or subprocess |

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

## 5. WASM Runtime (Third-Party Isolation)

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §6 — `wasmtime` 43 with Component Model + WASI Preview 2, `wasmtime-wasi-http` allow-list. **`extism` is explicitly rejected** (pinned to wasmtime <31, non-WIT proprietary ABI). See SPEC-dependencies §6 for the full rationale.

> **Scope clarification — 2026-04-16.** The WASM runtime is for **third-party and operator-authored hooks only**. Built-in hooks (content filter, rate limiter, secret injector, etc.) are native Rust implementations of the `Hook` trait and do not use the WASM runtime. This avoids the overhead of WASM serialization for hooks that are compiled into the binary.

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

Third-party hooks are compiled to **WASM Components** using standard toolchains. Built-in hooks are plain Rust implementations of the `Hook` trait and do not require WASM compilation:

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

## 10. Two-Tier Hook Bus (Fast / Slow Execution Lanes)

> **Status:** DRAFT (fills gap `sera-61ao`)
> **Not to be confused with §2.5**, which splits the bus by *event access control* (`InternalHookBus` vs `PluginHookBus`). This section splits by *execution lane* — synchronous in-process dispatch vs asynchronous durable fanout. The two axes are orthogonal: an event on the `PluginHookBus` may travel the fast or slow lane depending on its hook kind.

The single-tier `ChainExecutor` in `rust/crates/sera-hooks/src/executor.rs` executes every chain inline on the caller's task. That is correct for decisions the caller must see before proceeding (authZ, secret injection, constitutional gating), but wrong for fanout work that is append-only (audit, training-data emission, metric scrape). Putting audit in the same lane as authZ makes p99 tool-call latency hostage to Postgres commit latency. The two-tier bus separates them.

### 10.1 Lane Definitions

| Lane | Transport | Ordering | Durability | Tail-latency target | Typical consumers |
|---|---|---|---|---|---|
| **Fast** | `Arc<HookBus>` per process, synchronous `.await` | Strict in-chain order | None (best-effort; lost on crash) | **< 1 ms p95** per chain | `pre_tool`, `pre_route`, `constitutional_gate`, `on_llm_start`, any hook whose `HookResult` can short-circuit |
| **Slow** | Tokio `mpsc` → persistent FIFO | Per-event FIFO, parallel across consumers | At-least-once | < 100 ms p95 emit (consumption may lag) | `post_tool` audit, `post_deliver` analytics, training-data emission, metric fanout, replayable observability |

> **Tradeoff:** splitting lanes doubles the number of code paths a hook author must reason about, but the alternative (one lane for everything) means the slowest consumer sets the latency floor for every decision hook. The split is load-bearing for `pre_tool` p95 ≤ 1 ms under audit log backpressure.

### 10.2 Fast Path API

Fast-path publish is synchronous and returns the final `HookResult` — callers block on the decision because they cannot proceed without it.

```rust
impl HookBus {
    /// Dispatch all chains wired to `point` on the fast lane and return the
    /// merged result. Blocks the caller's task until the chain completes or
    /// times out.
    pub async fn publish_fast(
        &self,
        point: HookPoint,
        ctx: HookContext,
    ) -> Result<FastResult, HookError>;
}

pub struct FastResult {
    pub outcome: HookResult,                  // Continue | Reject | Redirect
    pub updated_input: Option<serde_json::Value>,
    pub hooks_executed: usize,
    pub duration_ms: u64,
}
```

This API is a thin renaming of the existing `ChainExecutor::execute_at_point` — the function's signature matches today's behavior. The only semantic change is that `publish_fast` is the *only* way to run chains whose result the caller needs.

> **Tradeoff:** keeping `FastResult` isomorphic to today's `ChainResult` means the migration is a rename, not a rewrite. But it also means the fast path inherits `ChainExecutor`'s current timeout model (per-chain deadline, cooperative). That is intentional — deterministic termination on the fast lane is more important than expressiveness.

### 10.3 Slow Path API

Slow-path publish returns immediately with a `PublishReceipt`; the caller never blocks on consumers.

```rust
impl HookBus {
    /// Enqueue an event for the slow lane. Returns as soon as the event is
    /// accepted into the in-memory queue (and, for durable configs, persisted).
    pub async fn publish_slow(
        &self,
        event: PluginEvent,
    ) -> Result<PublishReceipt, HookError>;
}

pub struct PublishReceipt {
    pub event_id: EventId,
    pub queued_at: DateTime<Utc>,
    pub queue_depth_after: u32,           // Observability signal for producers
}
```

The slow lane consumes from the queue on a dedicated Tokio runtime task (or task pool), each consumer driving its own independent chain. Consumers do not share state across events — fanout is strictly per-event.

### 10.4 Slow-Path Persistence Options

Two backends are in scope for Phase 1; the choice is per-deployment via `HookBusConfig::slow_backend`.

| Backend | Model | Pros | Cons |
|---|---|---|---|
| `sled`-backed FIFO | Embedded, single-process B-tree queue | Zero ops, fast local commit (~µs), no network | Not visible to other processes; replicas cannot share the queue |
| Postgres outbox | Single `hook_outbox` table, consumers `SELECT ... FOR UPDATE SKIP LOCKED` | Shared across all gateway replicas, survives restarts, aligns with existing Postgres infra (§SPEC-db) | Commit latency dominates (~1–5 ms); ops burden on shared DB |

> **Tradeoff:** `sled` is faster and zero-ops but single-process, so multi-gateway deployments must use Postgres outbox. We ship both and default to Postgres on HA deployments, `sled` on single-node ones. See also the SERA deployment tiers defined in [SPEC-deploy](SPEC-deploy.md) §3 if such a spec exists; otherwise the operator chooses.

Both backends satisfy the at-least-once guarantee defined in §10.6. Neither provides exactly-once — consumers must be idempotent. This is consistent with the audit store's own idempotency (see [SPEC-events](SPEC-events.md)).

### 10.5 Routing Matrix

The classification rule is mechanical — it does not require per-hook-point tuning.

> **Rule:** A chain runs on the fast lane iff its `HookResult` can short-circuit the caller (`Reject` or `Redirect` is meaningful to the caller at that point). Otherwise it runs on the slow lane.

Applying this rule to the hook points in §3:

| Hook Point | Lane | Why |
|---|---|---|
| `constitutional_gate` | Fast | Must reject before dispatch |
| `pre_route`, `post_route` | Fast | Can reject or redirect the event |
| `pre_tool` | Fast | AuthZ, secret injection — caller must see result |
| `on_llm_start`, `on_llm_end` | Fast | Budget checks short-circuit model call |
| `pre_deliver` | Fast | Last-line content filter |
| `pre_memory_write` | Fast | Policy decision gates the write |
| `on_approval_request` | Fast | Routes the approval synchronously |
| `post_tool` | **Slow** | Audit, risk scoring, training-data emission; caller has already moved on |
| `post_turn` | **Slow** | Post-hoc compliance scan; result is not consumed by the caller |
| `post_deliver` | Slow | Analytics, notification triggers |
| `session_start`, `on_session_transition` | Slow | Observability, lifecycle notification |
| `on_workflow_trigger` | Slow | Fanout to schedulers; no caller blocks on it |
| `on_change_artifact_proposed` | Slow | Meta-approval routing is async by nature (SPEC-self-evolution §5.3) |
| `subagent_delivery_target` | Fast | Transforms the result before it reaches the parent — must be synchronous |
| `context_*` | Fast | Inline context assembly steps |

Hooks that short-circuit (`HookOutcome::Reject` / `Redirect`) **must never** be placed on the slow lane — the short-circuit would have no caller to act on. The config loader (§4) validates this at manifest load time and refuses to wire a chain with short-circuiting potential onto a slow-lane point.

> **Tradeoff:** the classification is static (per hook point) rather than dynamic (per chain instance). A dynamic policy would let operators put the same chain on either lane, but it would also let them misroute a security-critical hook onto the slow lane by mistake. Static wins by default; per-chain override is an open question.

### 10.6 Backpressure & Overflow

The slow-path in-memory queue is bounded. When full, SERA uses **drop-oldest with a counter bump**:

1. The newest event is always accepted.
2. The oldest queued event is dropped to make room.
3. The counter `bus_slow_drops_total{reason="overflow"}` is incremented.
4. A `tracing::warn!` is emitted at most once per 10 s (rate-limited to avoid log flooding).

> **Tradeoff: drop-oldest vs block-producer.** Blocking the producer would give back-pressure and zero loss, but it would also let a slow audit sink stall `pre_tool` callers — defeating the whole point of splitting lanes. Drop-oldest preserves the fast-lane latency contract; operators accept bounded loss during saturation. This choice is also consistent with the hypothesis that overflow is a configuration bug (undersized queue, stuck consumer) that must be surfaced loudly by the counter.

The durable backends (sled / Postgres outbox) have a second guarantee: once an event crosses the in-memory queue into the durable backend, it cannot be dropped except by explicit operator action. The drop window is strictly the in-memory hop.

### 10.7 Persistence Guarantees

| Lane | Guarantee | Loss scenarios |
|---|---|---|
| Fast | **Best-effort.** Events lost on process crash mid-chain are acceptable because they were pre-call filters — the caller will retry or fail-closed | Process crash, OOM |
| Slow | **At-least-once** once the `PublishReceipt` is returned | None after receipt; drop-oldest can drop before receipt if the caller is racing saturation (rare — `publish_slow` awaits commit) |

Slow-path consumers must be idempotent. Every `PluginEvent` carries an `event_id` (§2.5); consumers dedupe on `(event_id, consumer_id)` in their own storage.

### 10.8 Metrics Contract

All metrics are exposed via `sera-telemetry` (see [SPEC-telemetry](SPEC-telemetry.md) if present) with the standard `{instance, circle, chain}` label set.

| Metric | Type | Meaning |
|---|---|---|
| `bus_fast_publish_total` | counter | Chains executed on the fast lane |
| `bus_fast_publish_duration_seconds` | histogram | Fast-lane chain wall-clock duration |
| `bus_fast_short_circuit_total{outcome}` | counter | Count of fast-lane `Reject` / `Redirect` outcomes |
| `bus_slow_queue_depth` | gauge | Current depth of the in-memory slow queue |
| `bus_slow_drops_total{reason}` | counter | Events dropped on slow-lane overflow |
| `bus_slow_consumer_lag_seconds` | gauge | Wall-clock delay between `queued_at` and consumer start |
| `bus_slow_replay_lag_seconds` | gauge | Only meaningful on durable backends — replay cursor lag after restart |

The names and labels are fixed by this spec; emitters (§SPEC-telemetry) may add additional ones but may not rename these.

### 10.9 Migration Path

The current `ChainExecutor` becomes the fast-lane implementation verbatim: rename `ChainExecutor` → `FastLaneExecutor`, add a new `SlowLaneDispatcher` behind the same crate, and re-export both through a `HookBus` facade that owns one of each. `execute_at_point` keeps working as an alias for `publish_fast` during the deprecation window (one release). The registry (`HookRegistry`) is shared between lanes — a hook does not know which lane it runs on. Only the chain manifest decides.

### 10.10 Open Questions

1. **Per-chain lane override.** Should operators be allowed to explicitly opt a slow-lane-default hook point into the fast lane (for a deployment where they accept the latency to gain the short-circuit capability)? Current answer: no. Revisit if observability use cases demand it.
2. **Cross-process slow-lane fanout.** If we deploy Postgres outbox, should we also expose the outbox to external subscribers (webhooks, gRPC stream)? That overlaps with SPEC-plugins; deferred.
3. **Replay semantics on durable restart.** When a gateway starts up and reads the outbox, should it replay from last-committed cursor or skip all un-consumed events older than N seconds? Hard durability vs "fresh-start" operator preference.

---

## 11. WASM Fuel Metering and Budgets

> **Status:** DRAFT (fills gap `sera-az1x`)
> **Scope:** formalizes the fuel/OOM discipline that §5.2 gestures at but does not specify.

Today `WasmHookAdapter::from_bytes` in `rust/crates/sera-hooks/src/wasm_adapter.rs` constructs a `wasmtime::Store` with neither `consume_fuel(true)` nor a `ResourceLimiter`. A malicious or buggy WASM hook can therefore loop forever or allocate unbounded memory — the timeout in §5.2 catches wall-clock forever-loops only at chain granularity, not per-instruction, and it does not bound memory at all. This section closes that gap.

### 11.1 Fuel Model

Wasmtime's fuel metering is the primary defense. The runtime enables it via `wasmtime::Config::consume_fuel(true)`; before each invocation the adapter calls `Store::set_fuel(initial_budget)`; each executed Wasm instruction decrements the store's fuel counter; when fuel reaches zero the engine traps with a fuel-exhaustion trap. The trap is caught by the adapter and converted to a typed error (§11.4).

```rust
// Conceptual — not implementation code.
let mut config = wasmtime::Config::new();
config.consume_fuel(true);
// ... rest of engine config
let engine = Engine::new(&config)?;

let mut store = Store::new(&engine, host_state);
store.set_fuel(budget.fuel_limit)?;           // per-invocation
// invoke hook; trap on exhaustion
```

The fuel ↔ time mapping is **workload-dependent** and **not deterministic across hosts**. Wasmtime documents fuel as roughly proportional to Wasm instructions executed (one fuel unit per instruction on the default configuration), not CPU cycles. Budget values below are starter guesses based on rough benchmarks (~100 MHz sustained Wasm instruction throughput ≈ 10M fuel ≈ ~100 ms wall on a modern desktop). Operators must re-tune in their deployment.

> **Tradeoff:** fuel is bytecode-only. A hook that spends most of its time in a host call (e.g. `wasi:http/outgoing-handler.handle`) does not consume fuel while blocked. That is acceptable because §5.5 already requires host calls to be wrapped in `tokio::time::timeout`. Fuel bounds *computation*; timeout bounds *blocking*. They are complementary, not substitutes.

### 11.2 Budget Sources (Precedence Order)

Effective budget for one hook invocation is resolved by the first source that sets a value:

1. **Per-hook override in the hook instance config** (`HookInstance.config.budget`).
2. **Per-chain budget in the `HookChain` manifest** (`HookChain.budget`).
3. **Per-agent-tier default** from the tier policy (tier-1 / tier-2 / tier-3 YAML in `sandbox-boundaries/`).
4. **Workspace-wide fallback** from the root `Instance` manifest (`spec.hooks.wasm.budget`).

Concrete examples of each layer:

```yaml
# 1) Per-hook override — narrowest scope
apiVersion: sera.dev/v1
kind: HookChain
metadata:
  name: "pre-tool-risk"
spec:
  hook_point: "pre_tool"
  hooks:
    - hook: "pii-deep-scan"
      config:
        budget:
          fuel_limit: 50_000_000       # Expensive regex engine, explicit override
          memory_limit_bytes: 134_217_728   # 128 MiB
```

```yaml
# 2) Per-chain budget — default for every hook in this chain
apiVersion: sera.dev/v1
kind: HookChain
metadata:
  name: "post-turn-compliance"
spec:
  hook_point: "post_turn"
  budget:
    fuel_limit: 20_000_000
    memory_limit_bytes: 67_108_864   # 64 MiB
  hooks:
    - hook: "compliance-scanner"
```

```yaml
# 3) Per-agent-tier default — sandbox-boundaries/tier-1.yaml
apiVersion: sera.dev/v1
kind: SandboxBoundary
metadata:
  name: "tier-1"
spec:
  hooks:
    budget:
      fuel_limit: 10_000_000
      memory_limit_bytes: 33_554_432   # 32 MiB
```

```yaml
# 4) Workspace-wide fallback
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: "my-sera"
spec:
  hooks:
    wasm:
      budget:
        fuel_limit: 5_000_000           # Conservative floor
        memory_limit_bytes: 16_777_216  # 16 MiB
```

> **Tradeoff:** a 4-level precedence ladder is more config surface than a flat per-hook value, but it is the minimum to let operators set a safe default *and* let individual hooks opt out when they genuinely need more. The alternative — requiring every hook to set its own budget — would push the decision onto hook authors, who do not know the deployment's total CPU budget.

### 11.3 Starter Budget Values

These are the values shipped in the default manifests. Each is expressed as `(fuel_limit, memory_limit_bytes)`:

| Scope | Fuel limit | Memory limit | Approx. wall time |
|---|---|---|---|
| Tier-1 (untrusted third-party) | 10_000_000 | 32 MiB | ~10 ms |
| Tier-2 (operator-authored) | 50_000_000 | 64 MiB | ~50 ms |
| Tier-3 (privileged integrations) | 100_000_000 | 128 MiB | ~100 ms |
| Workspace fallback | 5_000_000 | 16 MiB | ~5 ms |

Wasmtime documents the ~1-fuel-per-instruction mapping; see the upstream reference for `wasmtime::Config::consume_fuel` and `Store::set_fuel`. The mapping is imprecise (§5.5 caveat: fuel counted per basic block, slight overshoot possible) but adequate for budget enforcement.

### 11.4 Error Surface

When fuel exhausts, the wasmtime trap is converted by the adapter into a new typed variant on `HookError`:

```rust
// Addition to `rust/crates/sera-hooks/src/error.rs`
#[error("hook '{hook_name}' exhausted fuel: consumed {budget_consumed}/{budget_limit}")]
FuelExhausted {
    hook_name: String,
    budget_consumed: u64,   // fuel actually burned (budget_limit - Store::get_fuel())
    budget_limit: u64,
},

#[error("hook '{hook_name}' exceeded memory limit: {bytes_requested} > {bytes_limit}")]
MemoryExhausted {
    hook_name: String,
    bytes_requested: usize,
    bytes_limit: usize,
},
```

Both variants are terminal for the chain regardless of `fail_open`. The rationale: a hook that runs out of fuel or memory is likely malicious or broken; silently skipping it on `fail_open: true` would let an attacker bypass security hooks by crafting inputs that exhaust fuel. Treat fuel/memory exhaustion as a security signal, not as a transient failure.

> **Tradeoff:** making exhaustion terminal even under `fail_open: true` contradicts the general fail-open contract in §5.3. The exception is deliberate — fail-open exists for *transient* failures (network blips on host calls, temporary allocator pressure). Exhaustion is not transient, it is a budget violation by this specific hook invocation, and the correct response is to refuse the invocation.

### 11.5 Async Yield (Phase 2 deferred)

Wasmtime supports `Store::fuel_async_yield_interval(Some(N))` which turns fuel exhaustion into a cooperative yield instead of a trap — useful for running many WASM tasks on a single Tokio runtime without starving them of each other. **This is NOT used in Phase 1.** Phase 1 uses blocking exhaustion: fuel runs out → trap → `FuelExhausted`. Phase 2 may adopt yielding as part of a broader cooperative scheduling redesign when we have more than one hook concurrently per process, but it is not required for correctness and adds nontrivial complexity to the adapter (yields must be async-compatible with host call plumbing).

> **Tradeoff:** blocking exhaustion is simpler to reason about — the budget strictly bounds each invocation. Yielding would let a hook survive past its initial budget by cooperating, which complicates both the budget math and the telemetry.

### 11.6 OOM Metering (Memory Limit)

Memory is bounded separately from fuel, via `wasmtime::ResourceLimiter` installed on the `Store`. The limiter rejects memory growth requests beyond `memory_limit_bytes` and the WASM allocator receives a growth failure, which typically traps (same conversion path as fuel — caught and mapped to `HookError::MemoryExhausted`).

The budget struct pairs both limits so operators cannot accidentally set one without the other:

```rust
// In sera-types::hook or a new sera-hooks::budget module
pub struct HookBudget {
    pub fuel_limit: u64,
    pub memory_limit_bytes: usize,
}
```

> **Tradeoff:** a single `HookBudget` struct forces both fields to be set together. An alternative (separate optional `FuelBudget` and `MemoryBudget`) would let operators override only one, but experience in other Wasm hosts (Envoy, Fastly) shows that half-set budgets are a recurring foot-gun.

### 11.7 Integration Point

All fuel/memory bookkeeping lives in the WASM adapter, not in the registry or the executor. Specifically:

- **File:** `rust/crates/sera-hooks/src/wasm_adapter.rs`.
- **Callsite:** the adapter's `async fn execute(&self, ctx: &HookContext) -> Result<HookResult, HookError>` implementation of the `Hook` trait (currently line ~280).
- **Sequence:** before calling `hook_execute` inside the store:
  1. Compute `HookBudget` from the precedence ladder (§11.2) — this is passed through from the chain executor, not resolved inside the adapter.
  2. `store.set_fuel(budget.fuel_limit)`.
  3. Install the `ResourceLimiter` with `budget.memory_limit_bytes`.
  4. Invoke.
  5. On success: read residual fuel via `Store::get_fuel()`, compute consumed, emit `hook_fuel_consumed_total`.
  6. On trap: inspect trap reason; if fuel exhaustion → `FuelExhausted`; if resource-limit growth failure → `MemoryExhausted`; else map to existing `HookError::ExecutionFailed`.

Native (non-WASM) hooks do not participate in fuel metering — they are compiled into the gateway binary and trusted. See §1b for the native-first design decision.

### 11.8 Telemetry

| Metric | Type | Labels | Meaning |
|---|---|---|---|
| `hook_fuel_consumed_total` | counter | `hook_name` | Total fuel units consumed across all invocations (quantile via PromQL `rate`) |
| `hook_fuel_exhausted_total` | counter | `hook_name` | Count of `FuelExhausted` errors |
| `hook_memory_peak_bytes` | histogram | `hook_name` | Peak memory per invocation (observed from the `ResourceLimiter`) |
| `hook_memory_exhausted_total` | counter | `hook_name` | Count of `MemoryExhausted` errors |

These complement the chain-level metrics in §10.8. Together they let operators distinguish "chain is slow" from "one WASM hook in the chain is spending its entire budget".

### 11.9 Testing Strategy

Three test categories are required:

1. **Unit — exhaustion maps to typed error.** Load a hand-written WASM module that runs a tight loop; configure `fuel_limit: 1000`; assert `execute` returns `HookError::FuelExhausted { hook_name, budget_consumed, budget_limit }` with `budget_consumed == budget_limit`.
2. **Unit — residual fuel reported correctly.** Load a hook that does a known small amount of work; after a successful invocation, assert `Store::get_fuel()` was read and the metric `hook_fuel_consumed_total` incremented by the expected delta.
3. **Property — `fuel_consumed ≤ fuel_limit` always holds.** Using `proptest`, generate random (short) WASM programs and random budgets; assert the invariant across all invocations. This is the single most important property because it encodes the security contract: no hook runs past its budget.

Memory exhaustion tests mirror fuel tests: a hook that deliberately `memory.grow`s past the limit must produce `MemoryExhausted`.

> **Tradeoff:** the property test requires a tiny Wasm codegen helper, which adds test-time complexity. The alternative (exhaustive hand-written cases) would be less thorough. Adopt `proptest` + a minimal Wat template and keep the generator under 50 LOC.

### 11.10 Open Questions

1. **Global fuel budget per chain.** Should the chain as a whole have a fuel ceiling (sum of per-hook fuel), or is per-hook enforcement sufficient? A global ceiling would catch chains padded with many small-budget hooks that collectively DoS the caller. Revisit after we see chain authoring patterns.
2. **Fuel budget refunding.** If a hook short-circuits with `Reject` after consuming a fraction of its budget, should the residual fuel be credited to a downstream operation? Probably no — fuel is per-invocation, not a shared pool. Call out explicitly so nobody tries it.
3. **Cross-host fuel portability.** Wasmtime's fuel mapping is not deterministic across CPUs/architectures. Do we need to pin fuel budgets to a reference hardware class, or accept per-deployment tuning as the operator's job? Leaning toward the latter, but worth calling out.

---

## 12. Open Questions

1. **Hook-to-hook state passing** — Can hooks pass state to subsequent hooks in the chain beyond modifying HookContext? Is the `metadata` HashMap sufficient?
2. **Hook capabilities (WASM component model)** — Which host capabilities can be granted to hooks? Network? File system? Specific APIs?
3. **Hook lifecycle events** — Are hooks notified of system lifecycle events (startup, shutdown, config change)?
4. ~~**Hook versioning**~~ — Resolved: See [SPEC-versioning](SPEC-versioning.md) §6. WIT package versioning with compatibility matrix.
5. **Hook testing** — What's the DX for testing hooks locally before deploying?

---

## 13. Success Criteria

| Metric | Target |
|---|---|
| Hook chain overhead | < 5ms per WASM hook invocation, chains additive |
| Extension authoring | < 1 hour for a WASM hook |
