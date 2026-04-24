# Architecture Addendum — 2026-04-24

**Trigger:** Wave J (chat TUI redesign) design session. User asked for a simplicity review of the architecture against the chat use case.

**TL;DR:** The gateway + runtime + 34-crate layering is earned complexity for SERA's stated commitments (enterprise durability, sandboxing, multi-tenancy, self-evolution, BYOH). The chat TUI is a thin client; it doesn't pay the cost. Local bootstrap DX is the one real simplicity issue. Some ancillary components (shadow-git session store default, HttpTransport stub, unused hook points) are over-engineered for current use and should be pruned or gated.

---

## 1. Is the architecture a good fit?

### The core split (gateway ↔ runtime) is earned

Reasons it exists, each independently load-bearing:

1. **Durability**: runtime crash ≠ state loss. Gateway is canonical.
2. **Sandbox tier boundary**: runtime containerized per tier; gateway trusts itself.
3. **Multi-tenant**: one gateway, N runtime processes, different agents, different policies.
4. **Self-evolution**: `CodeEvolution` scope means runtime can restart with new code while gateway keeps serving.
5. **BYOH**: any NDJSON-speaking subprocess is a valid runtime. Not Rust-locked.

None of these commitments are optional for SERA's positioning vs Python-native frameworks (LangChain, AutoGen, CrewAI). Removing the split regresses the value prop.

### For the chat TUI specifically

The TUI is a presentation client. It sees HTTP + SSE. It does not observe runtime subprocesses, NDJSON envelopes, lane queues, constitutional registry, or the session FSM. The heavy machinery is invisible from its vantage. **The chat TUI pays no architectural tax** — it pays a DX tax (bootstrap) and a small latency tax (one extra HTTP hop, invisible against LLM latency).

## 2. Simplicity issues worth addressing

### K.0 — Unified local bootstrap

Current: `scripts/sera-local` runs gateway and runtime as separate tokio tasks under one bash process, sets 5 env vars (`SERA_LLM_BASE_URL`, `SERA_LLM_API_KEY`, `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE`, `SERA_DATA_DIR`, `SERA_PORT`), picks port 42540.

Proposal: `sera start --local` in the `sera` binary embeds gateway + spawns runtime as managed subprocess. Reads config from `~/.sera/config.toml` or flags. Defaults auto-detect LM Studio / Ollama. No env vars required.

Impact: one-command dev loop. Lowers friction from 4 terminals (LLM, gateway script, TUI, logs) to 2 (LLM, `sera start --local & sera-tui`). **High DX value.**

### K.1 — Gate SqliteGitSessionStore behind enterprise feature

Current: gateway default uses `SqliteGitSessionStore` — shadow git repo commits every turn. Adds disk + latency per turn. Values durable diffable replay for constitutional audit; not needed for chat-first local.

Proposal: introduce a plain `SqliteSessionStore` (transcript rows in SQLite, no git shadow). Default path = `SqliteSessionStore`. `SqliteGitSessionStore` remains available behind `--features enterprise` or `SERA_SESSION_STORE=git-shadowed` env var.

Impact: reduces per-turn latency and disk footprint. Git-shadow is a real feature, just not the right default.

### K.2 — Prune or implement dead-weight stubs

- `A2A::HttpTransport` returns 501. Either implement (Wave B.3 scope) or delete. Currently it's dead code that looks like a feature.
- Hook points declared in `HookPoint` enum but never registered anywhere (~12 of 20). Enum shape is fine; active wiring should match the docs.
- QueueMode variants `SteerBacklog` / `Interrupt` — implemented but rarely exercised. Keep if roadmap wants them; document as reserved with test coverage if not used in production.
- Envelope `Op` variants `Register`, system sub-variants — same pattern.

Proposal: audit-and-reconcile. Each unused element gets a decision: implement, gate, or delete. No "it might be useful later" unless paired with a dated bead.

## 3. What is *actually right* (don't change)

- **Gateway as canonical API surface** — ratatui TUI, sera-cli, sera-web, 3rd-party all speak the same HTTP/SSE vocabulary.
- **Runtime as subprocess** — clean crash/upgrade/sandbox boundary.
- **sera-types canonical** — one shape per domain concept. Hard-won in Wave 3 cleanup.
- **6-state SessionStateMachine** — explicit lifecycle.
- **HITL ApprovalRouter as independent crate** — single source of truth; 76 tests.
- **Pluggable memory (SemanticMemoryStore trait)** — mem0/hindsight/pgvector all satisfy the interface.
- **Hook registry + chain executor** — good extension point for third parties.

## 4. Fit test applied to Wave J chat TUI

| Chat TUI need | Architecture ready? | Gap |
|---------------|--------------------|----|
| Stream tokens | ✅ `text_delta` SSE events | none |
| Stream tool calls | ✅ `tool_call_begin`/`end` | none |
| Sub-agent drill-in | ⚠️ handoff scaffold + InProcRouter; SSE needs `parent_task_id` correlation | new plumbing in gateway (Wave B.3 + J.2.4) |
| Inline HITL approval | ✅ Phase 1 routes live (sera-z6ql); Phase 2 resume pending | Phase 2 (follow-up bead) |
| ESC cancel turn | ⚠️ CancellationToken exists (sera-bsem); no `/api/chat/cancel` route | K.3 gateway bead |
| Persistent history | ✅ `/api/sessions/{id}/transcript` exists | none |
| Composer UX | ✅ pure client concern | none |
| Config file | ✅ pure client concern | none |

**Conclusion:** no architectural blockers. Three small gateway beads needed (K.0 bootstrap, K.3 cancel route, J.2.4 subagent SSE correlation). Everything else is TUI client work.

## 5. Recommendations

1. File Wave J beads (chat TUI client work).
2. File Wave K beads (architecture simplification: K.0 bootstrap, K.1 store gating, K.2 stub reconciliation, K.3 cancel route).
3. Implement J.0.1 (layout pivot) and K.0 (unified local) in parallel — they don't touch the same files.
4. Revisit this addendum after J.0 and K.0 ship. The chat TUI end-to-end working against `sera start --local` is the dogfood gate.

## 6. What would require an architectural rewrite (for context — not recommended)

If the chat TUI use case became the *only* use case and we wanted absolute minimum overhead:

- Collapse runtime into gateway (shared-memory tool dispatch). Loses BYOH + sandbox boundary.
- Embed gateway in the TUI binary (single-process mode). Loses web/3rd-party/multi-user.
- Skip the envelope protocol, use direct function calls. Loses plugin runtime support.

Each of these trades a SERA differentiator for marginal simplicity. **Don't do this.** The chat TUI is one of several clients; it should not dictate the server architecture.

---

**Related docs:**
- `SPEC-chat-tui.md` (Wave J spec)
- `TUI-ANALYSIS.md` (claw-code comparison + gap matrix)
- `ARCHITECTURE-2.0.md` (canonical architecture)
