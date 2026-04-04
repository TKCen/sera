# SERA — Sandboxed Extensible Reasoning Agent

**A Docker-native multi-agent AI orchestration platform built for the homelab, designed to grow into an open source ecosystem.**

SERA gives you a governed, extensible environment where AI agents run in isolated Docker containers, earn their permissions through a fine-grained capability model, and collaborate through structured circles — while every action is metered, audited, and under your control.

---

## Why SERA

Most agentic frameworks treat the host system as a sandbox. SERA does not. Every agent is a container. Every tool call is governed. Every token is metered. You decide exactly what each agent can see, reach, and do — per agent, not per tier.

**Local-first.** Your models, your keys, your data. Nothing leaves your network unless you explicitly configure it to. LLM routing happens in-process; Ollama and LM Studio run everything locally.

**Governance as a first-class concern.** A three-layer permission model (SandboxBoundary x CapabilityPolicy x ManifestInline) gives enterprise-grade access control without enterprise complexity. Shared reference lists, deny-always semantics, and human-in-the-loop permission grants for runtime escalation.

**Built to be extended.** Skills are text documents injected into agent context. MCP servers are containerised tools that agents discover and use. BYOH (Bring Your Own Host) agents can run outside Docker entirely. Neither requires touching core. A community can publish agent templates, skill packs, and MCP tools independently.

**Observable.** Agents stream their thoughts (observe, plan, act, reflect) in real time via Centrifugo. Every action is recorded in a Merkle hash-chain audit trail. Token usage, budgets, and circuit breaker state are all exposed.

---

## Architecture

```
                          ┌─────────────────────────────────────────────┐
                          │              sera-web (UI)                  │
                          │   Vite + React Router — operator dashboard  │
                          └──────────────────┬──────────────────────────┘
                                             │ REST + WebSocket
                          ┌──────────────────▼──────────────────────────┐
                          │             sera-core (Mind)                 │
                          │  Orchestrator · LLM Proxy · Capability Engine│
                          │  Scheduler · Skill Registry · MCP Registry  │
                          │  Memory · Audit · Secrets · Channels        │
                          └──┬────────┬──────────┬──────────┬───────────┘
                             │        │          │          │
                   Docker API│  Centrifugo API   │ SQL      │ Qdrant API
                          ┌──▼──────┐ ┌──▼──────┐ ┌──▼─────▼──────────┐
                          │  Agent  │ │Centrifugo│ │ PostgreSQL+pgvector│
                          │Containers│ │ (Pulse) │ │ Qdrant (vectors)  │
                          └──┬──────┘ └─────────┘ └────────────────────┘
                             │
              ┌──────────────▼──────────────┐
              │    sera-egress-proxy         │
              │  Squid — SNI-based filtering │
              │  per-agent ACLs · metering   │
              └─────────────────────────────┘
```

| Component         | Technology                              | Role                                                                      |
| ----------------- | --------------------------------------- | ------------------------------------------------------------------------- |
| **sera-core**     | Node.js 22 + TypeScript                 | Orchestrator, LLM proxy, governance, all API surfaces                     |
| **sera-web**      | Vite + React Router v7 + TanStack Query | Operator dashboard, real-time thought streams                             |
| **Agent runtime** | TypeScript (bun, containerised)         | Lightweight reasoning loop running inside each agent container            |
| **LlmRouter**     | In-process (`@mariozechner/pi-ai`)      | Provider gateway — cloud and local providers, no sidecar needed           |
| **Centrifugo**    | Latest stable                           | Real-time pub/sub for thoughts, tokens, agent status                      |
| **PostgreSQL**    | 15 + pgvector                           | Primary store — agents, audit trail, secrets, tasks, metering, embeddings |
| **Qdrant**        | Latest                                  | Vector search for agent memory and knowledge retrieval                    |
| **Egress Proxy**  | Squid                                   | SNI-based HTTPS filtering, per-agent ACLs, bandwidth rate limiting        |

### Agent deployment models

SERA supports two ways to run agents:

| Model                                 | How it works                                                             | When to use                                                                       |
| ------------------------------------- | ------------------------------------------------------------------------ | --------------------------------------------------------------------------------- |
| **Container agents** (default)        | Agent runtime runs inside a Docker container managed by SandboxManager   | Full sandbox isolation, capability enforcement, egress filtering                  |
| **BYOH agents** (Bring Your Own Host) | Agent runs on any host and communicates with sera-core via HTTP protocol | Existing infrastructure, GPU hosts, development machines, non-Docker environments |

BYOH agents implement a standardised HTTP protocol (see `schemas/byoh-*.schema.json`). Example implementations are provided in Rust (`rust/`), Python (`examples/byoh-python/`), and shell (`examples/byoh-shell/`).

---

## Key Concepts

**AgentTemplate / Agent** — Templates are reusable blueprints (community-publishable). Agents are named instances with their own configuration, lifecycle, and identity. Instances override templates selectively; the resolution is explicit and auditable.

**Capability model** — Three independent layers intersect to produce the effective permission set: `SandboxBoundary` (hard ceiling — network, filesystem, shell, Docker access), `CapabilityPolicy` (operator-assigned grants, references shared NamedLists), and `ManifestInline` (agent-declared requirements). Deny wins at every layer.

**Persistent vs ephemeral agents** — First-class lifecycle modes. Ephemeral agents cannot create persistent agents. Lineage is tracked. Resource cleanup is automatic.

**Circles** — Named groups of agents with a shared constitution, broadcast channels, and pooled budgets. Agents within a circle collaborate; a circle's shared knowledge base is git-backed with per-agent commit identity.

**Skills vs MCP tools** — Skills are versioned Markdown documents injected into the agent's system prompt. MCP tools are containerised executables the agent calls at runtime. Skills guide; tools act.

**Sera** — The built-in primary agent, auto-instantiated on first boot. She orchestrates the instance, manages other agents, and is the natural entry point for natural-language interaction with the platform itself.

**Human-in-the-loop grants** — Agents can request elevated permissions at runtime (new filesystem path, new shell command, external network access). The operator approves once, for the session, or persistently. Approvals are recorded with full identity context.

---

## Repository Layout

This is a **bun workspace monorepo**. The `core/` and `web/` packages are bun workspaces; the TUI and CLI are standalone Go modules.

```
sera/
  core/                  # sera-core — Node.js API server, orchestrator, governance
  core/agent-runtime/    # Agent worker process (bun, runs inside containers)
  web/                   # sera-web — Vite + React operator dashboard
  tui/                   # Go terminal UI (standalone module)
  cli/                   # Go CLI client with auth support
  rust/                  # Rust BYOH agent implementation
  examples/              # BYOH example agents (Python, shell)
  agents/                # Agent YAML manifests (instances)
  templates/             # AgentTemplate definitions (builtin + custom)
  schemas/               # JSON Schema for manifests, policies, and BYOH protocol
  sandbox-boundaries/    # Tier policy definitions (tier-1/2/3.yaml)
  capability-policies/   # CapabilityPolicy definitions
  circles/               # Circle definitions and shared memory
  lists/                 # Network and command allow/denylists
  skills/                # Skill documents (builtin + examples)
  mcp-servers/           # MCP server manifest definitions
  egress-proxy/          # Squid egress proxy config and Dockerfile
  centrifugo/            # Centrifugo real-time messaging config
  docs/                  # Architecture, epic specs, API reference
  scripts/               # Repo-level tooling and CI helpers
  e2e/                   # End-to-end tests (Playwright)
  tests/                 # Cross-cutting test suites (BYOH compliance)
```

---

## Getting Started

### Prerequisites

- Docker and Docker Compose
- An LLM provider: [Ollama](https://ollama.com) (local), [LM Studio](https://lmstudio.ai) (local), OpenAI, Anthropic, or any OpenAI-compatible endpoint

### Quick start

```bash
git clone https://github.com/TKCen/sera.git
cd sera

# Configure your environment
cp .env.example .env
# Edit .env — set your LLM provider URL and API keys

# Start the stack (production)
bun run prod:up

# Or start in development mode (hot-reload)
bun run dev:up
```

On first start, sera-core runs database migrations automatically, bootstraps the Sera agent, and prints the bootstrap API key to the log. Use it to authenticate with the dashboard.

**Access points:**

| Service              | Dev mode               | Production             |
| -------------------- | ---------------------- | ---------------------- |
| sera-web (dashboard) | http://localhost:3000  | http://localhost:3000  |
| sera-core (API)      | http://localhost:3001  | http://localhost:3001  |
| Centrifugo           | http://localhost:10001 | http://localhost:10001 |

> **Note:** All `/api/*` endpoints (except `/api/health/*`) require `Authorization: Bearer <key>`. The bootstrap API key is printed to the sera-core log on first start.

### Development on Windows

On Windows hosts, use `npm install` (not `bun install`) for local development — Bun 1.3.x creates junction points that Node.js and tsc cannot traverse. Docker containers use `bun install` via the entrypoint scripts.

---

## Developer Commands

All commands run from the repository root via `bun run <script>`.

### Docker Compose

| Command                  | Description                                                                   |
| ------------------------ | ----------------------------------------------------------------------------- |
| `bun run dev:up`         | Start the full stack in **hot-reload dev mode** (core + web with live reload) |
| `bun run dev:down`       | Stop the dev stack                                                            |
| `bun run dev:logs`       | Tail dev stack logs                                                           |
| `bun run prod:up`        | Start the production stack                                                    |
| `bun run prod:down`      | Stop the production stack                                                     |
| `bun run prod:logs`      | Tail production logs                                                          |
| `bun run prod:auth:up`   | Start production stack **with Authentik** SSO                                 |
| `bun run prod:auth:down` | Stop the Authentik stack                                                      |
| `bun run prod:auth:logs` | Tail Authentik stack logs                                                     |

### Code Quality

| Command                  | Scope | Description                                                            |
| ------------------------ | ----- | ---------------------------------------------------------------------- |
| `bun run ci`             | all   | Full CI pipeline: format check + lint + typecheck + unit tests + build |
| `bun run check-all`      | all   | Full local check: format + lint + typecheck + all tests + build        |
| `bun run typecheck`      | all   | TypeScript type-check all workspaces                                   |
| `bun run typecheck:core` | core  |                                                                        |
| `bun run typecheck:web`  | web   |                                                                        |
| `bun run lint`           | all   | ESLint all workspaces                                                  |
| `bun run lint:core`      | core  |                                                                        |
| `bun run lint:web`       | web   |                                                                        |
| `bun run format`         | all   | Prettier write all workspaces                                          |
| `bun run format:check`   | all   | Prettier check (no writes — use in CI)                                 |

### Builds

| Command              | Scope | Description                    |
| -------------------- | ----- | ------------------------------ |
| `bun run build`      | all   | Build all workspaces + TUI     |
| `bun run build:core` | core  | `tsup` (esbuild-based, ~100ms) |
| `bun run build:web`  | web   | `tsc -b && vite build`         |
| `bun run build:tui`  | tui   | `go build`                     |

### Tests

| Command                    | Scope | Description                                   |
| -------------------------- | ----- | --------------------------------------------- |
| `bun run test`             | all   | Run all tests across workspaces               |
| `bun run test:unit`        | all   | Unit tests only (no DB / Docker required)     |
| `bun run test:integration` | all   | Integration tests (requires running services) |
| `bun run test:core`        | core  | All core tests                                |
| `bun run test:web`         | web   | All web tests                                 |
| `bun run test:tui`         | tui   | Go tests                                      |

### Pre-commit

```bash
# Install the git hook (one-time after cloning)
bun run hooks:install

# Run the pre-commit checks manually
bun run pre-commit   # typecheck + lint + web tests
```

The pre-commit check runs: `typecheck -> lint -> test:web`. For a full end-to-end verification run `bun run check-all` (format + lint + typecheck + test + build).

---

## Current Status

SERA is in active development. Here is the implementation status of major subsystems:

| Subsystem                               | Status          | Notes                                                                     |
| --------------------------------------- | --------------- | ------------------------------------------------------------------------- |
| Infrastructure (DB, Docker, networking) | **Implemented** | PostgreSQL + pgvector, Qdrant, Docker lifecycle, agent_net                |
| Agent manifest & registry               | **Implemented** | YAML templates, DB-backed instances, CRUD API                             |
| Docker sandbox & lifecycle              | **Implemented** | SandboxManager, tier policies, bind mounts, worktree isolation            |
| LLM proxy & routing                     | **Implemented** | In-process routing via pi-mono, multi-provider, budget enforcement        |
| Agent runtime                           | **Implemented** | Reasoning loop, context management, tool execution, thought streaming     |
| Capability resolution                   | **Implemented** | Three-layer model, NamedList refs, resolver with tests                    |
| Skill library                           | **Implemented** | SkillLibrary, SkillInjector, hot-reload, builtin skills                   |
| MCP tool registry                       | **Implemented** | MCPServerManager, SeraMCPServer, stdio/HTTP transports                    |
| Memory & RAG                            | **Implemented** | Block store, Qdrant indexing, knowledge-store/query tools, embeddings     |
| Real-time messaging                     | **Implemented** | Centrifugo integration, thought/token streaming, agent status             |
| Authentication                          | **Implemented** | API key + OIDC (Authentik), session management, RBAC                      |
| Scheduling & audit                      | **Implemented** | pg-boss scheduler, Merkle hash-chain audit, cron + one-shot               |
| Channels (Discord)                      | **Implemented** | DiscordChatAdapter with session management, typing indicators             |
| Egress proxy                            | **Implemented** | Squid with SNI filtering, per-agent ACLs, audit integration               |
| sera-web dashboard                      | **Implemented** | Agent management, chat, thoughts, memory, audit, providers, MCP, settings |
| BYOH agents                             | **Implemented** | HTTP protocol, Rust/Python/shell examples, compliance tests               |
| CLI                                     | **In progress** | Go CLI with auth support                                                  |
| TUI                                     | **In progress** | Go terminal UI                                                            |
| Circles & coordination                  | **Partial**     | Circle definitions exist, coordination patterns not yet wired             |
| Plugin SDK                              | **Not started** | Planned for Phase 2                                                       |
| Agent delegation (Epic 17)              | **Partial**     | ActingContext, delegation routes exist; full chain not yet wired          |
| Voice interface                         | **Not started** | Planned for Phase 3                                                       |
| A2A Federation                          | **Not started** | Planned for Phase 3                                                       |

---

## Documentation

| Document                                                       | Contents                                                                        |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)                 | Full system architecture, all design decisions, tech stack rationale            |
| [`docs/IMPLEMENTATION-ORDER.md`](docs/IMPLEMENTATION-ORDER.md) | Epic dependency order, phase definitions, and v1 prototype tracking             |
| [`docs/TESTING.md`](docs/TESTING.md)                           | Test strategy, patterns, and coverage requirements                              |
| [`docs/AGENT-WORKFLOW.md`](docs/AGENT-WORKFLOW.md)             | Multi-agent development workflow — agent roles, assignment, validation          |
| [`docs/epics/`](docs/epics/)                                   | 29 epics covering the full feature roadmap with story-level acceptance criteria |
| [`docs/openapi.yaml`](docs/openapi.yaml)                       | sera-core REST API specification                                                |
| [`CLAUDE.md`](CLAUDE.md)                                       | AI assistant development guide — environment, conventions, learnings            |
| [`AGENTS.md`](AGENTS.md)                                       | Cross-agent instructions for Jules, Gemini, Antigravity, and Codex              |
| [`SECURITY.md`](SECURITY.md)                                   | Security policy, secrets management, vulnerability reporting                    |

---

## Roadmap

The backlog is organised as 29 epics across five phases:

**Phase 0 — v1 Prototype** (current focus)
A single agent (Sera) you can talk to via web UI and Discord, who reasons with tools and remembers across sessions.

**Phase 1 — Usable**
Skills, MCP tools, enhanced memory with hybrid search, scheduling, egress proxy, full operator dashboard, interactive setup wizard.

**Phase 2 — Ecosystem**
Multi-operator auth (OIDC, RBAC), full delegation model, external channels (Slack, email, webhooks), plugin SDK for community contributions.

**Phase 3 — Consolidation**
Legacy memory retirement, ACP/IDE bridge, A2A federation protocol, voice interface.

**Phase 4 — Agent-Driven UI**
Canvas (A2UI) — agents push dynamic, interactive UI to the dashboard.

**Phase 5 — Multimodal**
Media processing, image generation, extended channel adapters (Telegram, WhatsApp, Signal, Matrix), enhanced web intelligence.

See [`docs/IMPLEMENTATION-ORDER.md`](docs/IMPLEMENTATION-ORDER.md) for the full sequencing and dependency graph.

---

## Contributing

SERA is built to become a thriving open source ecosystem. If you want to contribute an agent template, a skill pack, or an MCP tool server, the plugin SDK (Epic 15) defines the community contract — including the SERA MCP Extension Protocol for credential-aware tool servers.

Contribution guidelines, the community SDK (`@sera/mcp-sdk`), and the template registry format will be published as Phase 2 lands.

### For AI agents working on SERA

Read [`AGENTS.md`](AGENTS.md) for universal rules, then the workspace-specific instruction file (`core/CLAUDE.md`, `web/CLAUDE.md`, etc.) for the area you're working in. See [`docs/AGENT-WORKFLOW.md`](docs/AGENT-WORKFLOW.md) for the full coordination protocol.

---

## Philosophy

> Agents should be tenants, not residents. They earn access, operate within boundaries, and leave an auditable trail. The human stays in control — not by limiting what agents can do, but by making everything they do legible.

---

_SERA — your agents, your network, your rules._
