# SPEC: Hook System (`sera-hooks`)

> **Status:** DRAFT  
> **Source:** PRD §5 (all subsections), §14 (invariants 8, 11)  
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

pub enum HookResult {
    Continue(HookContext),       // Pass through (possibly modified) to next in chain
    Reject(RejectReason),        // Short-circuit: block the event/action
    Redirect(RedirectTarget),    // Short-circuit: reroute to different session/agent
}
```

### 2.3 Hook Context

The `HookContext` carries the current event/action data and accumulated state through the chain. Each hook can read and modify it.

```rust
pub struct HookContext {
    pub event: Option<Event>,           // Present for event-related hooks
    pub turn: Option<TurnContext>,      // Present for turn-related hooks
    pub tool_call: Option<ToolCall>,    // Present for tool-related hooks
    pub principal: PrincipalRef,
    pub session: Option<SessionRef>,
    pub metadata: HashMap<String, serde_json::Value>,  // Accumulated chain state
}
```

---

## 3. Hook Points (Comprehensive)

| Hook Point | Fires When | Use Cases |
|---|---|---|
| `pre_route` | After event ingress, before queue | Content filtering, rate limiting, classification |
| `post_route` | After routing decision, before enqueue | Routing override, logging |
| `pre_turn` | After queue dequeue, before context assembly | Context enrichment, policy injection |
| `context_persona` | During persona assembly step | Persona switching, mode injection |
| `context_memory` | During memory injection step | Memory tier selection, RAG tuning |
| `context_skill` | During skill injection step | Skill filtering, mode transitions |
| `context_tool` | During tool injection step | Tool filtering, capability policy |
| `pre_tool` | Before tool execution | Approval gates, argument validation, **secret injection** |
| `post_tool` | After tool execution | Result sanitization, audit, **risk assessment** |
| `post_turn` | After runtime, before response delivery | Response filtering, compliance, redaction |
| `pre_deliver` | Before response delivery to client/channel | Final formatting, channel-specific transforms |
| `post_deliver` | After response delivery confirmed | Analytics, notification triggers |
| `pre_memory_write` | Before durable memory write | Content policy, PII filtering |
| `on_session_transition` | On session state machine transition | Lifecycle hooks, cleanup, notification |
| `on_approval_request` | When HITL approval is triggered | Routing to correct approver, escalation logic |
| `on_workflow_trigger` | When a scheduled/triggered workflow fires | Workflow gating, context injection |

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

### 5.1 Runtime Configuration

```rust
pub struct WasmConfig {
    pub fuel_limit: u64,              // Computation budget per hook invocation
    pub memory_limit_mb: u32,         // Memory cap per hook instance
    pub timeout: Duration,            // Per-hook timeout
    pub hot_reload: bool,             // Watch hook directory for changes
    pub hook_directory: PathBuf,      // Where .wasm files live
}
```

### 5.2 Sandbox Guarantees

- **Fuel metering** — each hook invocation has a computation budget; exceeded = abort
- **Memory cap** — each hook instance has a memory ceiling; exceeded = abort
- **Timeout** — per-hook and per-chain timeouts
- **No ambient host access** — hooks cannot access the filesystem or network unless explicitly granted capabilities via the WASM component model
- **Deterministic termination** — fuel + timeout ensure hooks always terminate

### 5.3 Fail-Open vs. Fail-Closed

Each chain has a `fail_open` setting:
- `fail_open: true` — if a hook errors, skip it and continue the chain (suitable for non-critical hooks like analytics)
- `fail_open: false` — if a hook errors, the entire chain fails and the action is rejected (suitable for security hooks)

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
