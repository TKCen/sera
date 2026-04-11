# 🏗️ Agent Workspace Architecture: Circles, Containers & Federation

> This document defines the runtime topology for SERA agents — how they are organized, isolated, connected, and scaled. It incorporates patterns from **crewAI** (orchestration), **herm** (sandboxing), **OpenHands** (workspace model), **Letta** (memory blocks), **AutoGen** (dynamic agent creation), and **BMAD-METHOD** (agent personas, skills architecture, party mode, project-context alignment).

---

## 📐 Core Concepts

### 1. The Agent

An **Agent** is an autonomous LLM-powered entity defined by an `AGENT.yaml` manifest. It runs inside a long-lived **Brain Container** and has its own:
- **Workspace** — a filesystem volume for working data
- **Memory** — structured blocks (Human, Persona, Core, Archive) in the style of Letta
- **Skills** — registered capabilities the agent can invoke (inspired by BMAD skills architecture)
- **Tools** — external services available via MCP or direct API
- **Identity** — persona, communication style, and principles (inspired by BMAD agent personas)

### 2. The Subagent

A **Subagent** is a short-lived agent spawned by a parent agent to handle a delegated task. Subagents:
- Share the parent's **workspace volume** (read/write access to the same filesystem)
- Share the parent's **knowledge base** (Qdrant collection + PostgreSQL namespace)
- Run in their own container with their own `AGENT.yaml` (can have different model, tools, tier)
- Report back to the parent via the internal event bus
- Are **ephemeral** — they stop after their task completes

### 3. The Circle

A **Circle** is an organizational group of top-level agents that share:
- A **common knowledge channel** for persistent data exchange
- An **intercom mesh** for real-time messaging between member agents
- A **project context** (*inspired by BMAD's `project-context.md`*) — a shared "constitution" document defining standards, conventions, and decisions that all agents in the circle must follow
- An **agent manifest** (*inspired by BMAD's `agent-manifest.csv`*) — a registry of all agents in the circle with their roles, capabilities, and identities

Circles do **not** share workspace volumes — each agent maintains its own workspace. Knowledge sharing between agents in a circle is explicit and opt-in via knowledge channels.

### 4. Circle Connections (Federation)

Circles can be **connected** to other circles, even across hosts or SERA instances:
- Connected circles can exchange messages via **bridge channels**
- Agents can address agents in connected circles using qualified names (e.g., `researcher@ops-circle`)
- Knowledge can be selectively published to connected circles
- Each connection has its own authentication and access policy

---

## 🔬 Container Topology

### Single-Host Architecture (Phase 1)

```
┌──────────────────────────────────────────────────────────────────┐
│                        SERA Infrastructure                        │
│                                                                   │
│  sera-core ─────────── Sandbox Manager API ◄──── Agent requests   │
│  sera-web  ─────────── Dashboard / UI                             │
│  centrifugo ────────── Event bus + Intercom backbone              │
│  sera-db ──────────── PostgreSQL (audit, metadata, memory)        │
│  qdrant ───────────── Vector DB (semantic knowledge)              │
│                                                                   │
├───────────────────────────────────────────────────────────────────┤
│                                                                   │
│  ┌─ Circle: "Development" ────────────────────────────────────┐  │
│  │  project-context.md (shared constitution)                   │  │
│  │  agent-manifest.yaml (circle roster)                        │  │
│  │                                                             │  │
│  │  ┌──────────────────┐      ┌──────────────────┐            │  │
│  │  │ Brain: Architect │◄────►│ Brain: Developer  │            │  │
│  │  │ vol: /workspace  │ inter│ vol: /workspace   │            │  │
│  │  │ vol: /memory     │  com │ vol: /memory      │            │  │
│  │  └───────┬──────────┘      └───────┬───────────┘            │  │
│  │          │                         │                        │  │
│  │    ┌─────┴─────┐            ┌──────┴──────┐                 │  │
│  │    │ Subagent: │            │ Tool:       │                 │  │
│  │    │ Researcher│            │ Terminal    │                 │  │
│  │    │ (shares   │            │ (shares     │                 │  │
│  │    │ workspace)│            │ workspace)  │                 │  │
│  │    └───────────┘            └─────────────┘                 │  │
│  └─────────────────────────────────────────────────────────────┘  │
│                         ▲                                         │
│                    bridge channel                                  │
│                         ▼                                         │
│  ┌─ Circle: "Operations" ─────────────────────────────────────┐  │
│  │                                                             │  │
│  │  ┌──────────────────┐      ┌──────────────────┐            │  │
│  │  │ Brain: Monitor   │◄────►│ Brain: Deployer   │            │  │
│  │  └──────────────────┘      └──────────────────┘            │  │
│  └─────────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────────┘
```

### Key Design Decisions

| Decision | Rationale |
|---|---|
| **Brain containers are long-running** | They hold the agent's reasoning loop, memory, and conversation state. Restarting them would lose context. |
| **Tool/subagent containers are ephemeral** | They execute a single action and stop. This limits blast radius and resource consumption. |
| **No Docker socket in agent containers** | Agents request container spawning via the Sandbox Manager API in `sera-core`. This enforces RBAC, resource limits, and security tiers. |
| **Workspaces are shared within an agent tree** | Parent agent and its subagents operate on the same filesystem. This eliminates serialization overhead. |
| **Knowledge is scoped per circle** | Agents within a circle share a knowledge namespace. Between circles, sharing is explicit. |
| **Project context is per circle** | Inspired by BMAD's `project-context.md`, this ensures all agents in a circle follow the same standards and conventions. |

---

## 📝 The AGENT.yaml Schema

Every agent is defined by a declarative manifest, inspired by BMAD's `bmad-skill-manifest.yaml` and Kubernetes pod specs:

```yaml
apiVersion: sera/v1
kind: Agent
metadata:
  name: architect-prime
  displayName: Winston                      # BMAD-inspired persona name
  icon: "🏗️"
  circle: development                       # Which circle this agent belongs to
  tier: 2                                   # Security tier (1=ReadOnly, 2=Internal, 3=Executive)

# ── Identity (BMAD-inspired) ──────────────────────────────────────
identity:
  role: "System Architect + Technical Design Leader"
  description: >
    Senior architect with expertise in distributed systems,
    cloud infrastructure, and API design.
  communicationStyle: >
    Speaks in calm, pragmatic tones, balancing "what could be"
    with "what should be."
  principles:
    - "User journeys drive technical decisions"
    - "Embrace boring technology for stability"
    - "Design simple solutions that scale when needed"

# ── Model Configuration ──────────────────────────────────────────
model:
  provider: lm-studio                       # or: openai, anthropic, ollama
  name: qwen3-30b-a3b
  temperature: 0.7
  fallback:                                 # Multi-model fallback (goose-inspired)
    - provider: openai
      name: gpt-4o-mini
      maxComplexity: 3                      # Use for simple tasks only

# ── Capabilities ─────────────────────────────────────────────────
tools:
  allowed:
    - file-read
    - file-write
    - knowledge-store
    - knowledge-query
    - web-search
  denied:
    - shell-exec
    - docker-exec

skills:                                     # BMAD-inspired skills
  - create-architecture
  - check-implementation-readiness
  - adversarial-review

subagents:
  allowed:
    - role: researcher
      maxInstances: 3
    - role: browser
      maxInstances: 1
      requiresApproval: true                # Human-in-the-loop gate

# ── Intercom ─────────────────────────────────────────────────────
intercom:
  canMessage:                               # Direct messaging peers
    - developer-prime
    - reviewer-prime
  channels:                                 # Pub/sub channels
    publish:
      - architecture-decisions
    subscribe:
      - code-review-requests
      - research-findings

# ── Resources ────────────────────────────────────────────────────
resources:
  memory: 512Mi
  cpu: "0.5"

# ── Storage ──────────────────────────────────────────────────────
workspace:
  provider: local                           # or: nfs, s3
  path: /workspaces/architect-prime

memory:
  personalMemory: /memory/architect-prime
  sharedKnowledge: development-knowledge    # Circle-scoped Qdrant collection
```

---

## 🔄 The Circle Schema

```yaml
apiVersion: sera/v1
kind: Circle
metadata:
  name: development
  displayName: "Development Circle"
  description: "Software architecture, development, and code review"

# ── Project Context (BMAD-inspired "constitution") ───────────────
projectContext:
  path: /circles/development/project-context.md
  autoLoad: true                            # All agents load this on activation

# ── Agent Roster ─────────────────────────────────────────────────
agents:
  - architect-prime
  - developer-prime
  - reviewer-prime
  - qa-prime

# ── Knowledge Scope ──────────────────────────────────────────────
knowledge:
  qdrantCollection: development-knowledge
  postgresSchema: circle_development

# ── Intercom Channels ────────────────────────────────────────────
channels:
  - name: architecture-decisions
    type: persistent                        # Messages are stored for replay
  - name: code-review-requests
    type: ephemeral                         # Messages expire after consumption
  - name: research-findings
    type: persistent

# ── Party Mode (BMAD-inspired) ───────────────────────────────────
partyMode:
  enabled: true
  orchestrator: architect-prime             # Who facilitates group discussions
  selectionStrategy: relevance              # or: round-robin, all

# ── Connected Circles ────────────────────────────────────────────
connections:
  - circle: operations
    bridgeChannels:
      - deployment-requests                 # Dev circle publishes, Ops subscribes
      - incident-reports                    # Ops circle publishes, Dev subscribes
    auth: internal                          # Same SERA instance
  - circle: research@sera.friend.lab        # Remote SERA instance
    bridgeChannels:
      - shared-findings
    auth:
      type: mtls
      certPath: /certs/friend-lab.pem
```

---

## 🏭 Sandbox Manager API

Agents do **not** interact with Docker directly. All container operations go through the Sandbox Manager, a service inside `sera-core`:

```
Agent Brain ──── HTTP/gRPC ────► Sandbox Manager ────► Docker API
                                       │
                                       ├── Validates AGENT.yaml permissions
                                       ├── Enforces security tier limits
                                       ├── Applies resource constraints
                                       ├── Records audit trail (Merkle)
                                       └── Returns container handle
```

### Operations

| Operation | Description |
|---|---|
| `POST /sandbox/spawn` | Spawn a subagent or tool container. Validates against the requesting agent's `subagents.allowed` and `tools.allowed`. |
| `POST /sandbox/exec` | Execute a command in an existing container. |
| `DELETE /sandbox/{id}` | Terminate a container. |
| `GET /sandbox/{id}/logs` | Stream container logs via Centrifugo. |
| `GET /sandbox/status` | List all running containers for an agent tree. |

### Security Tiers (applied automatically)

| Tier | Network | Filesystem | Use Case |
|---|---|---|---|
| **1 — Read Only** | None | Read-only workspace mount | Analysis, code review |
| **2 — Internal** | `sera_net` only | Read-write workspace mount | Development, testing |
| **3 — Executive** | Full internet | Read-write + ephemeral scratch | Research, web automation |

---

## 📡 Intercom Architecture

The intercom uses **Centrifugo** as the backbone, with structured channel namespaces:

### Channel Namespaces

```
internal:agent:{agent-id}:thoughts      ← Agent's reasoning stream (UI only)
internal:agent:{agent-id}:terminal      ← Tool container stdout/stderr
intercom:{circle}:{agent-a}:{agent-b}   ← Private DM between two agents
channel:{circle}:{channel-name}         ← Circle-scoped pub/sub channel
bridge:{circle-a}:{circle-b}:{channel}  ← Cross-circle bridge channel
public:status:{agent-id}                ← Agent status (externally subscribable)
external:{subscriber-id}:inbox          ← Inbound from external consumers
```

### Event Schema

All intercom messages follow a standard envelope:

```typescript
interface IntercomMessage {
  id: string;                    // UUID
  timestamp: string;             // ISO 8601
  source: {
    agent: string;               // Agent name
    circle: string;              // Circle name
    instance?: string;           // SERA instance (for federation)
  };
  target: {
    channel: string;             // Full channel path
  };
  type: 'message' | 'knowledge' | 'task' | 'status' | 'approval-request';
  payload: Record<string, any>;
  metadata: {
    securityTier: number;
    replyTo?: string;            // For threaded conversations
    ttl?: number;                // Message expiry in seconds
  };
}
```

---

## 🧠 Learnings from BMAD-METHOD

The following BMAD patterns are directly adapted into SERA's architecture:

### 1. Agent Personas (Identity System)
BMAD defines agents with rich personas — names, communication styles, principles, expertise areas. SERA adopts this in the `identity` block of `AGENT.yaml`. This matters because:
- Agents with defined personas produce more consistent, in-character output
- Users can build trust relationships with named agents (e.g., "Winston the Architect")
- Persona principles act as soft guardrails on agent behavior

### 2. Skills Architecture
In BMAD, agents have skills (registered capabilities with canonical IDs). SERA adapts this as:
- Each agent's `skills` list defines what workflows/tools it can invoke
- Skills are composable — skills can invoke other skills
- New skills can be added without changing the agent's core persona

### 3. Party Mode → Circle Discussions
BMAD's party mode brings multiple agent personas into one conversation with an orchestrator selecting relevant agents per message. SERA elevates this from a single-session trick to a persistent architectural feature:
- **Party mode** becomes a first-class capability of Circles
- The Circle's `partyMode.orchestrator` manages group discussions
- Selection strategy determines which agents participate (relevance-based, round-robin, or all)
- Unlike BMAD (which runs in one LLM context), SERA's party mode runs across actual separate agent containers with real tool access

### 4. Project Context → Circle Constitution
BMAD's `project-context.md` is a single document that all agents read to follow consistent standards. SERA adopts this per-circle:
- Each circle has a `project-context.md` loaded on agent activation
- It defines technology decisions, naming conventions, API patterns, and architectural constraints
- This prevents the conflict patterns BMAD documents (inconsistent API styles, conflicting state management, etc.)
- The constitution is a living document — agents can propose amendments via the intercom

### 5. Workflow Phases → Agent Task Lifecycle
BMAD's 4-phase workflow (Analysis → Planning → Solutioning → Implementation) maps to how agents approach work:
- **Phase 1 (Analysis)**: Researcher subagent gathers context
- **Phase 2 (Planning)**: Main agent creates a structured plan
- **Phase 3 (Solutioning)**: Architecture decisions documented in project context
- **Phase 4 (Implementation)**: Developer subagent executes, QA subagent verifies

### 6. Conflict Prevention → Shared Architectural Context
BMAD prevents agent conflicts through explicit ADRs (Architecture Decision Records) loaded as context. SERA implements this via:
- Circle-level project context (shared ADRs)
- Circle-level knowledge channels (persistent decision log)
- The Sandbox Manager enforcing that agents operate within their declared capabilities

---

## 🗺️ Implementation Plan: Single-Host First

### Phase A: Foundation (Prerequisites)

These components must exist before agents can run:

1. **AGENT.yaml Parser**
   - Location: `sera/core/src/agents/AgentManifest.ts`
   - Parse and validate `AGENT.yaml` files against the schema
   - Resolve model provider configuration
   - Validate tool/subagent permissions

2. **Circle Registry**
   - Location: `sera/core/src/circles/CircleRegistry.ts`
   - Load and validate `CIRCLE.yaml` files
   - Maintain the agent roster per circle
   - Manage project-context loading

3. **Sandbox Manager**
   - Location: `sera/core/src/sandbox/SandboxManager.ts`
   - Express routes: `POST /sandbox/spawn`, `POST /sandbox/exec`, etc.
   - Docker API integration via `dockerode`
   - RBAC enforcement against `AGENT.yaml` permissions
   - Resource limit application per security tier

### Phase B: Agent Runtime

4. **Brain Container Lifecycle**
   - Location: `sera/core/src/agents/BrainManager.ts`
   - Start/stop brain containers based on `AGENT.yaml`
   - Mount workspace and memory volumes
   - Inject environment variables (model config, API keys, Centrifugo URL)
   - Health monitoring and restart policy

5. **Agent Identity System**
   - Location: `sera/core/src/agents/Identity.ts`
   - Load persona, communication style, principles from `AGENT.yaml`
   - Generate system prompts from identity configuration
   - Support BMAD-style persona consistency

6. **Skills Framework**
   - Location: `sera/core/src/skills/SkillRegistry.ts`
   - Register skills as invocable capabilities
   - Skill composition (skills can call other skills)
   - Skill manifest per agent

### Phase C: Intercom & Knowledge

7. **Intercom Service**
   - Location: `sera/core/src/intercom/IntercomService.ts`
   - Centrifugo channel management (create, subscribe, publish)
   - Channel namespace enforcement
   - Message envelope serialization/deserialization

8. **Circle Knowledge Manager**
   - Location: `sera/core/src/circles/KnowledgeManager.ts`
   - Per-circle Qdrant collection management
   - Knowledge channel ingestion (intercom → vector store)
   - Project context loading and distribution

9. **Party Mode Engine**
   - Location: `sera/core/src/circles/PartyMode.ts`
   - Multi-agent orchestrated discussion within a circle
   - Agent selection based on relevance analysis
   - Cross-agent reference and response threading

### Phase D: Storage Abstraction

10. **Storage Provider Interface**
    - Location: `sera/core/src/storage/StorageProvider.ts`
    - Interface: `mount(agentId, config) → volumeMount`
    - Implementations:
      - `LocalStorageProvider` — bind mounts (default)
      - `DockerVolumeProvider` — named Docker volumes
    - Future: `NFSStorageProvider`, `S3StorageProvider`

---

## 🚀 Expansion Steps

### Step 1: Multi-Host (Docker Swarm)

| Change | Details |
|---|---|
| **Centrifugo** | Add Redis broker config for multi-node channel sync |
| **Sandbox Manager** | Replace `docker.createContainer` with `docker.createService` (Swarm mode) |
| **Storage** | Add `NFSStorageProvider` for cross-host workspace access |
| **Circle Registry** | No change needed — circles are logical, not physical |
| **sera-core** | Deploy as a Swarm service with a single manager replica |

**Centrifugo config change:**
```json
{
  "broker": "redis",
  "redis_address": "redis://sera-redis:6379"
}
```

**Swarm deployment:**
```bash
docker stack deploy -c docker-compose.yaml sera
```

### Step 2: External Subscribers

| Change | Details |
|---|---|
| **Centrifugo** | Configure JWT auth with channel-scoped claims |
| **Channel namespaces** | Enable `public:` namespace for external access |
| **sera-core** | Add `/auth/token` endpoint to issue scoped JWTs |
| **NPM** | TLS termination for `ws://sera.yourdomain.com` |

**JWT claims example:**
```json
{
  "sub": "external-client-abc",
  "channels": ["public:status:*", "external:abc:inbox"],
  "exp": 1735689600
}
```

### Step 3: Federation (SERA-to-SERA)

| Change | Details |
|---|---|
| **Bridge Service** | New service: `sera/core/src/federation/BridgeService.ts` |
| **Authentication** | mTLS between SERA instances (or WireGuard tunnel) |
| **Agent addressing** | Qualified names: `agent@circle@instance` |
| **Knowledge sync** | Event-sourced knowledge deltas via bridge channels |
| **Circle connections** | `CIRCLE.yaml` `connections` block with remote circle references |

**Architecture:**
```
SERA Instance A                          SERA Instance B
┌──────────────┐                        ┌──────────────┐
│ Centrifugo   │◄── mTLS/WireGuard ───►│ Centrifugo   │
│ Bridge Svc   │                        │ Bridge Svc   │
│              │                        │              │
│ Circle: Dev  │◄── bridge channels ──►│ Circle: QA   │
└──────────────┘                        └──────────────┘
```

### Step 4: Kubernetes Migration (Optional, Long-term)

| SERA Concept | Kubernetes Equivalent |
|---|---|
| Brain Container | StatefulSet (1 replica per agent) |
| Tool Container | Job (ephemeral, runs to completion) |
| Subagent Container | Job with shared PVC |
| Workspace Volume | PersistentVolumeClaim (PVC) |
| Circle | Namespace |
| Sandbox Manager | Custom Controller / Operator |
| AGENT.yaml | CustomResourceDefinition (CRD) |

---

## 📐 Design Principles Summary

1. **Agents never touch Docker directly** — all container operations go through the Sandbox Manager API.
2. **Circles are the organizational unit** — agents are grouped, knowledge is scoped, and context is shared at the circle level.
3. **Personas are first-class** — every agent has a defined identity that shapes its behavior, inspired by BMAD.
4. **Storage is pluggable** — the same agent runs on local bind mounts, NFS shares, or S3 depending on the provider.
5. **Intercom is Centrifugo-native** — scales from single-host to multi-host to federated with configuration changes only.
6. **Project context prevents conflicts** — every circle has a constitution document that all member agents follow.
7. **The AGENT.yaml is the single source of truth** — it defines what an agent is, what it can do, and who it can talk to.
