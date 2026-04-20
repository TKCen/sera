# SERA 2.0 — Phase 0 / Phase 1 Implementation Status

> **Document Status:** Updated 2026-04-20 — Session-closeout snapshot  
> **Scope:** Phase 0 (P0-1 … P0-10, M0–M4) and Phase 1 in-flight work  
> **Basis:** Live `cargo` + `gh` output; git log on `main` as of this write

---

## 1. Executive summary

**Phase 0: CLOSED (to gate).** All M0–M4 exit criteria are reached on `main` except one item (apalis production worker wiring, P0-4), which has been explicitly deferred to Phase 1 as bead `sera-wdjx` because `LocalQueueBackend` already satisfies the P0-4 acceptance contract.

**Phase 1: Substantially advanced.** The four Phase-1 foundation lanes shipped in this session:

| Lane | PR | Scope |
|------|----|-------|
| A — `sera-events` delete | #968 | Centrifugo/JWT out of the workspace, all sera-gateway call sites migrated to sera-telemetry |
| B — feature matrix | #967 | `cargo check --workspace` green on default / `--no-default-features` / `--features enterprise`; CI script at `scripts/check-feature-matrix.sh` |
| C — WASM runtime | #969 | `wasmtime >=43 <50` + WASIp1 + fuel + epoch interruption in `WasmSandboxProvider`, gated by `wasm` feature |
| E — HITL P0-10 | #976 | `SecurityAnalyzer` trait, `GuardianAssessment`, `AskForApproval`, `GranularApprovalConfig`, `RevisionRequested` state — +14 tests (76 total) |
| G — ConstitutionalGate | #977 | Fail-closed enforcement at `_observe` / `_react`; new `allow_missing_constitutional_gate` opt-in config; +4 acceptance tests |
| H — WorkflowEngine | #979 | Async `WorkflowEngine` with rusqlite durable store + orphan recovery — matches project SQLite ADR, **no sqlx** |
| I — config hot-reload | #974 | `notify-debouncer-mini` watcher → schema validation → atomic `RwLock` swap → `ConfigReloaded` / `ConfigReloadFailed` broadcast |

Also landed: #966 (NDJSON stdio transport for `sera-runtime`), #964 (sccache in CI), #970 (CLAUDE.md Working Principles), #975 (CONTRIBUTING.md), #978 (`examples/local-agent/`).

### Workspace state on `main`

```
cargo check --workspace           → green (0 errors)
cargo test --workspace            → >2,900 tests passing across 32 crates
cargo clippy --workspace -- -D warnings → clean
```

Feature matrix verified once per PR via `scripts/check-feature-matrix.sh`:

```
cargo check --workspace
cargo check --workspace --no-default-features
cargo check --workspace --features enterprise
```

---

## 2. Milestone map

| Milestone | Status | Notes |
|-----------|--------|-------|
| **M0** — `sera-types` first-mover | ✅ REACHED | Rename complete, 15 tests, `HookPoint::ALL.len() == 20` |
| **M1** — Infrastructure foundations | ✅ REACHED | sera-events gone, sera-telemetry canonical, feature matrix locked, sera-config hot-reload live |
| **M2** — Gateway + runtime spine | ✅ REACHED | NDJSON stdio (#966), ConstitutionalGate fail-closed (#977), 26 gateway tests |
| **M3** — Workflow + auth typed | ✅ REACHED | Auth 95% + workflow engine with rusqlite persistence landed |
| **M4** — Full Phase 0 gate | ✅ REACHED | All matrix configs green, no `TurnResult` / `sera_domain` refs, zero plaintext auth paths |

---

## 3. Phase 1 — what's next

Remaining Phase 1 work is tracked as beads (visible via `bd ready`) rather than inline in this document, to avoid tracker rot:

| Bead | Scope |
|------|-------|
| `sera-50y1` | Dedicated `sera-memory` crate: `SemanticMemoryStore` trait + `SqliteFtsMemoryStore` default + `PgVectorStore` enterprise + plugin hooks |
| `sera-s4b1` | `sera-hooks` WIT interface + wit-bindgen for third-party WASM hooks |
| `sera-y9d0` | `sera-gateway` admin kill-switch (Unix socket) + two-layer session persistence (PartTable + shadow git) |
| `sera-wdjx` | `apalis` production worker integration on top of `LocalQueueBackend` (apalis 0.7.x API + cron) |
| `sera-lilw` | `sera-config` hot-reload (CLOSED via #974) |
| `sera-4yz5` | OSS launch readiness: README/LANDING/"Why SERA?" docs before announcement |

### Adjacent but non-blocking

These were deferred intentionally and filed as beads — they are *not* on the Phase 0/1 critical path:

- **Secrets enterprise providers (Vault, AWS SM, Azure KV)** — scaffold present; enterprise-tier work.
- **Meta-change integration in HITL** — `MetaChangeContext` deliberately left out of P0-10 HITL PR; belongs with SPEC-self-evolution.
- **`sera-gateway` route wrapping to Submission emitters** — most routes call services directly; envelope wrapping is a P1 refactor.

---

## 4. Per-crate status (sorted by phase)

| Crate | Phase | Status | Evidence |
|-------|-------|--------|----------|
| `sera-types` | 0 | ✅ DONE | 15 tests; 20 hook points; `TurnOutcome` 8-variant |
| `sera-errors` | 0 | ✅ DONE | 248 LoC; unified error codes |
| `sera-cache` | 0 | ✅ DONE (scaffold) | MokaBackend complete; Redis Phase-1 |
| `sera-secrets` | 0 | ✅ DONE (scaffold) | Env / File / Chained; Vault deferred |
| `sera-telemetry` | 0 | ✅ DONE | OTel triad pinned; `AuditBackend` hash chain; sole observability crate after #968 |
| `sera-config` | 0 | ✅ DONE | `SchemaRegistry`, `ShadowConfigStore`, `ConfigVersionLog`, hot-reload via #974 |
| `sera-db` | 0 | ✅ DONE | rusqlite; `MigrationKind`; pgvector enterprise path |
| `sera-queue` | 0 | ✅ DONE | `LocalQueueBackend` + `GlobalThrottle`; 24 tests; apalis integration filed as bead |
| `sera-tools` | 0 | ✅ DONE | 5 sandbox providers; WASM via #969 behind `wasm` feature; `SsrfValidator` |
| `sera-auth` | 0 | ✅ DONE | Argon2 PHC, casbin, `CapabilityToken::narrow()`; 12 tests |
| `sera-workflow` | 0/1 | ✅ DONE | 14 task tests + WorkflowEngine rusqlite via #979 |
| `sera-gateway` | 0 | ✅ DONE | `AppServerTransport` 6-variant; admin socket filed as bead |
| `sera-runtime` | 0 | ✅ DONE | Four-method lifecycle; NDJSON stdio (#966); ConstitutionalGate (#977) |
| `sera-hitl` | 0/1 | ✅ DONE | Completed via #976; 76 tests |
| `sera-hooks` | 1 | PARTIAL | `HookPoint::ConstitutionalGate` enforced; WIT interface filed as bead |
| `sera-session` | 0 | ✅ DONE | 6-state machine, 4 memory tiers |
| `sera-testing` | 0 | ✅ DONE | Mocks for QueueBackend + SandboxProvider |
| `sera-{meta, mcp, a2a, agui, plugins, skills, models, oci, commands}` | 0 | ✅ SCAFFOLD | All present; Phase-3 interop deferred for a2a / agui / mcp |

---

## 5. Deferred items (filed as beads)

See `bd ready | grep Phase` for the live list. Summary:

- apalis workers (sera-wdjx)
- sera-memory crate (sera-50y1)
- sera-hooks WIT (sera-s4b1)
- gateway admin socket (sera-y9d0)
- OSS launch docs (sera-4yz5)

---

*Tracker maintained on close of 2026-04-20 session. The next session should run `bd ready` and pick from the Phase 1 bead pool. This file is deliberately short — per-change history lives in `git log`, not here.*
