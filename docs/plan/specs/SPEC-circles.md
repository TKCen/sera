# SPEC: Circles — Multi-Agent Coordination (`sera-circles`)

> **Status:** DRAFT  
> **Source:** PRD §11.2  
> **Crate:** Part of `sera-types` (model) + `sera-runtime` (coordination)  
> **Priority:** Phase 4  

---

## 1. Overview

Circles are SERA's model for **multi-agent coordination**. They organize agents into a **DAG** (Directed Acyclic Graph) — like an org structure. A Circle can contain agents and other Circles, enabling hierarchical coordination patterns.

Circles are about **coordination** (how agents work together), not authorization (who can do what). PrincipalGroups handle authorization boundaries. The relationship between the two concepts needs further refinement (see Open Questions).

---

## 2. Circle Model

```rust
pub struct Circle {
    pub id: CircleId,
    pub name: String,
    pub members: Vec<CircleMember>,
    pub sub_circles: Vec<CircleId>,     // DAG: circles can contain circles
    pub parent: Option<CircleId>,       // Parent circle in the DAG
    pub coordination: CoordinationPolicy,
    pub goal: Option<String>,
}

pub struct CircleMember {
    pub principal: PrincipalRef,        // Can be an agent or a human principal
    pub role: CircleRole,
    pub can_delegate: bool,
}

pub enum CircleRole {
    Lead,       // Coordinates work, reviews output
    Worker,     // Executes tasks
    Reviewer,   // Reviews work products
    Observer,   // Watch-only
}
```

---

## 3. Coordination Policies

```rust
pub enum CoordinationPolicy {
    Sequential,       // Members execute in order
    Parallel,         // Members execute concurrently
    Supervised,       // Lead reviews before work is finalized
    Consensus,        // Members vote on decisions
    Custom(String),   // Custom policy (resolved by hook)
}
```

| Policy | Behavior |
|---|---|
| **Sequential** | Tasks flow from one member to the next in order. Output of member N is input to member N+1. |
| **Parallel** | All workers receive the task simultaneously. Results are collected and merged. |
| **Supervised** | Workers execute; Lead reviews and approves before finalization. |
| **Consensus** | Members each produce a response; a voting/consensus mechanism selects the final output. |
| **Custom** | A hook resolves the coordination logic. |

---

## 4. DAG Structure Example

```
Organization Circle
├── Engineering Circle
│   ├── Frontend Circle
│   │   ├── Agent: UI-Designer (Lead)
│   │   └── Agent: Code-Writer (Worker)
│   └── Backend Circle
│       ├── Agent: Architect (Lead)
│       ├── Agent: Implementer (Worker)
│       └── Agent: Code-Reviewer (Reviewer)
└── Operations Circle
    └── Monitoring Circle
        └── Agent: SRE-Bot (Lead)
```

Circles form a DAG — a circle can have at most one parent, and cycles are not permitted.

---

## 5. Task Delegation

When a Circle receives a task:

1. The Circle's coordination policy determines how the task is distributed
2. The Lead (if Supervised) orchestrates the work
3. Workers execute their portions
4. Results are collected, reviewed, and merged
5. The final output is returned to the caller

Task delegation across circles follows the DAG structure — a parent circle can delegate to sub-circles.

---

## 5a. Inter-Agent Communication Channels (Deferred: Phase 4+)

> **Enhancement: Strategic Rearchitecture §Channel Partitioning**

Beyond structured Circle coordination, SERA supports **internal topic channels** for loose, event-driven inter-agent communication. Channels allow agents to publish to and subscribe to domain-specific topics without formal Circle membership.

### Channel Model

```rust
pub struct AgentChannel {
    pub id: ChannelId,
    pub name: String,                     // e.g., "security", "frontend", "architecture"
    pub subscribers: Vec<PrincipalRef>,   // Agents and/or humans
    pub access_policy: ChannelAccessPolicy,
}

pub enum ChannelAccessPolicy {
    Open,                                 // Any principal can subscribe
    InviteOnly(Vec<PrincipalRef>),        // Only listed principals can subscribe
    CircleScoped(CircleId),               // Only members of a specific circle
}
```

### Semantics

- Channels are **internal event topics**, not external connectors.
- Messages published to a channel are **events** that enter the gateway's normal event pipeline — subject to hooks, authorization, and session management.
- Channels enforce **context isolation**: agents only see messages in their subscribed channels, preventing context pollution from unrelated domain traffic.
- Channel messages are **compacted**: only the final, verified output of a thread/conversation is surfaced to subscribers, not the full back-and-forth.

### Relationship to Circles

- **Circles** are for **structured coordination** (task delegation, review loops, consensus).
- **Channels** are for **loose communication** (broadcasting results, sharing discoveries, status updates).
- A Circle may have an associated channel (e.g., the "engineering" circle auto-creates a `#engineering` channel).

### Use Cases

| Channel | Purpose |
|---|---|
| `#security` | Security agent publishes vulnerability findings, all subscribing agents receive them |
| `#architecture` | Architect agent publishes design decisions, implementer agents consume them |
| `#notifications` | System events (build results, deployment status) broadcast to interested agents |
| `#debug` | Agents publish diagnostic information during complex multi-agent workflows |

> [!NOTE]
> This is a Phase 4+ feature. The current Circle model with coordination policies covers structured multi-agent work. Channels add an additional loose-coupling mechanism for organic agent-to-agent communication.

---

## 6. Configuration

```yaml
sera:
  circles:
    - name: "engineering"
      goal: "Build and maintain the product"
      coordination: "supervised"
      members:
        - principal: "agent:architect"
          role: "lead"
          can_delegate: true
        - principal: "agent:implementer"
          role: "worker"
        - principal: "agent:reviewer"
          role: "reviewer"
      sub_circles:
        - "frontend"
        - "backend"
```

---

## 7. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | PrincipalGroups for authorization; Circles for coordination |
| `sera-runtime` | [SPEC-runtime](SPEC-runtime.md) | Subagent management and task delegation |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Custom coordination policies via hooks |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Circle leads can be approval targets |
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Channel events enter the gateway pipeline |

---

## 8. Open Questions

1. **Circles vs. PrincipalGroups overlap** — When a Circle also needs authorization boundaries, should Circles automatically create PrincipalGroups, or remain purely coordination? (PRD §19)
2. **Circle task protocol** — How does task delegation work at the protocol level? Is it a special event type? A tool call?
3. **Circle session management** — Do Circle tasks run in shared sessions or isolated per-member sessions?
4. **Result merging** — How are results from parallel/consensus policies merged? LLM-driven merge? Structured merge?
5. **Circle lifecycle** — Can circles be created/modified at runtime, or are they config-only?
6. **Human members** — The model allows human principals as circle members. What does this look like in practice? Humans receive tasks via approval-like prompts?
7. **Channel persistence** — Are channel messages persisted? For how long? Are they searchable via memory?
8. **Channel auto-creation** — Should circles automatically create associated channels, or are channels always manually configured?
