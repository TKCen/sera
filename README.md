# SERA — Sandboxed Extensible Reasoning Agent

**A Docker-native multi-agent AI orchestration platform built for the homelab, designed to grow into an open source ecosystem.**

SERA gives you a governed, extensible environment where AI agents run in isolated Docker containers, earn their permissions through a fine-grained capability model, and collaborate through structured circles — while every action is metered, audited, and under your control.

---

## Why SERA

Most agentic frameworks treat the host system as a sandbox. SERA does not. Every agent is a container. Every tool call is governed. Every token is metered. You decide exactly what each agent can see, reach, and do — per agent, not per tier.

**Local-first.** Your models, your keys, your data. Nothing leaves your network unless you explicitly configure it to. LLM routing happens in-process; Ollama runs everything locally.

**Governance as a first-class concern.** A three-layer permission model (SandboxBoundary × CapabilityPolicy × ManifestInline) gives enterprise-grade access control without enterprise complexity. Shared reference lists, deny-always semantics, and human-in-the-loop permission grants for runtime escalation.

**Built to be extended.** Skills are text documents injected into agent context. MCP servers are containerised tools that agents discover and use. Neither requires touching core. A community can publish agent templates, skill packs, and MCP tools independently.

**Observable.** Agents stream their thoughts (observe → plan → act → reflect) in real time via Centrifugo. Every action is recorded in a Merkle hash-chain audit trail. Token usage, budgets, and circuit breaker state are all exposed.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      sera-web (UI)                          │
│          Vite + React Router — operator dashboard           │
└───────────────────────────┬─────────────────────────────────┘
                            │ REST + WebSocket
┌───────────────────────────▼─────────────────────────────────┐
│                     sera-core (Mind)                        │
│  Orchestrator · LLM Proxy · Capability Engine · Scheduler   │
│  Skill Registry · MCP Registry · Memory · Audit · Secrets   │
└────┬──────────────┬────────────────┬────────────────────────┘
     │ Docker API   │ Centrifugo API  │ PostgreSQL / Qdrant
┌────▼────────┐  ┌──▼──────────┐  ┌──▼──────────────────────┐
│    Agent    │  │  Centrifugo │  │  PostgreSQL + Qdrant     │
│ Containers  │  │  (Pulse)    │  │  Tasks · Audit · Memory  │
│ (sandboxed) │  └─────────────┘  └─────────────────────────┘
└────┬────────┘
     │  MCP tool containers (per-agent, sandboxed)
     └──► LlmRouter → any LLM provider (Ollama, OpenAI, Anthropic, …)
```

| Component | Technology | Role |
|---|---|---|
| **sera-core** | Node.js 22 + TypeScript | Orchestrator, LLM proxy, governance, all API surfaces |
| **sera-web** | Vite + React Router v7 + TanStack Query | Operator dashboard, real-time thought streams |
| **Agent runtime** | TypeScript (containerised) | Lightweight reasoning loop running inside each agent container |
| **LlmRouter** | `@mariozechner/pi-ai` (in-process) | Provider gateway — cloud and local providers, no sidecar needed |
| **Centrifugo** | Latest stable | Real-time pub/sub for thoughts, tokens, agent status |
| **PostgreSQL** | 16 | Primary store — agents, audit trail, secrets, tasks, metering |
| **Qdrant** | Latest | Vector search for agent memory and knowledge retrieval |

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

This is a **bun workspace monorepo**. The `core/` and `web/` packages are bun workspaces; the TUI is a standalone Go module.

```
sera/
  core/               # sera-core — Node.js API server, orchestrator, governance
  web/                # sera-web — Vite + React operator dashboard
  tui/                # Go terminal UI (standalone module)
  agents/             # Agent YAML manifests (instances)
  templates/          # AgentTemplate definitions
  schemas/            # JSON Schema for manifests and policies
  sandbox-boundaries/ # Tier policy definitions (tier-1/2/3.yaml)
  capability-policies/# CapabilityPolicy definitions
  circles/            # Circle definitions and shared memory
  lists/              # Network and command allow/denylists
  docs/               # Architecture, epic specs, API reference
  scripts/            # Repo-level tooling
```

---

## Getting Started

### Prerequisites

- Docker and Docker Compose
- An LLM provider: [Ollama](https://ollama.com) (local), LM Studio, OpenAI, Anthropic, or any OpenAI-compatible endpoint

### Quick start

```bash
git clone https://github.com/TKCen/sera.git
cd sera

# Create the agent network (one-time)
docker network create agent_net

# Configure your environment
cp .env.example .env
# Edit .env — set your LLM provider URL and API keys

# Start the stack (production)
bun run prod:up
```

**Access points:**

| Service | URL |
|---|---|
| sera-web (dashboard) | http://localhost:3000 |
| sera-core (API) | http://localhost:3001 |
| Centrifugo | http://localhost:10001 |

On first start, sera-core prints a bootstrap API key to the log. Use it to configure your first operator account and connect your IdP (or leave it in API-key-only mode for local use).

---

## Developer Commands

All commands run from the repository root via `bun run <script>`.

### Docker Compose

| Command | Description |
|---|---|
| `bun run dev:up` | Start the full stack in **hot-reload dev mode** (core + web with live reload) |
| `bun run dev:down` | Stop the dev stack |
| `bun run dev:logs` | Tail dev stack logs |
| `bun run prod:up` | Start the production stack |
| `bun run prod:down` | Stop the production stack |
| `bun run prod:logs` | Tail production logs |
| `bun run prod:auth:up` | Start production stack **with Authentik** SSO |
| `bun run prod:auth:down` | Stop the Authentik stack |
| `bun run prod:auth:logs` | Tail Authentik stack logs |

### Code Sanity

| Command | Scope | Description |
|---|---|---|
| `bun run typecheck` | all | TypeScript type-check all workspaces |
| `bun run typecheck:core` | core | |
| `bun run typecheck:web` | web | |
| `bun run lint` | all | ESLint all workspaces |
| `bun run lint:core` | core | |
| `bun run lint:web` | web | |
| `bun run format` | all | Prettier write all workspaces |
| `bun run format:core` | core | |
| `bun run format:web` | web | |
| `bun run format:check` | all | Prettier check (no writes — use in CI) |
| `bun run format:check:core` | core | |
| `bun run format:check:web` | web | |

### Builds

| Command | Scope | Description |
|---|---|---|
| `bun run build` | all | Build all workspaces + TUI |
| `bun run build:core` | core | `tsc` |
| `bun run build:web` | web | `tsc -b && vite build` |
| `bun run build:tui` | tui | `go build` |

### Tests

| Command | Scope | Description |
|---|---|---|
| `bun run test` | all | Run all tests across workspaces |
| `bun run test:unit` | all | Unit tests only (no DB / Docker required) |
| `bun run test:integration` | all | Integration tests (requires running services) |
| `bun run test:core` | core | All core tests |
| `bun run test:web` | web | All web tests |
| `bun run test:tui` | tui | Go tests |

### Pre-commit

```bash
# Install the git hook (one-time after cloning)
bun run hooks:install

# Run the pre-commit checks manually
bun run pre-commit   # typecheck + lint + web tests
```

The pre-commit check runs: `typecheck → lint → test:web`. For a full end-to-end verification run `bun run check-all` (format → lint → typecheck → test → build). Integration tests are excluded because they require running services; run `bun run test:integration` separately in CI or against a live stack.

### TUI

```bash
bun run tui:build    # compiles tui/tui.exe
```

---

## Documentation

| Document | Contents |
|---|---|
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Full system architecture, all design decisions, tech stack rationale |
| [`docs/IMPLEMENTATION-ORDER.md`](docs/IMPLEMENTATION-ORDER.md) | Epic dependency order and MVP phase definition |
| [`docs/TESTING.md`](docs/TESTING.md) | Test strategy, patterns, and coverage requirements |
| [`docs/epics/`](docs/epics/) | 18 epics covering the full feature roadmap with story-level acceptance criteria |
| [`docs/openapi.yaml`](docs/openapi.yaml) | sera-core REST API specification |
| [`CLAUDE.md`](CLAUDE.md) | AI assistant development guide — environment, conventions, learnings |

---

## Roadmap

SERA is in active development. The backlog is organised as 18 epics across three phases:

**Phase 1 — Foundation** (MVP: a governed, sandboxed agent you can talk to)
Infrastructure · Manifest & Registry · Docker Sandbox · LLM Proxy · Agent Runtime · Authentication

**Phase 2 — Capability** (makes it genuinely useful)
Skill Library · MCP Tool Registry · Memory & RAG · Real-Time Messaging · Scheduling & Audit · sera-web UX

**Phase 3 — Ecosystem** (makes it extensible and open)
Circles & Coordination · Plugin SDK · Agent Identity & Delegation · Integration Channels (Discord, Slack, webhooks)

See [`docs/IMPLEMENTATION-ORDER.md`](docs/IMPLEMENTATION-ORDER.md) for the full sequencing.

---

## Contributing

SERA is built to become a thriving open source ecosystem. If you want to contribute an agent template, a skill pack, or an MCP tool server, the plugin SDK (Epic 15) defines the community contract — including the SERA MCP Extension Protocol for credential-aware tool servers.

Contribution guidelines, the community SDK (`@sera/mcp-sdk`), and the template registry format will be published as Phase 3 lands.

---

## Philosophy

> Agents should be tenants, not residents. They earn access, operate within boundaries, and leave an auditable trail. The human stays in control — not by limiting what agents can do, but by making everything they do legible.

---

*SERA — your agents, your network, your rules.*
