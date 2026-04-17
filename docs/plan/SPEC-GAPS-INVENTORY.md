# SPEC Gaps Inventory (bead sera-epnr)

Classification of P3 spec gaps from `.omc/plans/code-introspection-audit.md` §Category 6.
Legend: Effort S (< 1h) / M (1-4h) / L (1+ day). Ready = landable in current session.

## SPEC-runtime

### ToolUseBehavior
- **Current state:** Missing entirely. No matches for `ToolUseBehavior` anywhere in the Rust workspace.
- **File anchor:** Would belong in `rust/crates/sera-runtime/src/tools/` or `rust/crates/sera-types/src/tool.rs`.
- **Acceptance:** Enum or trait modelling OpenAI-style `tool_choice` modes (`Auto`, `Required`, `None`, `Specific{name}`) threaded into the LLM request path.
- **Effort:** M. **Ready:** N.

### HarnessSupportContext
- **Current state:** Partially present — struct exists with `agent_id` and `tier` only; `supports()` ignores context and always returns `Supported`.
- **File anchor:** `rust/crates/sera-runtime/src/harness.rs:16`.
- **Acceptance:** Populate fields the spec calls for (capability flags, model id, runtime features) and make `supports()` actually inspect them.
- **Effort:** S. **Ready:** Y (partial — extend fields + predicate logic).

### PlanAndAct
- **Current state:** Missing entirely. No matches for `PlanAndAct` or `plan_and_act`.
- **File anchor:** Would belong under `rust/crates/sera-runtime/src/` (new module) alongside `turn.rs` or `default_runtime.rs`.
- **Acceptance:** Multi-phase reasoning loop with distinct plan / act phases, plan persisted across turns.
- **Effort:** L. **Ready:** N.

## SPEC-gateway

### LSP routing
- **Current state:** Stubbed routes exist that explicitly return `Err(... "not yet implemented")`.
- **File anchor:** `rust/crates/sera-gateway/src/routes/lsp.rs:42,66,100`.
- **Acceptance:** Spawn and manage `tower_lsp`-style language server processes per workspace, proxy LSP JSON-RPC.
- **Effort:** L. **Ready:** N.

### Process persistence
- **Current state:** Missing entirely (no process registry/table for managed child processes).
- **File anchor:** Would live in `rust/crates/sera-gateway/src/` (new `process_manager.rs`).
- **Acceptance:** Registry of running child processes with restart policy, persisted across gateway restarts.
- **Effort:** L. **Ready:** N.

### OIDC group-to-role mapping
- **Current state:** Implemented — `OIDC_ROLE_MAPPING` env-var parser exists and maps groups to operator roles.
- **File anchor:** `rust/crates/sera-gateway/src/routes/oidc.rs:212,244`.
- **Acceptance:** (Already met.) Gap may have referred to a more complete mapping model (transitive roles, YAML config). Treat as closed for now.
- **Effort:** S (if widening). **Ready:** N (no clear acceptance target).

## SPEC-hooks

### WASM fuel metering
- **Current state:** `wasm_adapter.rs` exists; no fuel/metering hooks visible.
- **File anchor:** `rust/crates/sera-hooks/src/wasm_adapter.rs`.
- **Acceptance:** `wasmtime` fuel consumption + per-chain budget + OOM trap to `HookError`.
- **Effort:** L. **Ready:** N.

### HookAbortSignal
- **Current state:** Missing — no `AbortSignal` / `ChainAbort` / `abort_signal` in `sera-hooks` or `sera-types`. `HookResult::Reject` short-circuits the current chain but cannot abort the parent op with a typed reason distinct from "reject this one chain".
- **File anchor:** `rust/crates/sera-types/src/hook.rs:149` (add variant or sibling type), `rust/crates/sera-hooks/src/executor.rs`.
- **Acceptance:** Distinct typed signal that a hook can raise to abort the entire pipeline (not just the current chain), surfaced through `ChainResult`/`HookError`.
- **Effort:** S. **Ready:** Y.

### Two-tier bus
- **Current state:** Missing entirely. `ChainExecutor` is single-tier.
- **File anchor:** `rust/crates/sera-hooks/src/executor.rs`.
- **Acceptance:** Fast-path in-process bus + slow-path async/persistent bus with fanout routing.
- **Effort:** L. **Ready:** N.

## SPEC-memory

### RAG / embedding search
- **Current state:** Scaffolded — `HybridScorer`, `embedding` field on messages, stub returning zero vectors. No real embedding service.
- **File anchor:** `rust/crates/sera-runtime/src/context_engine/hybrid.rs`, `pipeline.rs:207`.
- **Acceptance:** `EmbeddingService` trait + provider (OpenAI/Ollama) wired through `ContextPipeline`.
- **Effort:** L. **Ready:** N.

## SPEC-circles (7 coordination policies)

- **Current state:** Complete — all seven variants (`Sequential`, `Parallel`, `Pipeline`, `Debate`, `Council`, `Hierarchical`, `Consensus`) are implemented and dispatched by `Coordinator::run`.
- **File anchor:** `rust/crates/sera-workflow/src/coordination.rs:426`.
- **Acceptance:** Met.
- **Effort:** — **Ready:** N/A (close out gap in audit doc).

## SPEC-secrets (Vault / AWS / Azure providers)

- **Current state:** Stub structs with no trait impl — `VaultSecretsProvider`, `AwsSecretsProvider`, `AzureSecretsProvider` declared as doc anchors only; they do NOT implement `SecretsProvider`, which means they cannot be accidentally used.
- **File anchor:** `rust/crates/sera-secrets/src/enterprise.rs`.
- **Acceptance:** Verify that constructing the types and attempting to use them as providers is impossible today (compile-time guarantee via missing trait impl). Add regression tests that future trait impls must return `SecretsError::Provider` until real backends land.
- **Effort:** S. **Ready:** Y.

## Summary — selected quick wins for this session

1. **HookAbortSignal** — add typed abort to `sera-hooks` + unit test.
2. **HarnessSupportContext** — extend fields and wire real predicate + tests.
3. **Secrets enterprise contract** — compile-time doc-test proving stubs cannot be used as providers.
