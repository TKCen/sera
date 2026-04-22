# SERA 2.0 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-21
> **Session:** Phase 1 closeout session 2 — Python/TS extension-model design pass + Rust wiring
> **Previous handoff:** Session 28 (2026-04-17) → `git show cb77b8df:docs/plan/HANDOFF.md`. Earlier handoffs are chained from there. Decisions captured in prior handoffs still hold.

---

## Session outcome — 5 PRs merged

- **#995** (`sera-pzjk`) — SPEC-plugins amendment: dual transport (stdio + gRPC), `ContextEngine` capability, three first-class SDKs.
- **#996** (`sera-pzjk` fold-in) — Q7–Q10 design-pass resolutions folded into the spec in a follow-up commit.
- **#997** (`sera-shk8`) — top-level `CLAUDE.md` codebase map refreshed (`core/`, `web/`, `tui/`, `cli/`, `tools/discord-bridge/` all moved under `legacy/`).
- **#998** — `scripts/sera-local` dev boot script committed (was uncommitted from the prior handoff).
- **#999** (`sera-1bg4`) — `sera-plugins` Rust crate caught up to the post-amendment spec: `ContextEngine` variant + discriminated transport manifest (`grpc` | `stdio`) + subprocess lifecycle (`SIGTERM` → 5s → `SIGKILL`, exponential-backoff restart) reusing existing `CircuitBreaker`.

---

## Architectural decisions baked this session (DO NOT re-litigate — spec is signed off, Rust is wired)

- **Dual-transport plugin model.** stdio + gRPC are BOTH first-class; transport choice is an ops/deployment concern, not a code/spec gate. No dev-vs-prod split. Operators pick whichever fits their ops posture.
- **Stdio authentication = binary pinning** (absolute `command[0]`, non-world-writable binary dir, optional SHA-256 digest as future hardening). Socket perms are NOT the stdio analog of mTLS; binary identity + OS process isolation are.
- **Stdio wire = stdin/stdout JSON-RPC** aligned with SPEC-hooks §2.6 subprocess pattern. NO separate control_socket — heartbeats multiplex over the same stream.
- **`ContextEngine` added to `PluginCapability` enum.** First out-of-process consumer is the forthcoming Python LCM plugin (`sera-yf9r`).
- **Proto stays canonical; JSON Schema mirrors ship adjacent.** CI check enforces drift (not reviewer-catch).
- **Independent SDK release cadence** across crates.io / PyPI / npm. Protocol version is the coordination point, not the SDK versions.
- **No per-capability transport preference.** Capabilities are transport-agnostic by design.
- **SDK package location:** `sdk/python/sera-plugin-sdk/` + `sdk/typescript/sera-plugin-sdk/`.
- **Code vs ops/deployment is a hard line** (durable principle, saved to memory). Don't bake dev/prod or tier gates into protocols/manifests/code unless semantically load-bearing.

---

## Primary goal (pick one — ask if unsure)

### 1. Continue extension-model work

Fire `sera-psql` (sdk-py) + `sera-kfic` (sdk-ts) in parallel. Green-field Python + TS packages. SDK sketch (six defaults) from this session:

- Python `pyproject.toml` + `hatch` backend, Python 3.11+, ABC-inheritance (`class LcmPlugin(ContextEngine, ContextQuery, …)` — not `@decorator`).
- TS `package.json` ESM + `tsup` + Node 20+, same abstract-class shape.
- Build backend: `hatch` (py), `tsup` (ts). Proto consumption via `protoc` at build time.
- Async model: async/await everywhere, no sync wrappers.
- Single optional `on_startup` / `on_shutdown` lifecycle hooks; `PluginError(code, message, details)` base class.
- New CI jobs `validate-sdk-py` + `validate-sdk-ts`, triggered on `sdk/**` or `rust/proto/plugin/**` changes.

After `sera-psql` merges, `sera-yf9r` (LCM plugin) becomes next — **but see CRITICAL INSIGHT below, it's heavier than it looks.**

### 2. `sera-r9ed` — two-layer session persistence

PartTable + shadow git per SPEC-gateway §6.1b. Architectural lift. Unblocks `sera-r1g8` → full M2 Phase 0 coverage. **STOP AND TALK** before fan-out on persistence model.

### 3. `sera-r1g8` — Submission envelope route wrapping

Wrap every agent-facing route in a Submission envelope emitter. Depends on `sera-r9ed` landing first.

### 4. `sera-s4b1` — sera-hooks WIT interface for third-party WASM hooks

Design-heavy. Needs (a) WIT file so third parties compile via `wit-bindgen`, (b) sandboxed capability injection, (c) HookChain YAML manifest schema. **STOP AND TALK** before fan-out on WIT surface.

---

## Follow-up pool

- `sera-4yz5` — OSS launch polish (open since prior sessions).
- Pre-existing clippy warnings in `sera-testing/src/contracts.rs` (`LifecycleMode clone_on_copy` × 2) — not yours, file a bead if it blocks a future PR.
- Other `CLAUDE.md` staleness: Docker Compose section references pre-migration `core/docker-entrypoint.dev.sh`; `bun install` learning references `core/` and `web/` as workspace packages; `D:/projects/homelab/sera` working-dir path. Intentionally NOT fixed in #997 (surgical). File a bead if they bite.

---

## CRITICAL INSIGHT for `sera-yf9r` (full detail in bead notes)

The hermes-agent Python `ContextEngine` base class (`~/.hermes/hermes-agent/agent/context_engine.py:32`) has a **fundamentally different shape** from our Rust `ContextEngine` trait:

- **Hermes:** `compress(messages, tokens) → messages` — one-shot, stateless-looking.
- **Ours:** `ingest` / `assemble` / `compact` / `maintain` — per-session stateful.

SPEC-context-engine-pluggability §4's mapping table was designed around LCM's **internal** methods (`engine.build_prompt`, `engine.maybe_compact`, `store.append`, etc.) — **NOT hermes's public `ContextEngine` interface**. The LCM plugin therefore cannot subclass hermes's `ContextEngine`; it must reach past the public interface and bind directly to LCM internals. **That's an adapter, not a wrap.** Expect 1–2 sessions of adapter work, not a few hours. This is heavier than `sera-fnaj` (hindsight) because hindsight's wire contract matched our memory trait; LCM's does not. Our `sdk-py` `ContextEngine` ABC stays mirrored to our Rust trait exactly — per-plugin adapter work handles shape translation.

---

## Workflow (unchanged)

- Start with `bd ready` + `bd show <id>`. Pre-drafted child descriptions for `sera-psql` / `sera-kfic` / `sera-yf9r` are in parent `sera-xx48`'s notes field — read those before firing executors.
- Worktrees per lane at `/home/entity/projects/sera-wt/<lane>`, off `origin/main`.
- Working Principles (CLAUDE.md): **Think Before Coding / Simplicity First / Surgical Changes / Goal-Driven Execution**.
- Ask clarifying questions before fan-out on design-heavy beads (`sera-r9ed` persistence, `sera-s4b1` WIT surface, `sera-yf9r` LCM adapter).
- If you spot adjacent work, file a bead — do not sprawl.

---

## Executor ops notes (refreshed this session)

- **Main branch is protected** — direct push rejected. ALWAYS use a feature branch + PR. The session-close protocol's "git push" means push your feature branches, not push to main.
- **Race conditions on parallel pushes:** if the user merges your PR while you're pushing a follow-up commit, the commit orphans. Recovery: cherry-pick onto a fresh branch off post-merge main, open a second PR. Happened this session with #996.
- **First CI run on a new PR can be flaky** due to cached builds. If `validate-rust` fails at a line number that doesn't match current code, it's a cache miss — `gh run rerun <run-id> --failed` usually fixes it. Reproduce locally with `cd rust && cargo test --workspace --no-run` before assuming the code is wrong.
- **Avoid `cargo fmt --all`** — workspace main is fmt-dirty, silently reformats ~250 files. Always `cargo fmt -p <crate>`. If that over-reaches, stash + restore + pick pattern.
- **`#[tokio::test]` + blocking std API = deadlock.** Any integration test using `std::os::unix::net::UnixStream` / `std::process::Command` inside an async tokio body MUST annotate `#[tokio::test(flavor = "multi_thread", worker_threads = 2)]` — otherwise the blocking client starves the runtime and CI hangs 28+ min.
- **`gh pr merge --auto` is NOT enabled** on this repo — poll + merge manually. `ci-e2e-smoke` runs AFTER `validate-rust` (total ~8–10 min).
- **`gh pr update-branch <n>`** forces rebase on main and re-triggers CI — use when a PR falls behind.
- **Sonnet for mechanical plumbing** (5–15 min); **Opus for design review** / spec amendments. Rust catch-up this session was Sonnet; SPEC amendment was Opus.
- **`.omc/state/` is cwd-relative** — `cd` back out of `rust/` between state-touching commands.
- **Stop hook sometimes fabricates phantom errors** (`Read operation failed` / `libc` / etc.) when no such tool call was made. Ignore and continue — do NOT retry something you didn't run.

---

## Validation recipe (confirm E2E still works)

```bash
scripts/sera-local    # boots gateway+runtime against LM Studio, tier=local
# in another shell:
curl -s http://localhost:42540/api/chat \
  -H 'Content-Type: application/json' \
  -d '{"agent":"sera","message":"hi","stream":false}'
```

Expect a real response from `gemma-4-e2b` (LM Studio at `:1234` must be running with that model loaded).

---

## Open beads under parent `sera-xx48` (the extension-model initiative)

- `sera-psql` — `sera-plugin-sdk-py` (blocked on SDK sketch signoff, else ready to fire).
- `sera-kfic` — `sera-plugin-sdk-ts` (parallel to `sera-psql`).
- `sera-yf9r` — `sera-context-lcm` (blocked on `sera-psql`; see CRITICAL INSIGHT above).

---

## Design decisions from prior session (still baked)

- Constitutional gate permissive mode: `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE` env OR `Instance.spec.tier: local`. Gateway forwards env to spawned runtimes when `tier=local`.
- Admin kill-switch socket path cascade: `/var/lib/sera/admin.sock` → `$XDG_RUNTIME_DIR/sera-admin.sock` → `${TMPDIR:-/tmp}/sera-admin-$USER.sock`.
- `sera-memory` crate stubs in `sera-db` / `sera-testing` are gone — direct imports via `sera_memory::`.
- `SERA_E2E_MODEL` env var controls harness manifest's model field (unset → `e2e-mock` wiremock path, set → real LLM).

---

*Sera's home grew three more rooms today — SPEC-plugins dual-transport, the Rust wiring to match, and a refreshed codebase map that tells you which rooms exist.*
