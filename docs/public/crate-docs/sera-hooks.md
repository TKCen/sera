# `sera-hooks` — In-Process Hook Registry and Chain Executor

**Crate:** `rust/crates/sera-hooks`
**Type:** library
**Spec:** `docs/plan/specs/SPEC-hooks.md`
**Types source:** `rust/crates/sera-types/src/hook.rs`

---

## Overview

`sera-hooks` implements SERA's in-process hook execution layer. It provides:

- **`Hook` trait** — the interface every native Rust hook implements
- **`HookRegistry`** — a name-keyed store of registered hook instances
- **`ChainExecutor`** — runs ordered chains of hooks at a given hook point

This crate handles **native Rust hooks only**. WASM hook execution is a planned future extension; when it lands, a `WasmHookAdapter` will implement the same `Hook` trait, so `ChainExecutor` requires no changes.

All types shared with other crates (`HookChain`, `HookInstance`, `HookContext`, `HookResult`, `HookPoint`, `HookMetadata`, `ChainResult`) live in `sera-types::hook` to avoid a dependency on `sera-hooks` from crates that only need the data model.

---

## Architecture: In-Process Hook Model

```
  ┌────────────────────────────────────────────────────────┐
  │  Caller (gateway or harness)                           │
  │                                                        │
  │  let result = executor                                 │
  │      .execute_at_point(point, &chains, ctx)            │
  │      .await?;                                          │
  └───────────────────────┬────────────────────────────────┘
                          │
                          ▼
  ┌────────────────────────────────────────────────────────┐
  │  ChainExecutor                                         │
  │                                                        │
  │  1. Filter chains where chain.point == point           │
  │  2. For each matching chain:                           │
  │     execute_chain(chain, ctx) → ChainResult            │
  │  3. Carry updated ctx between chains                   │
  │  4. Stop on first Reject or Redirect                   │
  └───────────────────────┬────────────────────────────────┘
                          │
                          ▼
  ┌────────────────────────────────────────────────────────┐
  │  execute_chain — per-chain loop                        │
  │                                                        │
  │  For each HookInstance in chain.hooks:                 │
  │    if !instance.enabled → skip                         │
  │    if elapsed >= timeout → ChainTimeout error          │
  │    hook = registry.get(instance.hook_ref)              │
  │    result = timeout(remaining, hook.execute(&ctx))     │
  │                                                        │
  │    match result:                                       │
  │      Continue { context_updates } → ctx.apply_updates  │
  │      Reject / Redirect            → return immediately │
  │      hook error, fail_open=true   → warn + skip        │
  │      hook error, fail_open=false  → propagate Err      │
  │      timeout, fail_open=true      → warn + skip        │
  │      timeout, fail_open=false     → ChainTimeout Err   │
  └────────────────────────────────────────────────────────┘
```

### Chain Execution Flow (single chain)

```
  HookChain { hooks: [A, B, C], timeout_ms: 5000, fail_open: false }
  ctx = HookContext { point: PreRoute, metadata: {} }

  ┌───────────────────────────────────────────────────────────────┐
  │ t=0ms  Check: elapsed(0) < 5000ms → OK                        │
  │        registry.get("A") → PassthroughHook                    │
  │        A.execute(&ctx) → Continue { context_updates: {} }     │
  │        ctx unchanged                                          │
  ├───────────────────────────────────────────────────────────────┤
  │ t=2ms  Check: elapsed(2) < 5000ms → OK                        │
  │        registry.get("B") → ModifyingHook                      │
  │        B.execute(&ctx) → Continue { {"filtered": true} }      │
  │        ctx.metadata.insert("filtered", true)                   │
  ├───────────────────────────────────────────────────────────────┤
  │ t=4ms  Check: elapsed(4) < 5000ms → OK                        │
  │        registry.get("C") → PassthroughHook                    │
  │        C.execute(&ctx) → Continue {}                          │
  │        All hooks ran → return ChainResult { is_success: true } │
  └───────────────────────────────────────────────────────────────┘

  Short-circuit example (hook B returns Reject):

  ┌───────────────────────────────────────────────────────────────┐
  │ Hook A → Continue                                             │
  ├───────────────────────────────────────────────────────────────┤
  │ Hook B → Reject { reason: "blocked" }                         │
  │          ↓ immediate return                                   │
  │          ChainResult { outcome: Reject, hooks_executed: 2 }   │
  │          Hook C never runs                                    │
  └───────────────────────────────────────────────────────────────┘
```

---

## Types (from `sera-types::hook`)

### `HookPoint`

Twenty hook points spanning the full event lifecycle. Each point fires at a defined stage of request processing. Hooks registered for a point that doesn't match the current operation are silently skipped by `execute_at_point`.

Points are divided by **layer** — the process that owns and enforces them:

#### Gateway-side (policy, security-critical)

Gateway-side hooks enforce policy decisions that the harness cannot override. They apply to all connected harnesses (embedded runtime, BYOH, external agents).

| Hook Point | When | Purpose |
|---|---|---|
| `ConstitutionalGate` | Before all others | Fail-closed invariant enforcement — no `fail_open` |
| `PreRoute` | After ingress, before queue | Content filtering, rate limiting, classification |
| `PostRoute` | After routing decision, before enqueue | Routing override, logging |
| `PreTool` | Before tool execution | AuthZ enforcement, secret injection, approval gates |
| `PostTool` | After tool execution | Audit, result sanitization, risk assessment |
| `PreDeliver` | Before delivery to client/channel | Final content filtering, channel transforms |
| `PostDeliver` | After delivery confirmed | Analytics, notification triggers |
| `PreMemoryWrite` | Before durable memory write | Content policy, PII filtering |
| `OnSessionTransition` | On state machine transition | Lifecycle, cleanup, notifications |
| `OnApprovalRequest` | When HITL approval triggered | Routing to approver, escalation |
| `OnWorkflowTrigger` | When workflow fires | Gating, context injection |
| `OnChangeArtifactProposed` | When change artifact proposed | Observability, meta-approval routing |

#### Harness-side (operational, no security relevance)

Harness-side hooks run in the harness process and affect how context is assembled — not what is permitted. A compromised harness can ignore them without breaking system security invariants.

| Hook Point | When | Purpose |
|---|---|---|
| `PreTurn` | After dequeue, before context assembly | Context enrichment, policy |
| `ContextPersona` | During persona assembly | Persona switching, mode injection |
| `ContextMemory` | During memory injection | Tier selection, RAG tuning |
| `ContextSkill` | During skill injection | Skill filtering, mode transitions |
| `ContextTool` | During tool injection | Tool filtering, capability policy |
| `OnLlmStart` | Before LLM call | Prompt inspection, cost control, context trimming |
| `OnLlmEnd` | After LLM response | Response inspection, safety checks |
| `PostTurn` | After runtime, before delivery | Response filtering, compliance, redaction |

All 20 points are available as `HookPoint::ALL` (a `&[HookPoint]` constant in lifecycle order).

Serialization uses `snake_case` (e.g., `"pre_route"`, `"on_llm_start"`).

---

### `HookChain`

A named, ordered sequence of hook instances that fires at a single hook point.

```rust
pub struct HookChain {
    pub name: String,           // Unique name, e.g. "content-filter-chain"
    pub point: HookPoint,       // The point this chain fires at
    pub hooks: Vec<HookInstance>, // Ordered; output of hook N feeds into hook N+1
    pub timeout_ms: u64,        // Total wall-clock budget for the entire chain (default: 5000)
    pub fail_open: bool,        // Error handling mode (default: false = fail-closed)
}
```

`timeout_ms` is a *chain-level* budget. Each hook gets the remaining budget (`deadline - elapsed`) as its individual timeout, enforced via `tokio::time::timeout`. When the budget expires mid-chain, `HookError::ChainTimeout` is returned.

`fail_open` controls how hook errors and missing hooks are handled:
- `false` (fail-closed, default): any error or missing hook propagates immediately, rejecting the operation. Use for security-critical chains.
- `true` (fail-open): errors and missing hooks are logged as warnings and skipped; the chain continues with remaining hooks. Use for observability or enrichment chains where partial failure is acceptable.

---

### `HookInstance`

A single entry within a `HookChain`. References a registered hook by name and carries per-instance configuration.

```rust
pub struct HookInstance {
    pub hook_ref: String,             // Key used to look up the hook in HookRegistry
    pub config: serde_json::Value,    // Per-instance JSON config passed to Hook::init()
    pub enabled: bool,                // Toggle without removing (default: true)
}
```

Setting `enabled: false` causes `ChainExecutor` to silently skip the instance. The hook is not looked up in the registry and does not count toward `hooks_executed`.

---

### `HookResult`

The return value of a single `Hook::execute()` call. Determines whether the chain continues or short-circuits.

```rust
pub enum HookResult {
    Continue {
        context_updates: HashMap<String, serde_json::Value>,
        updated_input: Option<serde_json::Value>,   // Reserved — not yet consumed by ChainExecutor
    },
    Reject {
        reason: String,
        code: Option<String>,
    },
    Redirect {
        target: String,
        reason: Option<String>,
    },
}
```

- **`Continue`** — chain keeps going. `context_updates` are merged into `ctx.metadata` via `HookContext::apply_updates()`. `updated_input` is defined in the type but not yet wired into `execute_chain` (tracked in executor.rs TODO P0-5/P0-6).
- **`Reject`** — immediate chain termination. The `ChainResult` outcome is `Reject`. The caller is responsible for surfacing `reason` to the user/client.
- **`Redirect`** — immediate chain termination. The `ChainResult` outcome is `Redirect { target }`. The caller routes to the specified target.

Helper constructors: `HookResult::pass()`, `pass_with(updates)`, `reject(reason)`, `reject_with_code(reason, code)`, `redirect(target)`.

`is_terminal()` returns `true` for `Reject` and `Redirect` — used by `ChainExecutor` to detect short-circuit conditions.

---

### `HookContext`

The context object threaded through a chain. Hooks read the fields they need and ignore the rest. `context_updates` from `Continue` results are accumulated into `metadata` as the chain progresses.

```rust
pub struct HookContext {
    pub point: HookPoint,                                    // Always present
    pub event: Option<serde_json::Value>,                    // Present for route/turn hooks
    pub session: Option<serde_json::Value>,                  // Present for turn/tool/memory hooks
    pub tool_call: Option<serde_json::Value>,                // Present for pre_tool/post_tool
    pub tool_result: Option<serde_json::Value>,              // Present for post_tool only
    pub principal: Option<serde_json::Value>,                // Who is performing the action
    pub metadata: HashMap<String, serde_json::Value>,        // Accumulated hook outputs
    pub change_artifact: Option<ChangeArtifactId>,           // Present for evolution hooks
}
```

`HookContext::new(point)` creates a minimal context with all optional fields set to `None` and `metadata` empty. Callers populate the relevant fields before passing to `execute_at_point`.

---

### `HookMetadata`

Describes a hook module's identity and capabilities. Returned by `Hook::metadata()` and stored as the registry key.

```rust
pub struct HookMetadata {
    pub name: String,                       // Registry key — must be unique
    pub description: String,
    pub version: String,                    // Semantic version
    pub supported_points: Vec<HookPoint>,   // Points this hook can be used at
    pub author: Option<String>,
}
```

`HookRegistry` uses `name` as the lookup key. Re-registering a hook with the same name replaces the previous entry.

---

### `ChainResult`

The return value of `execute_chain` and `execute_at_point`.

```rust
pub struct ChainResult {
    pub context: HookContext,    // Final context after all hooks ran (with accumulated metadata)
    pub outcome: HookResult,     // Continue (full chain) or the terminal Reject/Redirect
    pub hooks_executed: usize,   // Count of hooks that actually ran (disabled/skipped not counted)
    pub duration_ms: u64,        // Total wall-clock time for the chain(s)
}
```

Convenience predicates: `is_success()` (`outcome` is `Continue`), `is_rejected()`, `is_redirected()`.

Note: disabled hooks are not counted in `hooks_executed`. Fail-open skipped hooks (missing or erroring) *are* counted because the executor reaches them and decides to skip.

---

## `Hook` Trait (`hook_trait.rs`)

The interface every in-process hook must implement. Uses `async_trait` because the trait methods are async.

```rust
#[async_trait]
pub trait Hook: Send + Sync {
    fn metadata(&self) -> HookMetadata;

    async fn init(&mut self, config: serde_json::Value) -> Result<(), HookError>;

    async fn execute(&self, ctx: &HookContext) -> Result<HookResult, HookError>;
}
```

- **`metadata()`** — synchronous, called at registration time to get the hook's name and capabilities.
- **`init(config)`** — called once when the hook is registered or reconfigured. `config` is the per-instance JSON block from `HookInstance::config`. Use this to parse and validate parameters, open connections, or compile patterns.
- **`execute(ctx)`** — called per invocation. Receives a shared reference to the current `HookContext`. Must not mutate global state; all output goes through the returned `HookResult::Continue { context_updates }`.

`Send + Sync` bounds are required because `ChainExecutor` holds hooks behind `Arc<HookRegistry>` and calls them from async tasks.

**WASM note:** WASM hooks are a planned future extension. A `WasmHookAdapter` struct will implement `Hook` by delegating to the WASM runtime, making the executor WASM-unaware. The `async_trait` annotation in CLAUDE.md learnings documents this design decision.

---

## `HookRegistry` (`registry.rs`)

A `HashMap<String, Box<dyn Hook>>` keyed by hook name. Not thread-safe on its own; callers wrap in `Arc` (read path) or `RwLock` (write path) as needed.

```rust
pub struct HookRegistry { /* private */ }
```

### Methods

| Method | Description |
|---|---|
| `new() -> Self` | Create an empty registry. |
| `register(hook: Box<dyn Hook>)` | Register a hook. Replaces any existing hook with the same name. Logs at `DEBUG`. |
| `get(name: &str) -> Option<&dyn Hook>` | Look up a hook by name. Returns a shared reference. |
| `unregister(name: &str) -> bool` | Remove a hook. Returns `true` if it existed. Logs at `DEBUG`. |
| `list() -> Vec<HookMetadata>` | Returns owned metadata copies for all registered hooks. Order is unspecified (HashMap). |
| `contains(name: &str) -> bool` | Check whether a hook is registered. |

The typical setup pattern:

```rust
let mut registry = HookRegistry::new();
registry.register(Box::new(RateLimiterHook::new()));
registry.register(Box::new(ContentFilterHook::new()));
let registry = Arc::new(registry);
// Registry is now read-only — pass Arc to ChainExecutor
```

---

## `ChainExecutor` (`executor.rs`)

Executes hook chains using a shared `Arc<HookRegistry>`. Holds no mutable state; all execution state is local to each call.

```rust
pub struct ChainExecutor {
    registry: Arc<HookRegistry>,
}
```

### `execute_chain`

```rust
pub async fn execute_chain(
    &self,
    chain: &HookChain,
    ctx: HookContext,
) -> Result<ChainResult, HookError>
```

Runs all enabled hooks in `chain.hooks` sequentially against `ctx`. Returns `ChainResult` on success, `HookError` on fatal failure.

**Execution rules (in order of precedence):**

1. Disabled hook instance → skip, no registry lookup.
2. `elapsed >= chain.timeout_ms` before a hook → `HookError::ChainTimeout`.
3. Hook not in registry + `fail_open=false` → `HookError::HookNotFound`.
4. Hook not in registry + `fail_open=true` → warn, skip.
5. `hook.execute()` returns `Err` + `fail_open=false` → propagate `HookError`.
6. `hook.execute()` returns `Err` + `fail_open=true` → warn, skip.
7. `hook.execute()` times out (tokio timeout on remaining budget) + `fail_open=false` → `HookError::ChainTimeout`.
8. `hook.execute()` times out + `fail_open=true` → warn, skip.
9. `hook.execute()` returns `Continue { context_updates }` → merge updates into ctx, continue.
10. `hook.execute()` returns `Reject` or `Redirect` → return `ChainResult` immediately (short-circuit).

After all hooks run, performs a final timeout check (guards against the last hook consuming exactly the budget).

### `execute_at_point`

```rust
pub async fn execute_at_point(
    &self,
    point: HookPoint,
    chains: &[HookChain],
    ctx: HookContext,
) -> Result<ChainResult, HookError>
```

Filters `chains` to those matching `point`, then runs them sequentially. Context (including accumulated metadata) flows from one chain into the next. Stops immediately if any chain returns `Reject` or `Redirect`.

If no chains match `point`, returns a pass-through `ChainResult` with `hooks_executed: 0` and `duration_ms: 0` — no allocation, no registry lookup.

---

## `HookError` (`error.rs`)

All failure modes from the registry and executor. Uses `thiserror`.

| Variant | When |
|---|---|
| `HookNotFound { name }` | `registry.get(name)` returned `None` and `fail_open=false` |
| `InitFailed { hook, reason }` | `hook.init()` returned `Err` |
| `ExecutionFailed { hook, reason }` | `hook.execute()` returned `Err` |
| `ChainTimeout { chain, elapsed_ms }` | Chain deadline exceeded (pre-hook check or tokio timeout) |
| `HookTimeout { hook, elapsed_ms }` | Reserved — individual hook timeout tracking (not yet emitted) |
| `InvalidHookPoint { hook, point, supported }` | Hook wired to a point it doesn't support |

`ChainTimeout` is the only variant emitted by `execute_chain` for deadline overruns. `HookTimeout` is defined for future per-hook timeout tracking distinct from chain-level timeouts.

---

## Public API Surface

```rust
// Re-exports from sera_hooks root
pub use error::HookError;
pub use executor::ChainExecutor;
pub use hook_trait::Hook;
pub use registry::HookRegistry;

// Types from sera_types::hook (not re-exported from sera-hooks)
// use sera_types::hook::{HookChain, HookContext, HookInstance, HookMetadata,
//                        HookPoint, HookResult, ChainResult};
```

---

## Usage Example

```rust
use std::sync::Arc;
use sera_hooks::{ChainExecutor, Hook, HookRegistry};
use sera_types::hook::{HookChain, HookContext, HookInstance, HookPoint};

// 1. Build the registry (typically once at startup)
let mut registry = HookRegistry::new();
registry.register(Box::new(RateLimiterHook::new()));
registry.register(Box::new(ContentFilterHook::new()));
let executor = ChainExecutor::new(Arc::new(registry));

// 2. Load chain definitions (from YAML manifests at runtime)
let chains: Vec<HookChain> = load_chains_from_config();

// 3. Execute at a hook point (called per request)
let ctx = HookContext {
    point: HookPoint::PreRoute,
    event: Some(serde_json::to_value(&incoming_event)?),
    ..HookContext::new(HookPoint::PreRoute)
};

let result = executor.execute_at_point(HookPoint::PreRoute, &chains, ctx).await?;

if result.is_rejected() {
    return Err(anyhow::anyhow!("request rejected by hook chain"));
}

// result.context.metadata contains any updates written by hooks
```

---

## Test Coverage

`src/tests.rs` provides integration-style tests using five test hook implementations:

| Hook | Behaviour |
|---|---|
| `PassthroughHook` | Always returns `Continue` with no updates |
| `RejectHook` | Always returns `Reject` |
| `RedirectHook` | Always returns `Redirect { target: "agent:fallback" }` |
| `ModifyingHook` | Returns `Continue { context_updates: {"modified": true} }` |
| `FailingHook` | Returns `Err(ExecutionFailed)` |

Tests cover: empty chain, single hook, multi-hook, short-circuit on reject, short-circuit on redirect, fail-open skipping erroring hooks, fail-closed error propagation, disabled hooks skipped, context modifications propagating, missing hook fail-closed, missing hook fail-open, chain timeout, `execute_at_point` point filtering, no-match pass-through, multi-chain sequential execution, and chain-level reject stopping subsequent chains.
