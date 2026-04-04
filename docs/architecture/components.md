# Component Architecture

## sera-core

The central intelligence and policy enforcement point. All API surfaces, orchestration, and governance live here.

### Key Modules

| Module                     | Responsibility                                                           |
| -------------------------- | ------------------------------------------------------------------------ |
| `Orchestrator`             | Agent lifecycle: load manifests, create instances, start/stop containers |
| `AgentFactory`             | DB-backed agent creation from YAML manifests                             |
| `SandboxManager`           | Docker container lifecycle via dockerode, tier policy enforcement        |
| `LlmRouter`                | In-process LLM routing via pi-mono provider functions                    |
| `ProviderRegistry`         | Model-name mapping, provider config, API key resolution                  |
| `SkillRegistry`            | Central registry of named skills (text guidance + MCP tool bridges)      |
| `MCPRegistry`              | Manages connections to MCP server processes                              |
| `MemoryManager`            | Hybrid block store + vector indexing via Qdrant                          |
| `MeteringService`          | Token usage tracking, hourly/daily quota enforcement                     |
| `AuditService`             | Merkle hash-chain event log in PostgreSQL                                |
| `IntercomService`          | Centrifugo pub/sub for agent-to-agent and agent-to-UI messaging          |
| `ScheduleService`          | Cron-based and one-shot task scheduling per agent                        |
| `EgressAclManager`         | Generates per-agent Squid ACL files from resolved network capabilities   |
| `PermissionRequestService` | Runtime capability escalation with operator approval                     |
| `ChannelRouter`            | Routes messages to/from external channels (Discord, Slack, etc.)         |
| `SecretsProvider`          | AES-256-GCM encrypted secret storage in PostgreSQL                       |

**Runtime:** Node.js 22 LTS (TypeScript, ES Modules)
**HTTP framework:** Express 5
**Port:** 3001

### Source Layout

```
core/src/
  agents/       # AgentFactory, instance management, process flows
  audit/        # Merkle hash-chain audit trail
  auth/         # AuthPlugin interface, API key + OIDC providers
  capability/   # Resolution engine — NamedList / Policy / Boundary
  channels/     # Outbound notification channel adapters
  circles/      # Circle management, knowledge scoping
  db/           # PostgreSQL client, migrations, import-on-load
  intercom/     # Centrifugo pub/sub (IntercomService)
  llm/          # LlmRouter, ProviderRegistry
  mcp/          # MCP registry, protocol client, sera-core MCP server
  memory/       # MemoryBlockStore, KnowledgeGitService, EmbeddingService
  metering/     # Token usage tracking, budget enforcement
  routes/       # HTTP route handlers (~50 route files)
  sandbox/      # SandboxManager, EgressAclManager, TierPolicy
  secrets/      # SecretsProvider (PostgreSQL AES-256-GCM)
  services/     # Cross-cutting services
  skills/       # SkillLibrary, loader, hot-reload
  tools/        # Built-in tool implementations
```

## Agent Runtime

A minimal TypeScript process that runs **inside each agent container**. It is not a copy of sera-core — it is a lightweight loop purpose-built for the sandbox environment.

| Module                | Responsibility                                           |
| --------------------- | -------------------------------------------------------- |
| `ReasoningLoop`       | Agentic loop: observe → plan → act → reflect             |
| `LLMClient`           | HTTP client for sera-core's LLM proxy                    |
| `ContextManager`      | Context window management and compaction                 |
| `SystemPromptBuilder` | Dynamic system prompt assembly from skills and identity  |
| `ToolLoopDetector`    | Infinite loop prevention                                 |
| `tools/`              | Local tool execution (file-read, file-write, shell-exec) |

**Runtime:** Bun (runs TypeScript directly, no build step)
**Image:** `sera-agent-worker:latest` (built from `core/sandbox/Dockerfile.worker`)

!!! note "Agents never call LLM providers directly"
All LLM calls go through sera-core's proxy at `/v1/llm/chat/completions`. This ensures metering, budget enforcement, and audit logging.

## sera-web

The operator dashboard. A React SPA that communicates with sera-core via REST and subscribes to Centrifugo for real-time streams.

**Stack:** Vite v6 + React 19 + React Router v7 + TanStack Query v5 + Tailwind CSS v4

### Dashboard Pages

| Page          | Function                                         |
| ------------- | ------------------------------------------------ |
| Dashboard     | System overview, agent status, usage stats       |
| Agents        | List, create, edit, start/stop agents            |
| Chat          | Conversational interface with any agent          |
| Memory        | Memory explorer, block viewer, knowledge graph   |
| Templates     | Agent template gallery                           |
| Schedules     | Task scheduling management                       |
| Channels      | Integration channel configuration                |
| Circles       | Circle management and member coordination        |
| MCP Servers   | MCP server registry and status                   |
| Providers     | LLM provider configuration                       |
| Audit         | Audit trail viewer with Merkle verification      |
| Health        | System health dashboard                          |
| Settings      | Instance configuration                           |
| Introspection | Real-time agent thought and communication viewer |

## Infrastructure Services

| Service          | Image                          | Role                                                               |
| ---------------- | ------------------------------ | ------------------------------------------------------------------ |
| **Centrifugo**   | `centrifugo/centrifugo:latest` | Real-time WebSocket pub/sub for thought streaming and agent status |
| **PostgreSQL**   | `pgvector/pgvector:pg15`       | Relational data + 768-dim pgvector embedding index                 |
| **Qdrant**       | `qdrant/qdrant:latest`         | Dedicated vector store, namespaced per agent/circle                |
| **Egress Proxy** | Custom (Squid)                 | SNI-based HTTPS filtering, per-agent ACLs, bandwidth limiting      |
