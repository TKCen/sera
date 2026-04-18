# SERA

**Sandboxed Extensible Reasoning Agent** — a Docker-native multi-agent orchestration platform written in Rust.

SERA runs local, long-lived agents with pluggable memory, human-in-the-loop approvals, safe tool execution, and self-evolution guardrails. A pure-local setup needs nothing beyond a filesystem and an LLM endpoint.

> The original TypeScript implementation has been archived under `legacy/`. The Rust workspace at `rust/` is the active codebase.

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

Agent list, live session view, inline HITL approvals, evolve-proposal status — all configurable keybindings (see `docs/tui-config.md`).

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

32 crates under `rust/crates/`. Notable:

| Crate | Purpose |
| ----- | ------- |
| `sera-gateway` | Main axum API server (`sera` binary) |
| `sera-runtime` | Agent worker: context engine, tool registry, LLM client |
| `sera-cli` | `sera` operator CLI (auth, agent, chat) |
| `sera-tui` | Ratatui operator dashboard |
| `sera-db` | SQLite + Postgres stores, memory, queue, auth, evolution proposals |
| `sera-mail` | RFC 5322 mail correlator for the Mail gate |
| `sera-e2e-harness` | Cross-crate integration test runner |
| `sera-models` | Provider-agnostic account pool + thinking/reasoning config |
| `sera-skills` | Skill loader (YAML, TOML, SKILL.md) |
| `sera-workflow` | Six AwaitType gates (Human / Change / GhRun / GhPr / Mail / Timer) |
| `sera-meta` | Self-evolution: 3-tier policy, shadow sessions, constitutional registry |

See `rust/CLAUDE.md` for the full crate map and development workflow.

## Development

```bash
cd rust
cargo check --workspace           # fast incremental validation
cargo test --workspace            # 3200+ tests
cargo clippy --workspace -- -D warnings
```

## Docs

- **Architecture:** `docs/plan/ARCHITECTURE-2.0.md`
- **Implementation status:** `docs/plan/HANDOFF.md` (session-by-session close-out)
- **Plugin contracts:** `docs/plugins/`
- **CLAUDE.md files:** per-directory agent-facing guides (root, `rust/`, `core/`, `web/`, `tui/`, `cli/`)

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

## Feedback and help

- File an issue: <https://github.com/TKCen/sera/issues>
- `/help` in Claude Code: built-in assistance if you're working in the `claude` CLI
