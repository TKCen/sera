# ADR — Tool Dispatch Ownership

> **Status:** ACCEPTED — 2026-04-29
> **Bead:** [`sera-y45a`](../../../). Anchor for the migration.
> **Authors:** SERA architecture lane (Claude burn + OMC coordination council).
> **Scope:** documentation + a single boot-log line. **No** runtime/gateway tool-dispatch migration in this PR.

---

## 1. Context

`SPEC-gateway.md §1b`, `SPEC-runtime.md §1a`, `SPEC-tools.md §6` and `ARCHITECTURE-ADDENDUM-2026-04-13.md §3` all assert the same architectural target:

- **Tool dispatch belongs entirely to the gateway.**
- **Credentials never reach the harness/runtime.**
- **Runtimes are cattle** — they may be restarted, cloned, or replaced without data loss.

The shipped MVS does the opposite. The gateway spawns `sera-runtime --ndjson` as a per-agent child process via `StdioHarness` and the runtime owns:

- the LLM client and `LLM_API_KEY` env (`rust/crates/sera-runtime/src/llm_client.rs`, spawn env at `rust/crates/sera-gateway/src/bin/sera.rs`),
- the tool registry and dispatcher (`rust/crates/sera-runtime/src/tools/dispatcher.rs` — `RegistryDispatcher::dispatch`),
- the capability gate (`with_capability_registry` + `cap_registry.check(...)` *inside the runtime process*).

The gateway's `enforce_tool_events` (`rust/crates/sera-gateway/src/bin/sera.rs`) only observes `tool_call_begin/end` events post-hoc as defence-in-depth audit; the file-level comment in `rust/crates/sera-gateway/src/capability_enforcement.rs` explicitly disclaims the spec ("*Pre-dispatch enforcement now lives inside `sera-runtime`*"). This is the largest spec/code drift in the gateway↔runtime story.

The drift is load-bearing because three downstream items depend on the answer:

- `sera-eq0m` — netns-based egress containment for the LLM/provider; only meaningful if the gateway is the egress process.
- `sera-plcv` — Pattern A/B BYOH delegation. M1 acceptance "all LLM cost attributed to the SERA agent's budget at the proxy" is unreachable while runtimes hold provider keys.
- `sera-w2dh` / future BYOH tool injection — there is no shared dispatcher to inject `knowledge_query` / `memory_write` into.

This ADR records the deviation, names the migration target, and adds a single boot-log marker so operators can see which model is live without reading source.

## 2. Decision

**Adopt gateway-owned dispatch as the canonical target. Frame today's MVS as a transitional `dispatch_mode=runtime` operating mode. No behaviour change in this PR beyond a structured boot log line per agent and per process.**

Three modes are defined:

| Mode | Meaning | Status |
|---|---|---|
| `runtime` | The runtime process owns the LLM client, tool registry, capability gate, and dispatcher. The gateway only audits `tool_call_*` events post-hoc. **This is the shipped MVS.** | **Current default.** |
| `gateway` | The runtime forwards `tool_call` events through SQ/EQ; the gateway runs `pre_tool` chain, `CapabilityRegistry`, dispatch, `post_tool`, and returns the result. The runtime never holds credentials. | Target architecture (matches SPEC-gateway §1b, SPEC-tools §6). Not implemented. |
| `embedded` | The gateway constructs `DefaultRuntime` in-process and calls `execute_turn` directly — no NDJSON, no child process. By process identity, "runtime crate dispatches" becomes "the gateway process dispatches"; the same hooks, `CapabilityRegistry`, and audit emission apply as for the binary path. | Library-mode entry point (matches `SPEC-runtime §1a` "Library mode (default)"). Not implemented. |

The transitional `runtime` mode is **explicit, logged, and time-bounded by the migration plan in §4**. It is not a permanent second architecture.

## 3. Why temporary

1. **Spec consistency.** Every existing reference (SPEC-gateway §1b/§1c/§13a, SPEC-runtime §1a, SPEC-tools §6, ADDENDUM §3/§9) names the gateway as the dispatch + credentials + AuthZ + audit owner. Inverting the spec to match the MVS would also collapse the MES framing (gateway as durable boundary, workers as cattle), break the universal LLM proxy compliance claim, and remove the architectural anchor for BYOH tool injection.
2. **Single security model.** Two production paths means two test surfaces, two threat models, and two audit pipelines. The team is not large enough to maintain that long-term.
3. **Library mode is the quiet path.** When `sera-runtime` is embedded as a library inside the gateway process, "runtime crate dispatches" *is* "the gateway process dispatches" by virtue of process identity. The current binary `StdioHarness` is an MVS bootstrap, not the target shape.

The pragmatic landing is therefore two-step: this PR records the deviation; a later PR (and a follow-up gateway-LLM-proxy PR) actually moves dispatch.

## 4. Migration plan

| Step | Bead | What lands |
|---|---|---|
| 1 (this PR) | `sera-y45a` | ADR + spec annotations + `dispatch_mode={runtime,gateway,embedded}` boot log line per agent and per process. **No** behavioural change. |
| 2 | new bead — `feat(sera-gateway,sera-runtime): embedded-library dispatch_mode=embedded entry point` | Add `DispatchMode::Embedded`. Gateway constructs `DefaultRuntime` in-process and calls `execute_turn` directly — no NDJSON, no child. Reuses the same hook chain, `CapabilityRegistry`, and audit emission. Gated behind config flag. |
| 3 | new bead — `feat(sera-gateway): move LlmClient to gateway as inference.local universal proxy` | Move `LlmClient` to the gateway, introduce `inference.local` virtual-host routing, strip `LLM_API_KEY` from runtime spawn env. Gated on `dispatch_mode=embedded`. Unblocks `sera-eq0m` and `sera-plcv` acceptance criterion 4. |
| 4 | follow-up | Flip default `dispatch_mode` to `embedded` once `sera-ov7z`-equivalent E2E passes for both modes. Retire or repurpose the binary `StdioHarness` path (kept only for external BYOH harnesses if useful). |

### Migration gate (acceptance for **flipping the default**)

The default `dispatch_mode` flips from `runtime` to `embedded` only when **all** of the following hold:

1. The embedded path passes the production-path E2E (`sera-ov7z`) — same assertions, same fixtures.
2. The gateway-side LLM proxy is the sole egress for provider traffic; the runtime cannot dial a provider directly even if env-leaked (verified by netns enforcement test).
3. `pre_tool` / `post_tool` hook chains fire from a single dispatcher process; no parallel dispatcher exists in the binary path.
4. Audit emission for `tool_call_begin/end` is byte-stable across both modes (so historical audit logs remain comparable).
5. `cargo clippy --workspace -- -D warnings` clean; `cargo test --workspace` green.

Until all five hold, `dispatch_mode=runtime` remains the default and the boot log surfaces it explicitly so operators can distinguish staging from production.

## 5. Boot-log contract

The gateway emits two structured fields so the log cannot lie about what the running binary actually does:

- `dispatch_mode` — the **effective** mode (what the running code does). Today this binary always spawns `sera-runtime` via `StdioHarness`, so the effective mode is always `runtime` regardless of operator request. Future migration PRs (§4 steps 2–4) replace the constant returning `"runtime"` with a real switch wired to the dispatcher selection.
- `dispatch_mode_configured` — the **operator-requested** mode parsed from `SERA_DISPATCH_MODE` (default `runtime`). One of `runtime | gateway | embedded`. Unrecognised values silently fall back to `runtime` so a typo cannot accidentally claim a security model the code does not implement.

Both fields are emitted:

- **Per process at startup**, immediately before agent harness spawn (one info-level line, even when no agents are configured).
- **Per agent at spawn time**, alongside `agent` and `model` on the existing `"Spawned runtime harness"` line.

When `dispatch_mode_configured` names a not-yet-implemented mode (`gateway` or `embedded`) and therefore differs from `dispatch_mode`, the gateway also emits a single warn-level line at process startup so operators cannot misread the request as an active deployment.

The accepted configured set is documented here and validated by unit tests in the gateway crate; future migration PRs extend both the validator and the effective-mode resolver in lock-step with the dispatcher implementation.

The label is **declarative**, not behavioural: this PR does not change which process executes tools. It only makes the active model visible while keeping the effective and requested modes distinguishable.

## 6. Consequences

- **Specs no longer silently contradict working MVS behavior.** SPEC-gateway §1b, §1c, §13a; SPEC-runtime §1a, §3; SPEC-tools §6 each cite this ADR and name the active mode applicability.
- **Operators can read the live security model from boot logs.** No source-diving required to know whether tool dispatch crosses a process boundary.
- **`sera-hwny`** (already merged as PR #1119) is mandatory regardless of mode. In `runtime` mode the filter narrows the LLM-visible schema in the runtime; in `embedded`/`gateway` mode the same filter applies at the gateway-side `TraitToolRegistry` constructor.
- **`sera-ov7z`** must observe the active `dispatch_mode` log line and assert dispatcher-side hook ordering for that mode. Test fixtures are parameterised on `dispatch_mode` so the same test exercises both paths once the embedded entry point lands.
- **`sera-eq0m`** netns enforcement target depends on this ADR. In `runtime` mode the runtime process must run inside the egress-restricted netns. In `embedded` mode only the gateway process needs the netns.
- **`sera-plcv`** M1 acceptance criterion 4 ("all LLM cost attributed to the SERA agent's budget at the proxy") is reachable only after step 3 of §4. Until then, M1 driver is staged with manual cost-attribution shims.

## 7. Alternatives considered

- **Option A — mode duality (pet keeps `runtime`, cattle gets `gateway`).** Rejected. Two production paths means two security models and two audit pipelines forever; pet-mode operators still read SPEC §1b and are still misled.
- **Option C — accept runtime-side dispatch as canonical and rewrite the specs.** Rejected. Inverts the canonical constraint set, breaks the universal-LLM-proxy compliance claim (`sera-eq0m` becomes pointless), breaks BYOH tool injection (no shared dispatcher), and collapses the MES framing.

## 8. References

- `docs/plan/specs/SPEC-gateway.md` §1b, §1c, §13a
- `docs/plan/specs/SPEC-runtime.md` §1a, §3
- `docs/plan/specs/SPEC-tools.md` §6
- `docs/plan/ARCHITECTURE-ADDENDUM-2026-04-13.md` §1–§3, §9
- `artifacts/reports/research/y45a-dispatch-ownership-adr-preflight-spark-2026-04-29.md`
- `artifacts/reports/claude-burn/gateway-runtime-dispatch-gap-2026-04-29.md`
- `artifacts/reports/claude-burn/sera-claude-burn-synthesis-2026-04-29.md`
- `artifacts/reports/coordination/dispatch-ownership-adr-prep-2026-04-29.md`
