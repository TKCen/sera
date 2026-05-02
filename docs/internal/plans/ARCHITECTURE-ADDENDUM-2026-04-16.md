# Architecture Addendum — 2026-04-16 (Hermes Comparison)

> **Purpose:** Architectural decisions from a Hermes ↔ SERA comparison review.
> These decisions refine (not replace) the 2026-04-13 addendum.
> Each decision is cross-referenced to the spec sections it produced or modified.

---

## What Changed and Why

### 1. Two-Tier Memory Injection Model

**Decision:** SERA adopts Hermes's proven 2-tier memory model as the default injection strategy: a compact `MemoryBlock` (always injected into context) + semantic search (on-demand retrieval). The existing four-tier working-memory ABC remains for eviction strategy but is orthogonal to injection.

**Why:** The original spec had four working-memory tiers, embedding-based search, dreaming promotion, and a complex injection pipeline — all designed before any of it was tested. Hermes proved that a simple 2-tier model (compact block + search) covers 95% of use cases. Additional tiers should be added only when the 2-tier model demonstrably breaks down.

**What changed:**
- New `MemoryBlock` struct with `priority`, `recency_boost`, and `char_budget` fields
- `MemorySegment` as the unit of injected context — priority-ordered, budget-constrained
- `flush_min_turns: 6` trigger: when the compact block exceeds budget for 6 consecutive turns, emit `memory_pressure` event
- Memory pressure can trigger dreaming, skill creation prompts, or operator notification

**Spec impact:** SPEC-memory §2.1 (new section — Two-Tier Injection Model).

**Key invariants:**
- The `MemoryBlock` is assembled gateway-side (cattle mode) or runtime-side (pet mode) — the LLM never knows the difference
- Soul content (persona) is always priority 0 — it never gets evicted from the block
- The four-tier working-memory ABC governs session history eviction, not context injection

---

### 2. Native-First Hook Strategy (WASM Opt-In)

**Decision:** Built-in hooks are plain Rust trait implementations (`impl Hook for ContentFilter`). WASM sandboxing is opt-in for third-party isolation only. The `ChainExecutor` is agnostic to implementation — native and WASM hooks coexist in the same chain.

**Why:** The original spec described all hooks as "WASM-based processing pipelines." This is over-engineered for hooks that ship with the binary. WASM adds serialization overhead, complicates debugging, and provides sandboxing guarantees that are unnecessary for code we compile ourselves. Hermes uses plain function hooks for built-in behaviour and only sandboxes third-party extensions.

**What changed:**
- Built-in hooks (content filter, rate limiter, secret injector, PII redactor, risk checker) are native Rust
- WASM runtime is used only for: third-party hooks, hot-reloaded hooks, untrusted operator code
- Subprocess hooks remain as the language-agnostic escape hatch
- Decision matrix added to SPEC-hooks §1b

**Spec impact:** SPEC-hooks §1 (overview updated), §1b (new section — Native-First strategy), §5 (retitled to emphasize third-party scope), §6.1 (clarified scope).

**Key invariants:**
- The `Hook` trait is the same for native and WASM — `ChainExecutor` doesn't distinguish
- WASM hooks use `WasmHookAdapter` which implements the same `Hook` trait
- Native hooks have zero serialization overhead — they receive `&HookContext` directly

---

### 3. Shared Command Registry (`sera-commands`)

**Decision:** Add a `sera-commands` foundation crate that provides a unified command registry shared between the CLI and the gateway. Commands are defined once and dispatched from either entrypoint.

**Why:** Hermes shares `COMMAND_REGISTRY` across its CLI and gateway, avoiding command definition duplication and ensuring feature parity. SERA currently has no shared command surface — the CLI and gateway define their commands independently, leading to drift.

**What changed:**
- New `sera-commands` crate in the Foundation layer
- Depends on `sera-types` and `clap` (for CLI argument parsing)
- Gateway imports commands for HTTP/gRPC dispatch; CLI imports the same commands for terminal dispatch
- Each command is a struct implementing a `Command` trait with `execute()`, `describe()`, and argument schema

**Spec impact:** SPEC-crate-decomposition §2, §3, §4, §6.1 (sera-commands added throughout).

---

### 4. Skill Self-Patching Loop

**Decision:** `sera-skills` gains a self-patching capability: agents can propose skill edits via a `skill_manage patch` pattern, which are validated and applied in a closed loop.

**Why:** Hermes's skill system self-patches via `skill_manage patch` — an agent notices a recurring pattern, extracts it as a skill, and the system validates and installs it. SERA lacked this closed-loop self-improvement path. Combined with the `flush_min_turns` memory pressure signal (§1), this enables: memory overflow → skill extraction → knowledge compaction.

**What changed:**
- `sera-skills` crate description updated to include self-patching loop
- Memory pressure (`flush_min_turns`) can trigger skill creation prompts
- Fits within Tier 1 self-evolution (SPEC-self-evolution §2.1) — agent improves within its own workspace

**Spec impact:** SPEC-crate-decomposition §3 (`sera-skills` row updated).

---

### 5. Hook Point Naming Alignment

**Decision:** SERA hook points gain Hermes-aligned aliases for cross-project legibility: `context_memory` → alias `pre_agent_turn`, mapping to Hermes's `prefetch_all`.

**Why:** Both systems fire hooks at the same lifecycle points but use different names. Adding aliases (not renaming) improves documentation cross-referencing without breaking existing config.

**Spec impact:** SPEC-memory §9 (alias added to hook point table).

---

## Philosophy

> **Start with the simplest model that works. Add complexity only when it demonstrably breaks down.**

These decisions all follow the same principle: Hermes proved that simpler approaches work in production. SERA's original specs were designed from first principles with maximum extensibility; this addendum grounds them in proven patterns.

| Area | Before (SERA original) | After (Hermes-informed) |
|---|---|---|
| Memory injection | Four-tier ABC + embedding search + dreaming | 2-tier: compact block + semantic search. Four tiers remain for eviction only. |
| Hook implementation | All WASM, always sandboxed | Native Rust by default, WASM opt-in for 3rd party |
| Command surface | CLI and gateway define independently | Shared `sera-commands` crate |
| Skill evolution | Manual skill pack loading | Self-patching loop with memory pressure trigger |
| Hook naming | SERA-specific names only | Aliases for Hermes cross-referencing |

---

## Cross-References

| Decision | Specs Modified |
|---|---|
| Two-Tier Memory | SPEC-memory §2.1, §9, §12 |
| Native-First Hooks | SPEC-hooks §1, §1b, §5, §6.1 |
| sera-commands | SPEC-crate-decomposition §2, §3, §4, §6.1; ARCHITECTURE-2.0 §2 |
| Skill Self-Patching | SPEC-crate-decomposition §3 |
| Hook Naming | SPEC-memory §9 |
