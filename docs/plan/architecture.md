# SERA 2.0 — Architecture Diagrams

> **Companion to:** [plan.md](plan.md) (PRD v0.3) · [Spec Index](specs/README.md)  
> **Purpose:** Visual reference for reasoning about the system  
> **Date:** 2026-04-10

---

## 1. The Big Picture

Everything in SERA flows through a single control plane — the **Gateway**. Clients and external channels push events in; the gateway routes them to agent harnesses; harnesses think, act, and respond. The gateway handles channel binding, session persistence, auth, config, etc. Harnesses are exchangeable workers connected via a transport layer (stdin/out, gRPC, etc.).

```mermaid
graph LR
    classDef client fill:#4a9eff,stroke:#2b7de9,color:#fff
    classDef channel fill:#f59e0b,stroke:#d97706,color:#fff
    classDef core fill:#10b981,stroke:#059669,color:#fff
    classDef harness fill:#8b5cf6,stroke:#7c3aed,color:#fff
    classDef external fill:#6b7280,stroke:#4b5563,color:#fff

    subgraph Clients
        CLI["CLI"]:::client
        TUI["TUI"]:::client
        WEB["Web SPA"]:::client
        SDK["SDKs"]:::client
        HMI["Thin Clients / HMIs"]:::client
    end

    subgraph Channels
        DISC["Discord"]:::channel
        SLACK["Slack"]:::channel
        WH["Webhooks"]:::channel
    end

    GW["⚡ SERA Gateway\n(Control Plane)"]:::core

    subgraph "Agent Harnesses"
        HN["Built-in Harness\n(Rust, stdin/out transport)"]:::harness
        EXT_HN["External Harness\n(Transport: gRPC, any language)"]:::harness
    end

    subgraph "External Ecosystem"
        MCP_EXT["MCP Servers"]:::external
        A2A_EXT["A2A Agents"]:::external
        ACP_EXT["ACP Agents"]:::external
    end

    CLI & TUI & SDK -->|"gRPC / WS"| GW
    WEB & HMI -->|"AG-UI / WS"| GW
    DISC & SLACK & WH -->|"gRPC adapter"| GW

    GW -->|"dispatch turn"| HN
    GW -->|"dispatch turn"| EXT_HN

    GW <-->|"MCP"| MCP_EXT
    GW <-->|"A2A"| A2A_EXT
    GW <-->|"ACP"| ACP_EXT
```

---

## 2. Gateway Internals

The gateway process itself is **stateless** to enable seamless horizontal scaling. All durability requirements (queues, active sessions, metadata) are delegated to a **pluggable storage plane** (which defaults simply to the local file system or SQLite for local dev, but scales to Redis and PostgreSQL for production). While the gateway process is stateless, it owns the central *logic* for routing, queuing, sessions, auth, scheduling, and hook orchestration.

```mermaid
graph TB
    classDef ingress fill:#4a9eff,stroke:#2b7de9,color:#fff
    classDef queue fill:#f59e0b,stroke:#d97706,color:#fff
    classDef session fill:#10b981,stroke:#059669,color:#fff
    classDef auth fill:#ef4444,stroke:#dc2626,color:#fff
    classDef hook fill:#8b5cf6,stroke:#7c3aed,color:#fff
    classDef sched fill:#ec4899,stroke:#db2777,color:#fff
    classDef obs fill:#6b7280,stroke:#4b5563,color:#fff

    INGRESS["Event Ingress\n(WS · gRPC · Webhook)"]:::ingress

    subgraph "sera-gateway"
        ROUTER["Event Router"]:::ingress
        QUEUE["Lane-Aware FIFO Queue\n(modes: collect · followup · steer · interrupt)"]:::queue
        SESSION["Session State Machine\n(Created → Active → Compacting → Archived)"]:::session
        SCHED["Scheduler\n(cron · workflows · heartbeats)"]:::sched
        OTEL["Observability\n(OpenTelemetry · Audit Log)"]:::obs
        PLUGINS["Plugin & Connector Registry\n(hot-reload · lifecycle)"]:::obs
        CONFIG["Config Surface\n(agent-accessible · bundled docs)"]:::obs
    end

    subgraph "sera-hooks (Default WASM Plugins)"
        HOOK_EDGE["⛓ Edge Ingress\n(Webhooks, Callbacks)"]:::hook
        HOOK_PRE["⛓ pre_route\n(AuthZ, Dedupe, Rate-Limit, PII)"]:::hook
        HOOK_POST["⛓ post_route\n(Audit Logging, HITL hooks)"]:::hook
    end

    HARNESS["→ Agent Harness"]:::queue

    INGRESS --> HOOK_EDGE --> ROUTER
    ROUTER -.->|"chain (AuthZ / Policy)"| HOOK_PRE
    HOOK_PRE -.-> ROUTER
    ROUTER --> QUEUE
    QUEUE --> SESSION
    SESSION -->|"dequeue turn"| HARNESS
    SESSION -.->|"chain (Audit / HITL / Async)"| HOOK_POST
    HOOK_POST -.-> SESSION
    SCHED -->|"trigger"| QUEUE
    ROUTER & SESSION --> OTEL
```

- **Ultra-Slim Core**: The Gateway isolates the heavy logic (Dedupe, Auth, HITL Escalation, Credential Injection) purely into default WASM Plugins (`sera-hooks`), keeping the core focused uniquely on queueing, scaling, and state machine lifecycle.
- **Lane-aware queue**: One writer per session (no races), global concurrency cap
- **Hook chains shape behavior**: `Edge` hooks process direct webhooks, `pre_route` hooks filter and authenticate before queuing, and `post_route` hooks handle async tasks like HITL approvals and custom audit logging.
- **Scheduler** drives cron jobs, dreaming workflows, and heartbeat checks

---

## 3. Agent Harness — The Turn Loop

The harness is the "thinking + doing" worker. It is **stateless per-turn** and **session-scoped**. We provide a built-in harness, but effectively they are exchangeable. Harnesses can run on the same machine, but they don't have to — they are connected to the gateway via a transport layer (which can be as basic as stdin/out, or gRPC).

```mermaid
graph LR
    classDef input fill:#4a9eff,stroke:#2b7de9,color:#fff
    classDef ctx fill:#10b981,stroke:#059669,color:#fff
    classDef model fill:#8b5cf6,stroke:#7c3aed,color:#fff
    classDef tool fill:#f59e0b,stroke:#d97706,color:#fff
    classDef mem fill:#ec4899,stroke:#db2777,color:#fff
    classDef hook fill:#6b7280,stroke:#4b5563,color:#888

    EVENT["Event + Session"]:::input

    subgraph "Turn Loop (sera-harness)"
        direction LR
        PRE["⛓ pre_turn"]:::hook
        CTX["Context Assembly\n(KV-cache optimized)"]:::ctx
        LLM["Model Call\n(provider adapter)"]:::model
        TOOLS["Tool Execution\n(registry · sandbox)"]:::tool
        POST["⛓ post_turn"]:::hook
        MEM["Memory Write\n(tiered pipeline)"]:::mem
        DELIVER["Deliver Response\n(⛓ pre/post_deliver)"]:::input
    end

    EVENT --> PRE --> CTX --> LLM
    LLM -->|"tool_call"| TOOLS
    TOOLS -->|"⛓ pre/post_tool"| TOOLS
    TOOLS -->|"result"| LLM
    LLM -->|"final response"| POST --> MEM --> DELIVER
```

The inner tool loop may cycle multiple times (agent decides when it's done).

---

## 4. Context Engineering

> **Guiding principle:** Find the **smallest set of high-signal tokens** that maximize the likelihood of the desired outcome. Context is a finite attention budget with diminishing marginal returns — every token added costs attention elsewhere.
>
> _Ref: [Anthropic — Effective Context Engineering for AI Agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)_

### The Problem

LLMs suffer from **context rot**: as token count increases, the model's ability to recall and reason over information degrades. This isn't a hard cliff but a performance gradient — models remain capable at longer contexts but show reduced precision. The context engine's job is to fight this by curating what enters the window at each turn.

### Entry & Exit Contract

The **Context Engine** is a pluggable trait. The harness hands it a `TurnContext` and gets back a `ContextWindow` — it doesn't care how context is assembled.

```mermaid
graph LR
    classDef input fill:#4a9eff,stroke:#2b7de9,color:#fff
    classDef engine fill:#10b981,stroke:#059669,color:#fff
    classDef output fill:#8b5cf6,stroke:#7c3aed,color:#fff

    IN["TurnContext\n(event · session state\n· agent config · principal)"]:::input
    ENGINE["ContextEngine (trait)\n─────────────────────\nassemble(&TurnContext)\n→ Result<ContextWindow>"]:::engine
    OUT["ContextWindow\n(messages array ready\nfor model call)"]:::output

    IN --> ENGINE --> OUT
```

```rust
#[async_trait]
pub trait ContextEngine: Send + Sync {
    async fn assemble(&self, ctx: &TurnContext) -> Result<ContextWindow, ContextError>;
    fn describe(&self) -> EngineDescription;
}
```

Any implementation — default pipeline, LCM-based, RAG-heavy, domain-specific — plugs in here.

### The Context Window (what the model actually sees)

Each turn, the engine assembles a fresh context window. This is what gets sent to the LLM:

```mermaid
graph TB
    classDef system fill:#dcfce7,stroke:#16a34a,color:#000
    classDef history fill:#fef9c3,stroke:#ca8a04,color:#000
    classDef current fill:#fecaca,stroke:#dc2626,color:#000
    classDef entry fill:#4a9eff,stroke:#2b7de9,color:#fff
    classDef exit fill:#8b5cf6,stroke:#7c3aed,color:#fff

    TOP["▼ Context Window (top → bottom = first → last token)"]:::entry

    SYS["System Prompt\n─────────────────────────────────────\nPersona · behavioral rules · tool guidance\nOutput format · constraints\n\nMinimal, clear, right altitude — not a laundry list\nof edge cases. Specific enough to guide, flexible\nenough to let the model be intelligent."]:::system

    TOOLS["Tool Definitions\n─────────────────────────────────────\nSelf-contained, minimal-overlap tool schemas\nwith descriptive parameters.\n\nIncludes base tools + any skill-activated tools\nfor the current mode."]:::system

    HISTORY["Conversation History\n─────────────────────────────────────\nPast turns: user messages · assistant responses\n· tool calls · tool results\n\nSubject to compaction — old tool results may\nbe cleared or summarized. Managed by the\nengine, not the caller."]:::history

    CURRENT["Current Turn\n─────────────────────────────────────\nUser message · attachments\n\n+ Just-in-time injections:\n  • Retrieved memories (for THIS query)\n  • Agent's scratchpad / notes\n  • File references pulled by the engine\n  • Any context the engine deems high-signal"]:::current

    BOTTOM["▼ → Model Call"]:::exit

    TOP --> SYS --> TOOLS --> HISTORY --> CURRENT --> BOTTOM
```

This is simply what LLM APIs expect: system message, then turn history, then the current turn. The interesting part is the **strategies** the engine uses to decide what goes into each section and how to keep it tight.

### Four Strategies for Managing the Attention Budget

```mermaid
graph LR
    classDef strat fill:#8b5cf6,stroke:#7c3aed,color:#fff
    classDef desc fill:#f3f4f6,stroke:#6b7280,color:#000

    S1["1 · Just-in-Time\nRetrieval"]:::strat
    S2["2 · Compaction"]:::strat
    S3["3 · Structured\nNote-Taking"]:::strat
    S4["4 · Sub-Agent\nCondensation"]:::strat

    D1["Don't pre-stuff context.\nMaintain lightweight references\n(file paths, queries, links) and\nlet the agent pull data via tools\nwhen it actually needs it."]:::desc
    D2["When context nears budget,\nsummarize the conversation\nand reinitiate with compressed\nhistory. Clear old tool results.\nPreserve decisions + open items."]:::desc
    D3["Agent writes persistent notes\noutside the context window\n(scratchpad, NOTES.md, memory).\nPulled back in when relevant.\nSurvives compaction."]:::desc
    D4["Sub-agents explore with full\ncontext windows. Return only\ncondensed summaries (1-2k tokens)\nto the lead agent. Deep work\nstays isolated."]:::desc

    S1 --- D1
    S2 --- D2
    S3 --- D3
    S4 --- D4
```

**How these map to SERA components:**

| Strategy | SERA Component | How It Works |
|---|---|---|
| **Just-in-time retrieval** | `sera-memory` (search tool), `sera-tools` (file/grep tools) | Agent maintains references; pulls context via tool calls when needed. Memory search is a _tool_, not automatic injection. Progressive disclosure through exploration. |
| **Compaction** | `ContextEngine` + `sera-session` | Engine monitors token budget. When threshold is hit: summarize history, clear old tool results, preserve key decisions. Flush-before-discard invariant ensures nothing is lost. |
| **Structured note-taking** | `sera-memory` (write tool), agent scratchpad | Agent writes notes/todos to persistent memory. Engine can inject recent notes into the current turn's context. Notes survive compaction. |
| **Sub-agent condensation** | `sera-harness` (sub-agent dispatch) | Lead agent spawns sub-agents for deep exploration. Sub-agents return condensed results. Lead agent's context stays clean. |

### What "Pluggable" Means in Practice

The `ContextEngine` trait lets different implementations choose different strategy mixes:

| Implementation | Strategy Mix | Use Case |
|---|---|---|
| **Default engine** | System prompt + history + just-in-time tools + compaction | General-purpose agent work |
| **RAG-heavy engine** | Embedding search pre-fills context with relevant docs | Knowledge-base Q&A, support bots |
| **LCM/DAG engine** | Hierarchical summaries with drill-down tools | Long-horizon research, analysis |
| **Minimal engine** | System prompt + current turn only (no history) | Stateless classification, one-shot tasks |

The harness doesn't care which engine is used — it gets a `ContextWindow` and sends it to the model.

### Compaction (Pluggable Strategies)

Compaction is a turn-layer operation with pluggable strategies. It can be triggered automatically or manually, and _how_ compaction works is itself swappable.

**Triggers:**

| Trigger | How | Example |
|---|---|---|
| **Budget limit** | Context engine detects token/turn count nearing configured threshold | `compaction.trigger.max_tokens: 80000` |
| **User command** | User enters a `/compact` or `/reset` command | `/compact` in chat or CLI |
| **Hook / API** | A hook, tool, or external API call requests compaction | Pre-turn hook checks policy and fires compaction |

**The trait:**

```rust
#[async_trait]
pub trait CompactionStrategy: Send + Sync {
    /// Compact the session history into a shorter representation
    async fn compact(
        &self,
        history: &[Turn],
        config: &CompactionConfig,
    ) -> Result<CompactedContext, CompactionError>;

    fn name(&self) -> &str;
}
```

**Built-in strategies:**

```mermaid
graph TB
    classDef strat fill:#8b5cf6,stroke:#7c3aed,color:#fff
    classDef desc fill:#f3f4f6,stroke:#6b7280,color:#000

    S1["Agent-as-Summarizer\n(default)"]:::strat
    S2["Algorithmic\nCleanup"]:::strat
    S3["Hybrid"]:::strat

    D1["Inject a summarization turn —\nthe agent itself reads the full history\nand produces a summary. That summary\nbecomes the seed of the new context.\n\nThis is the most faithful strategy:\nthe agent knows what matters."]:::desc

    D2["Drop old tool call results.\nKeep last N turns verbatim.\nNo model call needed — fast\nand cheap but lossy."]:::desc

    D3["Algorithmic cleanup first\n(clear tool results, trim)\nthen agent summarizes\nwhat remains. Best of both."]:::desc

    S1 --- D1
    S2 --- D2
    S3 --- D3
```

**Agent-as-Summarizer flow:**

```mermaid
graph LR
    classDef trigger fill:#f59e0b,stroke:#d97706,color:#fff
    classDef step fill:#8b5cf6,stroke:#7c3aed,color:#fff
    classDef result fill:#10b981,stroke:#059669,color:#fff

    T["Compaction\ntriggered"]:::trigger
    FLUSH["Flush session\nto durable store"]:::step
    INJECT["Inject summary turn:\n'Summarize this conversation.\nPreserve decisions, open items,\nand key context.'"]:::step
    AGENT["Agent produces\nsummary"]:::step
    REBUILD["Rebuild context:\nsystem prompt +\ntool defs +\nsummary +\ncurrent turn"]:::result

    T --> FLUSH --> INJECT --> AGENT --> REBUILD
```

The flush-before-discard invariant ensures the full session transcript is persisted before any context is discarded — compaction is lossy by design, but nothing is truly lost.

---

## 5. The Hook System — Chainable WASM Pipelines

Hooks are **sandboxed WASM modules** that form ordered pipeline chains. One hook's output feeds into the next. Hooks can execute synchronous data mutations, asynchronous deferrals, or short-circuit routing entirely with `Reject` or `Redirect`. 

For security, hooks are restricted from making arbitrary raw network connections (`no host FS/net`). If a hook needs to query an external component (e.g., verifying a token against an external identity provider or spam-check service), it calls a designated Host API. The sera-gateway then acts as a **proxy**, executing the HTTP request against an approved allow-list. This guarantees hooks are strictly bounded extension points but remain fully capable.

```mermaid
graph LR
    classDef event fill:#4a9eff,stroke:#2b7de9,color:#fff
    classDef hook fill:#8b5cf6,stroke:#7c3aed,color:#fff
    classDef result fill:#10b981,stroke:#059669,color:#fff
    classDef reject fill:#ef4444,stroke:#dc2626,color:#fff

    IN["Input\nContext"]:::event
    H1["Hook 1\ncontent-filter\n(config: patterns, action)"]:::hook
    H2["Hook 2\nrate-limiter\n(config: rpm, burst, scope)"]:::hook
    H3["Hook 3\nsecret-injector\n(config: provider, mappings)"]:::hook
    OUT["Output\nContext"]:::result
    REJ["❌ Rejected\n(short-circuit)"]:::reject

    IN --> H1
    H1 -->|"Continue(ctx)"| H2
    H1 -->|"Reject"| REJ
    H2 -->|"Continue(ctx)"| H3
    H2 -->|"Reject"| REJ
    H3 -->|"Continue(ctx)"| OUT
```

### Hook Points Across the System

```mermaid
graph TB
    classDef gw fill:#10b981,stroke:#059669,color:#fff
    classDef rt fill:#8b5cf6,stroke:#7c3aed,color:#fff
    classDef mem fill:#ec4899,stroke:#db2777,color:#fff
    classDef sess fill:#f59e0b,stroke:#d97706,color:#fff

    subgraph "Edge Phase"
        direction TB
        HP0["on_webhook_ingress\n(custom callback endpoints)"]:::gw
    end

    subgraph "Gateway Phase"
        direction TB
        HP1["pre_route"]:::gw
        HP2["post_route"]:::gw
    end

    subgraph "Harness Phase"
        direction TB
        HP3["pre_turn"]:::rt
        HP4["pre_context"]:::rt
        HP5["post_context"]:::rt
        HP6["pre_tool · post_tool"]:::rt
        HP7["post_turn"]:::rt
        HP8["pre_deliver · post_deliver"]:::rt
    end

    subgraph "Memory Phase"
        direction TB
        HP11["pre_memory_write"]:::mem
    end

    subgraph "Lifecycle"
        direction TB
        HP12["on_session_transition"]:::sess
        HP13["on_approval_request"]:::sess
        HP14["on_workflow_trigger"]:::sess
    end

    HP0 --> HP1 --> HP2 --> HP3 --> HP4 --> HP5 --> HP6 --> HP7 --> HP8
    HP7 --> HP11
    HP12 ~~~ HP13 ~~~ HP14
```

---

## 6. Memory System — Pluggable & Tiered

Memory is a **pluggable workflow**, not a monolithic store. Different agents can use different backends. The default is file-based (Karpathy llm-wiki pattern), with optional auto-git.

### Integration Points

Following Anthropic's context engineering principles, memory is **not** automatically stuffed into the context window by the engine. Instead, it integrates at two specific layers:

1. **Tool Layer (Just-in-Time Retrieval):** The agent is equipped with memory tools (`memory_search`, `memory_write`, `memory_recall`). During its turn, the agent decides _when_ to search its memory and _what_ to retrieve. This enables progressive disclosure and preserves the context budget.
2. **Workflow Layer (Background Processing):** Memory is maintained by asynchronous, triggered workflows outside the critical turn loop (e.g., Compaction, Dreaming, Knowledge Audits).

```mermaid
graph TB
    classDef trait fill:#8b5cf6,stroke:#7c3aed,color:#fff
    classDef backend fill:#10b981,stroke:#059669,color:#fff
    classDef tool fill:#f59e0b,stroke:#d97706,color:#fff
    classDef hook fill:#6b7280,stroke:#4b5563,color:#888

    subgraph "Tool Layer (Agent-Driven)"
        WRITE_REQ["memory_write()"]:::tool
        SEARCH_REQ["memory_search()"]:::tool
    end

    subgraph "Memory Trait (sera-memory)"
        TRAIT["MemoryBackend trait\nwrite · search · get · compact · stats"]:::trait
    end

    subgraph "Backends (pluggable per-agent)"
        FILE["📁 File-Based (default)\nMarkdown + index.md + log.md\n+ optional auto-git"]:::backend
        LCM["🌳 LCM / DAG\nLossless context\nhierarchical summaries"]:::backend
        DB["🗄 PostgreSQL\nStructured store\nSQL queries + audit"]:::backend
        CUSTOM["🔌 Custom\nImplement MemoryBackend"]:::backend
    end

    PRE_HOOK["⛓ pre_memory_write\n(PII filter · classification)"]:::hook
    POST_HOOK["⛓ post_memory_write\n(index update · cross-ref)"]:::hook

    WRITE_REQ --> PRE_HOOK --> TRAIT
    SEARCH_REQ --> TRAIT
    TRAIT --> FILE & LCM & DB & CUSTOM
    TRAIT --> POST_HOOK
```

### Dreaming — Background Memory Consolidation

Dreaming is a **built-in triggered workflow** that runs on a cron schedule (default: 3 AM). It consolidates short-term signals into durable long-term knowledge through three phases.

```mermaid
graph LR
    classDef trigger fill:#6b7280,stroke:#4b5563,color:#fff
    classDef light fill:#93c5fd,stroke:#3b82f6,color:#000
    classDef rem fill:#c4b5fd,stroke:#8b5cf6,color:#000
    classDef deep fill:#1e1b4b,stroke:#4c1d95,color:#fff
    classDef output fill:#10b981,stroke:#059669,color:#fff

    CRON["⏰ Cron Trigger"]:::trigger

    LIGHT["Phase 1: Light Sleep\n─────────────────\n• Ingest daily notes + transcripts\n• Deduplicate\n• Stage candidates\n• Record signal hits"]:::light

    REM["Phase 2: REM Sleep\n─────────────────\n• Extract recurring themes\n• Identify candidate truths\n• Record reinforcement signals"]:::rem

    DEEP["Phase 3: Deep Sleep\n─────────────────\n• Score candidates (6 signals)\n• Apply threshold gates\n  minScore ≥ 0.8\n  minRecallCount ≥ 3\n  minUniqueQueries ≥ 3\n• Promote survivors"]:::deep

    RESULT["📓 Dream Diary\n(narrative) +\n🧠 MEMORY.md\n(promoted knowledge)"]:::output

    CRON --> LIGHT --> REM --> DEEP --> RESULT
```

---

## 7. Identity & Authorization

**Principals**, not just users. Any acting entity — human, agent, service, external agent — is a Principal with identity, credentials, and authorization.

```mermaid
graph TB
    classDef principal fill:#4a9eff,stroke:#2b7de9,color:#fff
    classDef layer fill:#10b981,stroke:#059669,color:#fff
    classDef enterprise fill:#f59e0b,stroke:#d97706,color:#fff

    subgraph "Principals (any acting entity)"
        HUMAN["👤 Human\n(local · OIDC · SCIM)"]:::principal
        AGENT["🤖 Agent\n(registered · own credentials)"]:::principal
        EXT_AGENT["🌐 External Agent\n(A2A · ACP identity)"]:::principal
        SERVICE["⚙️ Service\n(CI/CD · monitoring)"]:::principal
    end

    subgraph "Authentication Layer"
        AUTHN["AuthN\nJWT · API Keys · Basic Auth\nOAuthv2 · OIDC · SCIM"]:::layer
    end

    subgraph "Authorization Layer (WASM Hooks)"
        AUTHZ["AuthZ Hooks\n(Standard Webhook Plugins)\nCalls external PDP / OPA sidecar"]:::layer
    end

    subgraph "Continuous Security (Enterprise)"
        SSF["Shared Signals Framework\nCAEP (session revocation)\nRISC (credential compromise)"]:::enterprise
    end

    HUMAN & AGENT & EXT_AGENT & SERVICE --> AUTHN --> AUTHZ
    AUTHZ --> SSF

    AUTHZ -->|"Allow"| ALLOW["✅ Execute"]
    AUTHZ -->|"Deny"| DENY["❌ Reject"]
    AUTHZ -->|"NeedsApproval"| HITL["⏸ HITL Gate"]
```

### HITL Approval Escalation

```mermaid
graph TD
    classDef trigger fill:#f59e0b,stroke:#d97706,color:#fff
    classDef assess fill:#8b5cf6,stroke:#7c3aed,color:#fff
    classDef agent fill:#10b981,stroke:#059669,color:#fff
    classDef human fill:#4a9eff,stroke:#2b7de9,color:#fff
    classDef result fill:#6b7280,stroke:#4b5563,color:#fff

    ACTION["Action triggers\napproval check"]:::trigger
    RISK["Dynamic risk\nassessment"]:::assess
    POLICY["Resolve approval\npolicy"]:::assess

    AUTO["Autonomous\n(execute immediately)"]:::result
    AGENT_R["Route to\nreviewer agent"]:::agent
    HUMAN_R["Route to\nhuman principal"]:::human

    APPROVE["✅ Approved\n→ Execute"]:::result
    REJECT["❌ Rejected"]:::result
    ESCALATE["⬆ Escalate to\nnext target"]:::trigger

    ACTION --> RISK --> POLICY
    POLICY -->|"low risk"| AUTO
    POLICY -->|"agent review"| AGENT_R
    POLICY -->|"human approval"| HUMAN_R

    AGENT_R -->|"approved"| APPROVE
    AGENT_R -->|"rejected"| REJECT
    AGENT_R -->|"uncertain"| ESCALATE

    HUMAN_R -->|"approved"| APPROVE
    HUMAN_R -->|"rejected"| REJECT
    HUMAN_R -->|"timeout"| ESCALATE

    ESCALATE --> POLICY
```

---

## 8. Interoperability — Protocol Integration

SERA is protocol-native. It speaks the emerging agent ecosystem standards so agents can participate in multi-agent networks beyond the SERA boundary.

```mermaid
graph LR
    classDef sera fill:#10b981,stroke:#059669,color:#fff
    classDef proto fill:#8b5cf6,stroke:#7c3aed,color:#fff
    classDef ext fill:#4a9eff,stroke:#2b7de9,color:#fff
    classDef client fill:#f59e0b,stroke:#d97706,color:#fff

    subgraph "External Ecosystem"
        MCP_S["MCP Servers\n(tools & resources)"]:::ext
        A2A_A["A2A Agents\n(Google federated)"]:::ext
        ACP_A["ACP Agents\n(structured messaging)"]:::ext
    end

    subgraph "SERA Protocol Adapters"
        MCP_B["sera-mcp\n(server + client bridge)"]:::proto
        A2A_B["sera-a2a\n(adapter)"]:::proto
        ACP_B["sera-acp\n(adapter)"]:::proto
        AGUI_B["sera-agui\n(AG-UI stream)"]:::proto
    end

    GW["SERA Gateway"]:::sera

    subgraph "Frontends"
        WEB["sera-web\n(AG-UI full)"]:::client
        CLI["sera-cli\n(gRPC/WS)"]:::client
        HMI["Thin Clients\n(AG-UI minimal SSE)"]:::client
    end

    MCP_S <-->|"MCP"| MCP_B
    A2A_A <-->|"A2A"| A2A_B
    ACP_A <-->|"ACP"| ACP_B

    MCP_B & A2A_B & ACP_B <--> GW

    GW --> AGUI_B
    AGUI_B -->|"full stream"| WEB
    AGUI_B -->|"minimal stream"| HMI
    GW -->|"gRPC/WS"| CLI
```

---

## 9. Crate Dependency Graph

The Rust workspace is decomposed into layers with clear dependency flow. Foundation crates at the bottom, the gateway binary at the top.

```mermaid
graph BT
    classDef found fill:#6b7280,stroke:#4b5563,color:#fff
    classDef infra fill:#4a9eff,stroke:#2b7de9,color:#fff
    classDef domain fill:#10b981,stroke:#059669,color:#fff
    classDef interop fill:#8b5cf6,stroke:#7c3aed,color:#fff
    classDef harness fill:#f59e0b,stroke:#d97706,color:#fff
    classDef gateway fill:#ef4444,stroke:#dc2626,color:#fff
    classDef client fill:#ec4899,stroke:#db2777,color:#fff

    subgraph "Foundation"
        TYPES["sera-types"]:::found
        CONFIG["sera-config"]:::found
        ERRORS["sera-errors"]:::found
    end

    subgraph "Infrastructure"
        DB["sera-db"]:::infra
        QUEUE["sera-queue"]:::infra
        CACHE["sera-cache"]:::infra
        OTEL["sera-telemetry"]:::infra
        SECRETS["sera-secrets"]:::infra
    end

    subgraph "Core Domain"
        SESSION["sera-session"]:::domain
        MEMORY["sera-memory"]:::domain
        TOOLS["sera-tools"]:::domain
        HOOKS["sera-hooks"]:::domain
        AUTH["sera-auth"]:::domain
        MODELS["sera-models"]:::domain
        SKILLS["sera-skills"]:::domain
        HITL["sera-hitl"]:::domain
        WORKFLOW["sera-workflow"]:::domain
    end

    subgraph "Interop"
        MCP["sera-mcp"]:::interop
        A2A["sera-a2a"]:::interop
        ACP["sera-acp"]:::interop
        AGUI["sera-agui"]:::interop
    end

    HN["sera-harness"]:::harness
    GW["sera-gateway"]:::gateway

    subgraph "Clients"
        CLI["sera-cli"]:::client
        TUI["sera-tui"]:::client
        SDK["sera-sdk"]:::client
    end

    TYPES --> CONFIG & ERRORS
    CONFIG & ERRORS --> DB & QUEUE & CACHE & OTEL & SECRETS
    DB --> SESSION & MEMORY
    QUEUE --> SESSION
    SECRETS --> AUTH & TOOLS
    SESSION & MEMORY & TOOLS & HOOKS & AUTH & MODELS & SKILLS & HITL & WORKFLOW --> HN
    MCP & A2A & ACP & AGUI --> GW
    HN --> GW
    OTEL --> GW
    GW --> CLI & TUI & SDK
```

---

## 10. Deployment Spectrum

SERA scales from a single binary on a laptop to a multi-node enterprise cluster. Same codebase, different config.

```mermaid
graph LR
    classDef t1 fill:#10b981,stroke:#059669,color:#fff
    classDef t2 fill:#f59e0b,stroke:#d97706,color:#fff
    classDef t3 fill:#ef4444,stroke:#dc2626,color:#fff

    T1["🏠 Tier 1: Local Dev\n───────────────\n• Single binary\n• SQLite + file memory\n• In-memory queue\n• No auth (autonomous)\n• Env secrets\n• sera start"]:::t1

    T2["👥 Tier 2: Team\n───────────────\n• Single node\n• PostgreSQL\n• Redis cache\n• JWT auth\n• File secrets\n• Docker Compose"]:::t2

    T3["🏢 Tier 3: Enterprise\n───────────────\n• Multi-node cluster\n• PostgreSQL HA\n• Redis Cluster\n• OIDC + AuthZen + SSF\n• Vault / Cloud SM\n• K8s / Nomad"]:::t3

    T1 -->|"scale up"| T2 -->|"scale out"| T3
```

---

## 11. Security Trust Boundaries

Three trust zones with clear boundaries. The WASM sandbox lives _inside_ the trusted core but is fuel-metered and memory-capped.

```mermaid
graph TB
    classDef trusted fill:#10b981,stroke:#059669,color:#fff
    classDef wasm fill:#8b5cf6,stroke:#7c3aed,color:#fff
    classDef isolated fill:#f59e0b,stroke:#d97706,color:#fff
    classDef untrusted fill:#ef4444,stroke:#dc2626,color:#fff
    classDef boundary fill:none,stroke:#000,stroke-dasharray:5

    subgraph CORE["Trusted Core (sera-gateway process)"]
        AUTH["sera-auth\n(Principal Registry + AuthZ)"]:::trusted
        SESSION["sera-session\n(state machine)"]:::trusted
        SECRET["sera-secrets\n(secret manager)"]:::trusted

        subgraph WASM_BOX["WASM Sandbox (sera-hooks)"]
            HOOKS["Hook Chains\n• fuel metered\n• memory capped\n• no host FS/net\n• config-driven"]:::wasm
        end
    end

    subgraph ADAPTERS["Isolated Harnesses & Adapters"]
        HN["sera-harness\n(context pipeline)"]:::isolated
        CONN["Connectors\n(Discord, Slack, ...)"]:::isolated
        EXT_TOOL["Ext Tools"]:::isolated
        EXT_HN["Ext Harnesses"]:::isolated
    end

    subgraph CLIENTS["Untrusted Clients (all principals)"]
        CLI["CLI"]:::untrusted
        TUI["TUI"]:::untrusted
        WEB["Web"]:::untrusted
        SDK_C["SDK"]:::untrusted
        HMI_C["HMI"]:::untrusted
    end

    CORE ---|"gRPC boundary"| ADAPTERS
    ADAPTERS ---|"client boundary"| CLIENTS
```

---

## 12. End-to-End Request Flow

A complete request lifecycle — from client message to delivered response.

```mermaid
sequenceDiagram
    autonumber
    participant C as Client
    participant GW as Gateway
    participant Q as Queue
    participant S as Session
    participant HN as Harness
    participant CTX as Context Assembly
    participant LLM as Model
    participant T as Tools
    participant M as Memory
    participant H as Hooks

    C->>GW: Send message (WS/gRPC)
    GW->>GW: Dedupe / debounce
    GW->>H: ⛓ pre_route chain
    H-->>GW: Continue(ctx) or Reject
    GW->>Q: Enqueue (session lane)
    Q->>S: Dequeue (single-writer)
    S->>HN: Dispatch turn
    HN->>H: ⛓ pre_turn chain
    HN->>CTX: Assemble context (KV-cache order)
    CTX->>M: Inject memories (tiered)
    CTX-->>HN: Assembled context window
    HN->>LLM: Model call
    LLM-->>HN: tool_call(name, args)
    HN->>H: ⛓ pre_tool chain
    HN->>T: Execute tool
    T-->>HN: Tool result
    HN->>H: ⛓ post_tool chain
    HN->>LLM: Continue with result
    LLM-->>HN: Final response
    HN->>H: ⛓ post_turn chain
    HN->>M: Memory write pipeline
    HN->>H: ⛓ pre_deliver chain
    HN->>GW: Deliver response
    GW->>C: Stream response (AG-UI / WS)
    GW->>H: ⛓ post_deliver chain
```

---

## 13. Multi-Agent Coordination — Circles

Circles organize agents in a DAG hierarchy (like an org chart). Each circle has a coordination policy.

```mermaid
graph TD
    classDef org fill:#6b7280,stroke:#4b5563,color:#fff
    classDef circle fill:#4a9eff,stroke:#2b7de9,color:#fff
    classDef agent fill:#10b981,stroke:#059669,color:#fff

    ORG["🏢 Organization"]:::org

    ENG["Engineering Circle\n(Supervised)"]:::circle
    OPS["Operations Circle\n(Parallel)"]:::circle

    FRONT["Frontend Circle\n(Sequential)"]:::circle
    BACK["Backend Circle\n(Supervised)"]:::circle
    MON["Monitoring Circle"]:::circle

    A1["🤖 UI-Designer\n(Lead)"]:::agent
    A2["🤖 Code-Writer\n(Worker)"]:::agent
    A3["🤖 Architect\n(Lead)"]:::agent
    A4["🤖 Implementer\n(Worker)"]:::agent
    A5["🤖 Code-Reviewer\n(Reviewer)"]:::agent
    A6["🤖 SRE-Bot\n(Lead)"]:::agent

    ORG --> ENG & OPS
    ENG --> FRONT & BACK
    OPS --> MON

    FRONT --> A1 & A2
    BACK --> A3 & A4 & A5
    MON --> A6
```

---

## Legend

| Symbol | Meaning |
|---|---|
| ⛓ | Hook chain (WASM pipeline) — fires at this point |
| → (solid) | Data / control flow |
| ⇢ (dashed) | Hook invocation (side-effect) |
| 🟢 | Stable (high KV cache reuse) |
| 🟡 | Semi-stable |
| 🔴 | Volatile (changes every turn) |
