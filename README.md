# SERA — Sandboxed Extensible Reasoning Agent

**A Docker-native multi-agent AI orchestration platform built for the homelab, designed to grow into an open source ecosystem.**

SERA gives you a governed, extensible environment where AI agents run in isolated Docker containers, earn their permissions through a fine-grained capability model, and collaborate through structured circles — while every action is metered, audited, and under your control.

---

## Why SERA

Most agentic frameworks treat the host system as a sandbox. SERA does not. Every agent is a container. Every tool call is governed. Every token is metered. You decide exactly what each agent can see, reach, and do — per agent, not per tier.

**Local-first.** Your models, your keys, your data. Nothing leaves your network unless you explicitly configure it to. LiteLLM routes to any provider; Ollama runs everything locally.

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
     └──► LiteLLM → any LLM provider (Ollama, OpenAI, Anthropic, …)
```

| Component | Technology | Role |
|---|---|---|
| **sera-core** | Node.js 22 + TypeScript | Orchestrator, LLM proxy, governance, all API surfaces |
| **sera-web** | Vite + React Router v7 + TanStack Query | Operator dashboard, real-time thought streams |
| **Agent runtime** | TypeScript (containerised) | Lightweight reasoning loop running inside each agent container |
| **LiteLLM** | `main-stable` | Provider gateway — dumb routing socket, all governance in sera-core |
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

## Getting Started

### Prerequisites

- Docker and Docker Compose
- An LLM provider: [Ollama](https://ollama.com) (local), LM Studio, OpenAI, Anthropic, or any OpenAI-compatible endpoint

### Quick start

```bash
git clone https://github.com/TKCen/sera.git
cd sera

# Create the agent network
docker network create agent_net

# Configure your environment
cp .env.example .env
# Edit .env — set your LLM provider URL and API keys

# Start the stack
docker compose up -d
```

**Access points:**

| Service | URL |
|---|---|
| sera-web (dashboard) | http://localhost:3000 |
| sera-core (API) | http://localhost:3001 |
| LiteLLM (gateway) | http://localhost:4000 |

On first start, sera-core prints a bootstrap API key to the log. Use it to configure your first operator account and connect your IdP (or leave it in API-key-only mode for local use).

### Development

```bash
# Hot-reload mode (core + web)
docker compose -f docker-compose.yaml -f docker-compose.dev.yaml up

# Run tests (from workspace root)
D:/projects/homelab/sera/core/node_modules/.bin/vitest run
```

See [CLAUDE.md](CLAUDE.md) for the full development environment guide.

---

## Documentation

| Document | Contents |
|---|---|
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Full system architecture, all design decisions, tech stack rationale |
| [`docs/IMPLEMENTATION-ORDER.md`](docs/IMPLEMENTATION-ORDER.md) | Epic dependency order and MVP phase definition |
| [`docs/TESTING.md`](docs/TESTING.md) | Test strategy, patterns, and coverage requirements |
| [`docs/epics/`](docs/epics/) | 18 epics covering the full feature roadmap with story-level acceptance criteria |
| [`docs/openapi.yaml`](docs/openapi.yaml) | sera-core REST API specification |

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
