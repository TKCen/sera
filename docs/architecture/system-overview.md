# System Overview

SERA is a Docker-native multi-agent AI orchestration platform. This page provides a high-level view of how the system fits together.

## The Big Picture

```
Operator (browser / CLI / TUI)
        │
        ▼
┌─── sera-web ────────────────────────────────────────────┐
│  React SPA — dashboard, chat, agent management          │
│  Subscribes to Centrifugo for real-time streams          │
└───────────────────────┬─────────────────────────────────┘
                        │ REST + WebSocket
┌───────────────────────▼─────────────────────────────────┐
│                  sera-core (Mind)                         │
│                                                          │
│  ┌──────────┐  ┌───────────┐  ┌──────────────┐          │
│  │Orchestr. │  │ LLM Proxy │  │  Capability   │          │
│  │          │  │           │  │  Engine       │          │
│  └────┬─────┘  └─────┬─────┘  └──────────────┘          │
│       │              │                                    │
│  ┌────▼─────┐  ┌─────▼─────┐  ┌──────────────┐          │
│  │ Sandbox  │  │ Metering  │  │   Audit      │          │
│  │ Manager  │  │ Service   │  │   Service    │          │
│  └────┬─────┘  └───────────┘  └──────────────┘          │
│       │                                                   │
│  ┌────▼─────┐  ┌───────────┐  ┌──────────────┐          │
│  │  Skill   │  │   MCP     │  │   Memory     │          │
│  │ Registry │  │ Registry  │  │  Manager     │          │
│  └──────────┘  └───────────┘  └──────────────┘          │
└──┬──────────────┬──────────────┬─────────────────────────┘
   │ Docker API   │ Centrifugo   │ SQL / Vector
   │              │              │
┌──▼──────┐  ┌────▼────┐  ┌─────▼──────────────────┐
│  Agent  │  │Centrifugo│  │ PostgreSQL + pgvector  │
│Container│  │  (Pulse) │  │ Qdrant (vectors)       │
└──┬──────┘  └─────────┘  └────────────────────────┘
   │
   ▼
┌─────────────────┐
│  Egress Proxy   │
│  (Squid)        │
└─────────────────┘
```

## Network Topology

SERA uses two Docker networks:

| Network     | Purpose                        | Members                                                        |
| ----------- | ------------------------------ | -------------------------------------------------------------- |
| `sera_net`  | Internal service communication | sera-core, sera-web, sera-db, centrifugo, qdrant, egress-proxy |
| `agent_net` | Agent container network        | Agent containers, egress-proxy, sera-web                       |

Agent containers communicate with sera-core via `sera_net` (for LLM proxy and API calls) and route all external traffic through the egress proxy on `agent_net`.

## Request Flow

1. **User sends a message** via the web dashboard or API
2. **sera-core routes it** to the target agent's container chat server
3. **Agent runtime processes** the message through its reasoning loop (observe → plan → act → reflect)
4. **LLM calls go through sera-core** — JWT validated, budget checked, provider resolved, usage recorded
5. **Tool calls execute locally** inside the container (file operations, shell commands)
6. **Thoughts stream in real-time** via Centrifugo to the dashboard
7. **Knowledge is stored** via the knowledge-store tool → sera-core → Qdrant
8. **Every action is audited** in the Merkle hash-chain audit trail

## What Makes SERA Different

| Concern         | Typical framework | SERA                                                |
| --------------- | ----------------- | --------------------------------------------------- |
| Agent isolation | Process or thread | Docker container with capability policy             |
| LLM access      | Direct API calls  | Proxied through core with metering and budgets      |
| Permissions     | All-or-nothing    | Three-layer model with runtime escalation           |
| Network access  | Unrestricted      | Per-agent domain filtering via egress proxy         |
| Audit           | Logging           | Merkle hash-chain with tamper detection             |
| Knowledge       | Shared database   | Scoped (personal/circle/global) with git provenance |
| Extension model | Plugin code       | Skills (documents) + MCP tools (containers)         |
