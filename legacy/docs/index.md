---
hide:
  - navigation
  - toc
---

# SERA — Sandboxed Extensible Reasoning Agent

<div class="grid cards" markdown>

- :material-docker:{ .lg .middle } **Docker-Native Agents**

  ***

  Every agent runs in an isolated Docker container with its own filesystem, network policy, and resource limits. No agent touches the host.

- :material-shield-lock:{ .lg .middle } **Three-Layer Governance**

  ***

  SandboxBoundary x CapabilityPolicy x ManifestInline — enterprise-grade access control with deny-always semantics and human-in-the-loop grants.

- :material-brain:{ .lg .middle } **Multi-Provider LLM Routing**

  ***

  In-process routing to Ollama, LM Studio, OpenAI, Anthropic, Google, or any OpenAI-compatible endpoint. Every token metered and budgeted.

- :material-eye:{ .lg .middle } **Full Observability**

  ***

  Real-time thought streaming, Merkle hash-chain audit trail, token metering, circuit breakers, and egress proxy logging.

- :material-puzzle:{ .lg .middle } **Extensible by Design**

  ***

  Skills (guidance documents), MCP tools (executable functions), agent templates, and BYOH agents — all extensible without touching core.

- :material-account-group:{ .lg .middle } **Multi-Agent Coordination**

  ***

  Circles for team collaboration, git-backed shared knowledge, structured delegation chains, and inter-agent messaging via Centrifugo.

</div>

## Architecture at a Glance

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

## Quick Start

```bash
git clone https://github.com/TKCen/sera.git && cd sera
cp .env.example .env        # Configure LLM provider and API keys
bun run dev:up               # Start the full stack in dev mode
```

Open [http://localhost:3000](http://localhost:3000) to access the dashboard.

[:octicons-arrow-right-24: Getting Started](getting-started/index.md){ .md-button .md-button--primary }
[:octicons-book-24: Architecture](architecture/index.md){ .md-button }
[:octicons-code-24: API Reference](api/index.md){ .md-button }

## Philosophy

> Agents should be tenants, not residents. They earn access, operate within boundaries, and leave an auditable trail. The human stays in control — not by limiting what agents can do, but by making everything they do legible.

---

_SERA — your agents, your network, your rules._
