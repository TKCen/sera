# SERA

**Sandboxed Extensible Reasoning Agent** — a Docker-native, multi-agent AI runtime written in Rust.

SERA is for teams who want long-lived, governed agents on their own infrastructure: constitutional invariants enforced at runtime (not just prompted), a 6-state session machine, 20 hook points across the turn lifecycle, and a pluggable memory ladder that runs locally against a single SQLite file — and scales to Postgres + pgvector for enterprise deployments. If you want an agent framework that treats safety, audit, and sandboxing as primitives instead of afterthoughts, SERA is built for you.

> The original TypeScript implementation has been archived under `legacy/`. The Rust workspace at `rust/` is the active codebase.

## Architecture at a glance

```
         ┌─────────────────────────────────────────────────────┐
 client ─▶  sera-gateway (axum, HTTP/WS/gRPC)                 │
 channel │                                                     │
 webhook │   [⛓ pre_route] ─▶ lane-aware queue ─▶ [⛓ post_route]
         │                                                     │
         │   session state machine: Created → Active → Idle   │
         │                          → Compacting → Closed      │
         │                                                     │
         │   ┌───────────────────────────────────────────┐    │
         │   │  sera-runtime: turn loop                  │    │
         │   │  [⛓ pre_turn] ─▶ ContextEngine            │    │
         │   │  [⛓ context_{persona,memory,skill,tool}]  │    │
         │   │  [⛓ on_llm_start] ─▶ LLM call             │    │
         │   │  tool_call → gateway dispatch             │    │
         │   │     [⛓ pre_tool] ─▶ CapabilityPolicy      │    │
         │   │     ─▶ SandboxProvider (OCI / WASM / …)   │    │
         │   │     [⛓ post_tool] ─▶ result               │    │
         │   │  [⛓ on_llm_end] ─▶ [⛓ post_turn]          │    │
         │   │  [⛓ constitutional_gate]                  │    │
         │   └───────────────────────────────────────────┘    │
         │                                                     │
         │   memory ladder: MemoryBlock (in-process)           │
         │                 ├─ SQLite FTS5 + sqlite-vec (RRF)   │
         │                 ├─ pgvector (enterprise)            │
         │                 └─ SemanticMemoryStore plugin       │
         │                                                     │
         │   workflow gates (sera-workflow):                   │
         │     Human · Change · GhRun · GhPr · Mail · Timer   │
         │                                                     │
         │   sera-meta: 3-tier evolution policy                │
         │     (AgentImprovement → ConfigEvolution →           │
         │      CodeEvolution) + constitutional registry       │
         └─────────────────────────────────────────────────────┘
                               │
                               ▼
            LLM provider (OpenAI-compatible, Anthropic, …)
```

All durable state lives gateway-side. Runtimes are ephemeral — a worker crash loses nothing.

## What's different?

| Axis | LangChain / LangGraph | AutoGen | CrewAI | OpenAgents / BYO-agent | **SERA** |
| --- | --- | --- | --- | --- | --- |
| Deployment model | Python library embedded in app | Python library | Python library | Orchestration-only (BYO runtime) | **Rust runtime + gateway; one SQLite file locally, Docker-native for multi-tenant** |
| Memory | DIY per chain | Chat history | Vector DB of choice | Delegated | **Tiered: MemoryBlock → SQLite FTS5+sqlite-vec (RRF) → pgvector → user plugin** |
| Safety gates | Prompt-level guardrails | Prompt-level | Prompt-level | Delegated to runtime | **20 runtime hook points + constitutional registry + 3-tier evolution policy** |
| State machine | Graph (LangGraph); ad-hoc elsewhere | Conversation | Implicit | Agent-defined | **Explicit 6-state `SessionStateMachine` with validated transitions** |
| Language | Python | Python | Python | Varies | **Rust (edition 2024), 34-crate workspace, `cargo clippy -D warnings` across the tree** |

SERA is not a Python-native framework. If most of your codebase is Python and you want to compose nodes in a notebook, LangGraph or CrewAI is the right answer. If you need a runtime you can audit end-to-end, sandbox per tier, and deploy without a vector DB or message broker, read on.

## Features

- **Constitutional runtime.** Invariants are registered with `sera-meta::constitutional::ConstitutionalRegistry` and enforced at runtime hook points (e.g. `constitutional_gate`) — not delegated to prompt text the model can ignore.
- **20 hook points across the turn lifecycle.** `pre_route`, `post_route`, `pre_turn`, `context_{persona,memory,skill,tool}`, `on_llm_start`, `pre_tool`, `post_tool`, `on_llm_end`, `post_turn`, `constitutional_gate`, `pre_deliver`, `post_deliver`, `pre_memory_write`, `on_session_transition`, `on_approval_request`, `on_workflow_trigger`, `on_change_artifact_proposed`. YAML-configured chains with fail-open / fail-closed behaviour.
- **Explicit session state machine.** A 6-state FSM (`Created`, `Active`, `Idle`, `Suspended`, `Compacting`, `Closed`) with validated transitions — no implicit lifecycle.
- **Pluggable memory ladder.** Tier-0 in-process `MemoryBlock`; Tier-1 local uses SQLite FTS5 + sqlite-vec with RRF hybrid search; enterprise swaps in pgvector; or bring your own `SemanticMemoryStore` (mem0, hindsight, external RAG).
- **Docker-native per-tier sandboxing.** Tools run through a `SandboxProvider` trait — OCI containers with tier-1/2/3 capability policies (different egress, filesystem, and compute per tier). `MockSandboxProvider` for tests; bollard-backed `sera-oci` in production.
- **Six AwaitType workflow gates.** `Human`, `Change`, `GhRun`, `GhPr`, `Mail`, `Timer` — suspend a session on a real-world event and resume when it fires.
- **3-tier evolution policy.** Self-modification is scoped to `AgentImprovement` (prompts, persona), `ConfigEvolution` (runtime config), or `CodeEvolution` (source-level). Each tier has its own approver requirements.
- **Local-first by default.** One binary, one SQLite file, no Postgres, no Centrifugo, no Redis. Add them when you need multi-pod scale.
- **Rust workspace, strict boundaries.** 34 crates under `rust/crates/`, `sera-types` is the sole leaf, `sera-tui` is forbidden from importing gateway internals. `cargo test --workspace` exercises 3000+ tests.

## Quickstart — local profile (zero external services)

Prerequisites: Rust **1.94+** (edition 2024) and any OpenAI-compatible LLM endpoint (LM Studio, Ollama, vLLM, OpenAI, Anthropic…).

```bash
# 1. Build the workspace
cd rust
cargo build --release --bin sera

# 2. Point at an LLM
export SERA_LLM_BASE_URL=http://localhost:1234/v1   # LM Studio default
export SERA_LLM_API_KEY=not-needed-for-local

# 3. Start the gateway (SQLite on disk, no DATABASE_URL required)
./target/release/sera start
```

All state lands in `./sera.db` and `./logs/`. No Postgres, no Centrifugo, no external embedding service — the default memory tier uses SQLite FTS5 + sqlite-vec + RRF hybrid search, and embeddings can run on CPU via fastembed (`--features local-embedding`).

Health check:

```bash
curl http://localhost:8080/api/health
```

### CLI

```bash
cargo run --bin sera-cli -- auth login
cargo run --bin sera-cli -- agent list
cargo run --bin sera-cli -- chat --agent <agent-id>   # interactive REPL with SSE streaming
```

### TUI

```bash
cargo run --bin sera-tui
```

Agent list, live session view, inline HITL approvals, evolve-proposal status.

> **Demo placeholder.** A terminal-cast of the CLI + TUI flow is planned for the next release. If you record one while evaluating SERA, a PR adding `docs/media/quickstart.gif` is welcome.

## Quickstart — enterprise profile (Postgres + pgvector + Centrifugo)

```bash
docker compose -f docker-compose.rust.yaml up --build
```

Starts Postgres (with pgvector), Centrifugo, and `sera-gateway` on :3001. The gateway auto-detects `DATABASE_URL` and switches from SQLite to Postgres for all stores; pgvector replaces the SQLite hybrid store for Tier-1 semantic memory; Centrifugo enables multi-pod thought streaming.

## Memory tiers

| Tier | Backend | When |
| ---- | ------- | ---- |
| Tier 0 | `MemoryBlock` (in-process) | Always on |
| Tier 1 basic | `SqliteFtsMemoryStore` (FTS5 + sqlite-vec + RRF) | Default local profile |
| Tier 1 enterprise | `PgVectorStore` | `DATABASE_URL` set + pgvector extension |
| Tier 1 plugin | User impl of `SemanticMemoryStore` (mem0, hindsight, external RAG…) | Compile-time feature select |
| Tier 2 | `ContextEnricher` auto-promotes hits into MemoryBlock | Always on when Tier 1 is wired |

See `docs/plugins/memory.md` for the plugin contract.

## Workspace layout

34 crates under `rust/crates/`. Notable:

| Crate | Purpose |
| ----- | ------- |
| `sera-gateway` | Main axum API server (`sera` binary) |
| `sera-runtime` | Agent worker: context engine, tool registry, LLM client |
| `sera-cli` | `sera` operator CLI (auth, agent, chat) |
| `sera-tui` | Ratatui operator dashboard |
| `sera-db` | SQLite + Postgres stores, memory, queue, auth, evolution proposals |
| `sera-session` | 6-state `SessionStateMachine`, transcript, persistence |
| `sera-hooks` | In-process `Hook` registry + chain executor, YAML manifests |
| `sera-workflow` | Six AwaitType gates (Human / Change / GhRun / GhPr / Mail / Timer) |
| `sera-meta` | Self-evolution: 3-tier policy, shadow sessions, constitutional registry |
| `sera-memory` | Memory tier ladder + `SemanticMemoryStore` trait |
| `sera-mail` | RFC 5322 mail correlator for the Mail gate |
| `sera-models` | Provider-agnostic account pool + thinking/reasoning config |
| `sera-skills` | Skill loader (YAML, TOML, SKILL.md) |
| `sera-oci` | OCI sandbox provider (bollard) |
| `sera-plugins` | gRPC plugin registry, SDK, circuit breaker |
| `sera-e2e-harness` | Cross-crate integration test runner |

See `rust/CLAUDE.md` for the full crate map and development workflow.

## Development

```bash
cd rust
cargo check --workspace           # fast incremental validation
cargo test --workspace            # 3000+ tests
cargo clippy --workspace -- -D warnings
```

## Docs

- **Why SERA?** `docs/WHY-SERA.md` — architectural commitments, honest tradeoffs, and a framework comparison.
- **Architecture:** `docs/plan/ARCHITECTURE-2.0.md`
- **Implementation status:** `docs/plan/HANDOFF.md` (session-by-session close-out)
- **Plugin contracts:** `docs/plugins/`
- **Competitive landscape:** `docs/competitive-analysis.md`
- **CLAUDE.md files:** per-directory agent-facing guides (root, `rust/`, `rust/crates/sera-runtime/`)

## Issue tracker

This project uses **beads (`bd`)** for task tracking.

```bash
bd ready              # work with no active blockers
bd show <id>          # view details
bd update <id> --claim
bd close <id> --reason FIXED
bd prime              # full workflow reference
```

Do NOT use TodoWrite, TaskCreate, or markdown TODO lists for project work.

## Contributing

See `CONTRIBUTING.md` for the development loop, PR expectations, and beads workflow. `CODE_OF_CONDUCT.md` applies to all contributors — human and automated.

## License

MIT. See `LICENSE`.

## Feedback and help

- File an issue: <https://github.com/TKCen/sera/issues>
- Open a GitHub discussion or file a bead with `type=question`.
