# Research: Subagent Skill Delegation and A2A Communication

> **Issue:** sera-0ct  
> **Status:** DRAFT  
> **Date:** 2026-04-16  
> **Related:** sera-z2t (AgentSkills spec), GH#268, GH#621, GH#154

---

## Executive Summary

SERA's current delegation model (`DelegationOrchestrator` + `SubagentManager`) delegates **prompts** to target agents identified by ID. This research proposes extending delegation to be **skill-aware** (delegate a specific skill, not just a prompt), adding **callback-on-completion** (replacing status polling), **streaming results**, and **structured A2A messaging** across internal, external, and circle-level communication channels.

---

## Part 1: Internal Delegation

### 1.1 Problem Statement

Today's `DelegationOrchestrator` (in `sera-runtime::delegation`) delegates work by agent ID + free-form JSON context. The caller has no way to:

1. **Request a specific skill** -- delegation targets an agent, not a capability. If two agents both offer "code-review", the caller cannot express which skill it needs without knowing which agent has it.
2. **Receive results via callback** -- the orchestrator polls `SubagentStatus` via a `watch::Receiver` in a 100ms poll loop. This wastes CPU and adds latency for short tasks.
3. **Stream partial results** -- the `DelegationResponse::Success` variant carries a single `serde_json::Value`. Long-running skills (code generation, research) cannot send incremental output.
4. **Share a collaborative session** -- each delegation creates an isolated session. Two agents working on related sub-tasks cannot share transcript context.

### 1.2 Proposed Design

#### 1.2.1 Skill-Based Delegation Request

Extend `DelegationRequest` with an optional skill specifier. When present, the orchestrator resolves the best agent for that skill rather than requiring a hardcoded `target_agent_id`.

```rust
/// Specifies which skill to delegate, with optional version constraint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTarget {
    /// Skill name (e.g., "code-review", "shell-exec").
    pub skill_name: String,
    /// Optional semver constraint (e.g., ">=1.0.0").
    pub version_constraint: Option<String>,
    /// Skill-specific parameters (merged with context).
    pub parameters: serde_json::Value,
}

/// Extended delegation request supporting both agent-targeted and skill-targeted delegation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationRequest {
    // -- existing fields --
    pub target_agent_id: String,
    pub task_description: String,
    pub context: serde_json::Value,
    pub input_filter: Option<HandoffInputFilter>,
    pub timeout: Option<Duration>,
    pub depth: u32,

    // -- new fields --
    /// When set, the orchestrator resolves the best agent for this skill.
    /// `target_agent_id` becomes a hint/preference rather than a requirement.
    pub skill_target: Option<SkillTarget>,
    /// When true, the orchestrator uses callback delivery instead of polling.
    pub callback: bool,
    /// When true, the orchestrator opens a streaming channel for partial results.
    pub streaming: bool,
    /// Parent session key for collaborative delegation (shared transcript).
    pub parent_session_key: Option<String>,
}
```

#### 1.2.2 Skill Resolution

A new `SkillRouter` trait resolves skill names to available agents, using the `SkillRegistry` from `sera-types::skill` and the agent manifest registry.

```rust
/// Resolves a skill target to one or more candidate agents.
#[async_trait]
pub trait SkillRouter: Send + Sync {
    /// Find agents that can fulfill the given skill target, ranked by fitness.
    async fn resolve(
        &self,
        target: &SkillTarget,
    ) -> Result<Vec<SkillCandidate>, DelegationError>;
}

/// A candidate agent that can fulfill a skill request.
#[derive(Debug, Clone)]
pub struct SkillCandidate {
    pub agent_id: String,
    /// The agent's advertised skill definition.
    pub skill: SkillDefinition,
    /// Fitness score (0.0-1.0) based on version match, load, affinity.
    pub fitness: f64,
}
```

**Resolution strategy:**
1. Query `SkillRegistry` for all agents advertising the skill name.
2. Filter by version constraint (if provided).
3. Rank by: version match closeness, current load, affinity to caller's circle, and historical success rate.
4. If `target_agent_id` is also set, boost that agent's fitness score (preference hint).

#### 1.2.3 Callback-on-Completion

Replace the 100ms poll loop with a `tokio::sync::oneshot` for fire-and-forget delegation, or a `tokio::sync::mpsc` for streaming.

```rust
/// Handle returned by skill-based delegation.
pub struct DelegationHandle {
    /// Session key for this delegation.
    pub session_key: String,
    /// Resolves when the delegation completes (replaces poll loop).
    pub completion: oneshot::Receiver<DelegationResponse>,
    /// Cancel the delegation.
    pub cancel: oneshot::Sender<()>,
}

/// Handle for streaming delegation (partial results).
pub struct StreamingDelegationHandle {
    pub session_key: String,
    /// Stream of partial results as they arrive.
    pub stream: mpsc::Receiver<DelegationEvent>,
    pub cancel: oneshot::Sender<()>,
}

/// Events emitted during a streaming delegation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DelegationEvent {
    /// Partial text output from the delegated agent.
    Chunk { text: String },
    /// A tool was invoked by the delegated agent.
    ToolUse { tool_name: String, input: serde_json::Value },
    /// A tool call completed.
    ToolResult { tool_name: String, output: serde_json::Value },
    /// The delegated agent requests input from the caller.
    InputRequired { prompt: String },
    /// Final result.
    Completed { output: serde_json::Value },
    /// The delegation failed.
    Failed { error: String },
}
```

**Integration with existing `SubagentManager`:**

The `SubagentManager::spawn` return type (`SubagentHandle`) already uses `watch::Receiver<SubagentStatus>`. The callback model wraps this internally -- the `DelegationOrchestrator` spawns a background task that awaits the watch channel and resolves the oneshot when a terminal status is reached, eliminating the caller-side poll loop.

#### 1.2.4 Collaborative Sessions

When `parent_session_key` is set, the delegated agent shares the parent's `SessionStateMachine` transcript rather than creating an isolated session. This enables:

- Shared context: the sub-agent can read prior conversation turns.
- Transcript continuity: the sub-agent's output appears inline in the parent transcript.
- Memory sharing: both agents access the same session-scoped `ShortTerm` memory.

```rust
/// Collaborative session configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborativeSessionConfig {
    /// Parent session key to join.
    pub parent_session_key: String,
    /// Access level for the child agent in the shared session.
    pub access: CollaborativeAccess,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborativeAccess {
    /// Read parent transcript, write own turns (default).
    ReadWrite,
    /// Read-only access to parent transcript.
    ReadOnly,
    /// Full access including ability to modify parent state.
    Full,
}
```

**State machine implications:** The parent session transitions to `Active` while the child is running. The child gets its own `SessionStateMachine` that is **linked** to the parent -- the child's `Closed` state triggers a notification to the parent, but does not close the parent.

### 1.3 Integration Points

| Crate | Integration |
|---|---|
| `sera-runtime::delegation` | Extend `DelegationOrchestrator` with `SkillRouter`, callback handles |
| `sera-runtime::subagent` | Add `SubagentManager::spawn_with_callback` variant |
| `sera-skills` | `SkillPack::list_skills` feeds the `SkillRouter` resolution |
| `sera-types::skill` | `SkillRegistry` provides the lookup index |
| `sera-session::state` | Add linked session support for collaborative access |
| `sera-runtime::handoff` | `DelegationRequest` gains new fields |

### 1.4 Open Questions

1. **Skill version negotiation:** Should the orchestrator reject requests when no agent matches the version constraint, or fall back to the closest available version?
2. **Collaborative session isolation:** How do we prevent a misbehaving child agent from corrupting the parent transcript? Should writes go through a review gate?
3. **Streaming backpressure:** If the caller is slower than the delegated agent, should we buffer or drop chunks? What is the buffer size?
4. **Fitness scoring weights:** Should fitness weights be configurable per-circle, or global?

---

## Part 2: A2A Channels (Internal Messaging)

### 2.1 Problem Statement

SERA agents currently communicate only through delegation (fire-and-forget task assignment) or Centrifugo pub/sub (`IntercomMessage`). There is no structured **conversational** messaging between agents -- no way for agent A to send a typed message to agent B and receive a typed response within the same logical conversation.

GH#621 requests session spawn/yield/send primitives. The current `SessionStateMachine` has no concept of inter-session message passing.

### 2.2 Proposed Design

#### 2.2.1 Structured Message Types

Define a typed message envelope for all internal agent-to-agent communication, distinct from the untyped `IntercomMessage`.

```rust
/// Unique message identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub String);

impl MessageId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

/// Structured agent-to-agent message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: MessageId,
    pub from: String,       // sender agent_id
    pub to: String,         // recipient agent_id
    pub kind: MessageKind,
    pub payload: serde_json::Value,
    pub correlation_id: Option<MessageId>,  // links response to request
    pub session_key: Option<String>,        // scopes to a session
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Discriminated message types for agent communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// A request expecting a response (correlated by `correlation_id`).
    Request,
    /// A response to a prior request.
    Response,
    /// A one-way notification (no response expected).
    Notification,
    /// A handoff -- transfers ownership of a task/session to the recipient.
    Handoff,
    /// A yield -- temporarily suspends the sender and passes control.
    Yield,
}
```

#### 2.2.2 Agent-Message Skill

Expose agent-to-agent messaging as a **skill** (tool) available to agents, rather than requiring agents to use raw channel APIs.

```rust
/// The "agent-message" skill definition, registered in every agent's SkillRegistry.
/// Agents invoke this skill to send structured messages to other agents.
///
/// Tool schema:
/// {
///   "name": "agent_message",
///   "parameters": {
///     "to": "string (agent_id)",
///     "kind": "request | notification | handoff | yield",
///     "payload": "object",
///     "await_response": "boolean (default: false)"
///   }
/// }
pub struct AgentMessageSkill {
    /// Channel for sending messages to the message router.
    outbox: mpsc::Sender<AgentMessage>,
    /// Channel for receiving responses (when await_response = true).
    inbox: mpsc::Receiver<AgentMessage>,
}

#[async_trait]
impl SkillExecutor for AgentMessageSkill {
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &SkillExecutionContext,
    ) -> Result<serde_json::Value, SkillExecutionError>;
}
```

#### 2.2.3 Session Spawn/Yield/Send (GH#621)

Three new session operations that integrate with `SessionStateMachine`:

```rust
/// Operations for inter-session communication.
#[async_trait]
pub trait SessionChannel: Send + Sync {
    /// Spawn a new child session, optionally linked to the current one.
    /// Returns the child's session key.
    async fn spawn(
        &self,
        parent_key: &str,
        agent_id: &str,
        config: SpawnConfig,
    ) -> Result<String, SessionChannelError>;

    /// Yield control from the current session to another session.
    /// The current session transitions to Suspended; the target session
    /// transitions to Active. Resumes when the target yields back or closes.
    async fn yield_to(
        &self,
        current_key: &str,
        target_key: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, SessionChannelError>;

    /// Send a message to another session without yielding control.
    /// The current session remains Active.
    async fn send(
        &self,
        from_key: &str,
        to_key: &str,
        message: AgentMessage,
    ) -> Result<(), SessionChannelError>;
}

/// Configuration for spawning a child session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnConfig {
    /// Initial task for the child session.
    pub task: String,
    /// Whether the child shares the parent's transcript.
    pub collaborative: bool,
    /// Timeout for the child session.
    pub timeout: Option<Duration>,
}
```

**State machine integration:**

The `SessionStateMachine` gains two new valid transitions:

- `Active -> Suspended` (already exists) -- used by `yield_to`
- `Suspended -> Active` (already exists) -- used when yield returns

The `yield_to` operation:
1. Transitions the caller's session to `Suspended`.
2. Sends the payload to the target session as an `AgentMessage` with `kind: Yield`.
3. Awaits a return message from the target (either a `Response` or the target session closing).
4. Transitions the caller back to `Active` with the response payload.

### 2.3 Integration Points

| Crate | Integration |
|---|---|
| `sera-session` | Add `SessionChannel` trait, linked session tracking |
| `sera-session::state` | No new states needed; existing transitions suffice |
| `sera-types::skill` | Register `agent_message` as a built-in skill |
| `sera-types::intercom` | `AgentMessage` extends (not replaces) `IntercomMessage` |
| `sera-events` | Emit audit events for all agent messages |
| `sera-runtime` | Wire `AgentMessageSkill` into agent tool registry |

### 2.4 Open Questions

1. **Message ordering guarantees:** Should `SessionChannel::send` guarantee ordered delivery, or is best-effort acceptable? Ordered delivery requires a persistent queue; best-effort can use Centrifugo.
2. **Yield timeout:** What happens if the target session never yields back? Should there be a mandatory timeout, or can the caller wait indefinitely?
3. **Deadlock detection:** Two sessions yielding to each other creates a deadlock. Should the runtime detect and break cycles?
4. **Message persistence:** Should `AgentMessage` records be persisted for audit replay, or are they ephemeral?

---

## Part 3: External A2A

### 3.1 Problem Statement

The existing `sera-a2a` crate provides an `A2aAdapter` trait with four methods: `discover`, `send_task`, `get_task`, `cancel_task`. This covers basic task delegation to external A2A agents but lacks:

1. **Federation via BridgeService** -- no mechanism for SERA to act as an A2A bridge between its internal agents and external A2A networks.
2. **External skill invocation** -- external agents are treated as opaque task processors, not skill providers. SERA cannot route a skill request to an external agent.
3. **Streaming** -- `send_task` returns a completed `Task`; there is no streaming variant for long-running external tasks.
4. **Push notifications** -- `get_task` requires polling; no webhook/SSE callback for task completion.

### 3.2 Proposed Design

#### 3.2.1 BridgeService

A `BridgeService` that exposes SERA's internal agents as A2A agents to external callers, and routes external A2A agent capabilities into SERA's internal skill registry.

```rust
/// Bridge between SERA's internal agent network and external A2A networks.
///
/// Inbound: external A2A clients discover and delegate to SERA agents.
/// Outbound: SERA agents discover and delegate to external A2A agents.
#[async_trait]
pub trait A2aBridgeService: Send + Sync {
    // -- Inbound (external -> SERA) --

    /// Publish SERA agent capabilities as A2A AgentCards.
    /// Called on agent registration and skill changes.
    async fn publish_agent_card(
        &self,
        agent_id: &str,
        skills: Vec<AgentSkill>,
    ) -> Result<AgentCard, A2aError>;

    /// Handle an inbound A2A task from an external caller.
    /// Translates to a DelegationRequest and routes to the appropriate agent.
    async fn handle_inbound_task(
        &self,
        task: Task,
        caller_identity: ExternalAgentIdentity,
    ) -> Result<Task, A2aError>;

    // -- Outbound (SERA -> external) --

    /// Register an external A2A agent discovered at `endpoint`.
    /// Its skills are imported into SERA's SkillRegistry as remote skills.
    async fn register_external_agent(
        &self,
        endpoint: &str,
    ) -> Result<ExternalAgentRecord, A2aError>;

    /// Invoke a skill on a registered external A2A agent.
    /// Translates the SkillTarget to an A2A Task and delegates.
    async fn invoke_external_skill(
        &self,
        agent_record: &ExternalAgentRecord,
        skill_id: &str,
        input: serde_json::Value,
    ) -> Result<Task, A2aError>;
}

/// Identity of an external A2A agent for authorization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalAgentIdentity {
    pub agent_url: String,
    pub agent_name: String,
    pub trust_level: TrustLevel,
    pub authentication: Option<AuthenticationInfo>,
}

/// Trust level assigned to external agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// Fully trusted (same organization).
    Trusted,
    /// Verified identity but limited permissions.
    Verified,
    /// Unknown/unverified external agent.
    Untrusted,
}

/// Record of a registered external A2A agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalAgentRecord {
    pub agent_card: AgentCard,
    pub identity: ExternalAgentIdentity,
    pub registered_at: chrono::DateTime<chrono::Utc>,
    pub last_seen: chrono::DateTime<chrono::Utc>,
    /// Skills imported from this agent into SERA's registry.
    pub imported_skills: Vec<String>,
}
```

#### 3.2.2 External Skill Integration with SkillRouter

External A2A agent skills are registered in `SkillRegistry` with a `source` marker of `"a2a:<agent_url>"`. The `SkillRouter` from Part 1 treats them as candidates alongside internal agents:

```rust
impl SkillRouter for UnifiedSkillRouter {
    async fn resolve(&self, target: &SkillTarget) -> Result<Vec<SkillCandidate>, DelegationError> {
        let mut candidates = Vec::new();

        // Internal agents
        for (agent_id, registry) in &self.internal_registries {
            if let Some(config) = registry.get_config(&target.skill_name) {
                candidates.push(SkillCandidate {
                    agent_id: agent_id.clone(),
                    skill: skill_def_from_config(config),
                    fitness: self.score_internal(agent_id, target),
                });
            }
        }

        // External A2A agents
        for record in &self.external_agents {
            for skill in &record.agent_card.skills {
                if skill.name == target.skill_name {
                    candidates.push(SkillCandidate {
                        agent_id: format!("a2a:{}", record.agent_card.url),
                        skill: skill_def_from_a2a_skill(skill),
                        fitness: self.score_external(record, target),
                    });
                }
            }
        }

        candidates.sort_by(|a, b| b.fitness.partial_cmp(&a.fitness).unwrap_or(std::cmp::Ordering::Equal));
        Ok(candidates)
    }
}
```

**Fitness scoring for external agents** applies a trust-level discount:
- `Trusted`: 1.0x multiplier (same as internal)
- `Verified`: 0.8x multiplier
- `Untrusted`: 0.5x multiplier

#### 3.2.3 Streaming and Push for External Tasks

Extend `A2aAdapter` with streaming variants that use SSE (Server-Sent Events), matching the A2A spec's `tasks/sendSubscribe` method:

```rust
#[async_trait]
pub trait A2aAdapter: Send + Sync + 'static {
    // -- existing methods --
    async fn discover(&self, endpoint: &str) -> Result<Vec<AgentCard>, A2aError>;
    async fn send_task(&self, agent_url: &str, task: &Task) -> Result<Task, A2aError>;
    async fn get_task(&self, agent_url: &str, task_id: &str) -> Result<Task, A2aError>;
    async fn cancel_task(&self, agent_url: &str, task_id: &str) -> Result<Task, A2aError>;

    // -- new streaming method --
    /// Send a task and subscribe to streaming updates via SSE.
    /// Returns a stream of task status updates.
    async fn send_task_subscribe(
        &self,
        agent_url: &str,
        task: &Task,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<TaskStatusUpdate, A2aError>> + Send>>, A2aError>;
}

/// A streaming task status update from an external A2A agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatusUpdate {
    pub task_id: String,
    pub status: TaskStatus,
    /// Partial artifact if the agent streams output incrementally.
    pub artifact: Option<Artifact>,
    /// Whether this is the final update.
    pub final_: bool,
}
```

### 3.3 Integration Points

| Crate | Integration |
|---|---|
| `sera-a2a` | Add `A2aBridgeService` trait, `send_task_subscribe`, `TaskStatusUpdate` |
| `sera-auth` | `ExternalAgentPrincipal` registration for inbound A2A callers |
| `sera-gateway` | Expose `/.well-known/agent.json` endpoint, A2A JSON-RPC handler |
| `sera-skills` | Import external skills into `SkillRegistry` with `a2a:` source prefix |
| `sera-runtime::delegation` | `SkillRouter` includes external candidates |
| `sera-events` | Audit trail for all external A2A interactions |

### 3.4 Open Questions

1. **Discovery refresh:** How often should SERA re-discover external A2A agents? On-demand, periodic, or webhook-triggered?
2. **Skill namespace collisions:** If an external agent advertises a skill with the same name as an internal skill, should the router prefer internal? Should external skills be namespaced (e.g., `ext:code-review`)?
3. **Credential management:** How are credentials for authenticated external A2A agents stored? Via `sera-secrets`, or a dedicated federation credential store?
4. **Rate limiting:** Should SERA rate-limit outbound A2A requests per external agent? Per skill?

---

## Part 4: Circle Orchestration

### 4.1 Problem Statement

SERA Circles are groups of agents that collaborate on a shared domain. Current infrastructure lacks:

1. **Task distribution to best-available agent** -- no circle-level scheduler that routes tasks based on agent skills, load, and availability.
2. **Shared context during collaboration** -- agents in a circle cannot access a shared working memory during a collaborative task.
3. **Circle-level memory** -- the `MemoryBackend` trait supports `MemoryTier::Shared`, but there is no `WorkflowMemoryManager` that coordinates shared memory access across agents in a circle during a workflow.

### 4.2 Proposed Design

#### 4.2.1 Circle Task Distributor

A circle-level scheduler that receives tasks and routes them to the best-available agent based on skill match, current load, and circle policy.

```rust
/// Distributes tasks to agents within a circle.
#[async_trait]
pub trait CircleDistributor: Send + Sync {
    /// Submit a task to the circle. Returns the assigned agent and delegation handle.
    async fn submit(
        &self,
        circle_id: &str,
        task: CircleTask,
    ) -> Result<CircleAssignment, CircleError>;

    /// List agents currently available in the circle with their load status.
    async fn available_agents(
        &self,
        circle_id: &str,
    ) -> Result<Vec<CircleAgentStatus>, CircleError>;
}

/// A task submitted to a circle for distribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircleTask {
    pub description: String,
    /// Required skill (if known). The distributor matches this against agent capabilities.
    pub required_skill: Option<String>,
    /// Priority (higher = more urgent).
    pub priority: u32,
    /// Context from the requesting agent or workflow.
    pub context: serde_json::Value,
    /// Preferred agent (hint, not requirement).
    pub preferred_agent: Option<String>,
}

/// Result of circle task distribution.
#[derive(Debug, Clone)]
pub struct CircleAssignment {
    pub agent_id: String,
    pub session_key: String,
    pub delegation_handle: DelegationHandle,
}

/// Load and availability status of an agent in a circle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircleAgentStatus {
    pub agent_id: String,
    pub skills: Vec<String>,
    pub active_tasks: u32,
    pub max_concurrent_tasks: u32,
    pub available: bool,
}
```

**Distribution algorithm:**

1. Filter agents by `required_skill` (if specified).
2. Filter agents by availability (`active_tasks < max_concurrent_tasks`).
3. Score remaining agents:
   - Skill match specificity: exact match > tag match > general capability
   - Load headroom: `(max - active) / max`
   - Historical success rate for this skill type
   - Preferred agent bonus (if `preferred_agent` matches)
4. Assign to highest-scoring agent.
5. If no agent is available, queue the task with backpressure signaling.

#### 4.2.2 Shared Context Pattern

During a circle workflow, all participating agents access a shared `CircleContext` that provides:

```rust
/// Shared context for agents collaborating within a circle workflow.
pub struct CircleContext {
    /// Circle identifier.
    pub circle_id: String,
    /// Workflow identifier (scopes this collaboration).
    pub workflow_id: String,
    /// Shared memory backend scoped to this circle + workflow.
    pub memory: Arc<dyn MemoryBackend>,
    /// Shared key-value scratchpad for lightweight coordination.
    pub scratchpad: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    /// Broadcast channel for real-time updates to all circle members.
    pub broadcast: broadcast::Sender<CircleBroadcast>,
}

/// Broadcast messages within a circle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CircleBroadcast {
    /// An agent completed a sub-task.
    TaskCompleted { agent_id: String, summary: String },
    /// An agent found something relevant to share.
    Discovery { agent_id: String, content: String, tags: Vec<String> },
    /// An agent needs help or input from the circle.
    HelpRequest { agent_id: String, question: String },
    /// Workflow-level status update.
    StatusUpdate { phase: String, progress: f64 },
}
```

**Memory scoping:** The `CircleContext.memory` backend is initialized with a `MemoryContext` where `agent_id` is set to the circle ID (not individual agent ID), ensuring all writes go to the shared tier. Individual agents can still access their own `LongTerm` memory separately.

#### 4.2.3 WorkflowMemoryManager

Coordinates memory access during a multi-agent workflow, ensuring consistency and enabling cross-agent knowledge sharing. References the dreaming/recall infrastructure from `sera-types::memory`.

```rust
/// Manages shared memory lifecycle during a circle workflow.
#[async_trait]
pub trait WorkflowMemoryManager: Send + Sync {
    /// Initialize shared memory for a workflow.
    /// Creates the shared memory scope and returns a CircleContext.
    async fn init_workflow(
        &self,
        circle_id: &str,
        workflow_id: &str,
        participating_agents: &[String],
    ) -> Result<CircleContext, MemoryError>;

    /// Promote discoveries from workflow shared memory to circle long-term memory.
    /// Uses the dreaming promotion gates (minScore >= 0.8, minRecallCount >= 3,
    /// minUniqueQueries >= 3) to decide which entries are worth keeping.
    async fn consolidate(
        &self,
        circle_id: &str,
        workflow_id: &str,
    ) -> Result<ConsolidationResult, MemoryError>;

    /// Clean up workflow-scoped memory after completion.
    async fn cleanup_workflow(
        &self,
        circle_id: &str,
        workflow_id: &str,
    ) -> Result<(), MemoryError>;
}

/// Result of consolidating workflow memory into long-term circle memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationResult {
    /// Entries promoted to circle long-term memory.
    pub promoted: u32,
    /// Entries discarded (did not meet promotion gates).
    pub discarded: u32,
    /// Entries that were already present in long-term memory (deduplicated).
    pub deduplicated: u32,
}
```

**Consolidation flow:**

1. At workflow end, the `WorkflowMemoryManager` scans all `MemoryTier::Shared` entries for this workflow.
2. For each entry, it checks `RecallStore::stats_for()` to see if the entry meets the dreaming promotion gates.
3. Entries that pass are written to the circle's `MemoryTier::LongTerm` store.
4. The workflow-scoped `Shared` entries are then cleaned up.

### 4.3 Integration Points

| Crate | Integration |
|---|---|
| `sera-runtime::delegation` | `CircleDistributor` uses `DelegationOrchestrator` + `SkillRouter` |
| `sera-types::memory` | `MemoryBackend` for shared tier, `RecallStore` for promotion |
| `sera-session` | Circle workflows spawn linked sessions |
| `sera-events` | Circle broadcast events are also emitted as audit events |
| `sera-workflow` | `WorkflowMemoryManager` integrates with workflow engine lifecycle |
| `sera-skills` | `CircleDistributor` queries `SkillRegistry` for agent capabilities |

### 4.4 Open Questions

1. **Concurrency control:** Should the shared scratchpad use optimistic concurrency (CAS) or pessimistic locking? CAS is simpler but may lead to lost updates if two agents write the same key.
2. **Memory isolation between workflows:** Should two concurrent workflows in the same circle share memory, or should each workflow be fully isolated?
3. **Promotion gate tuning:** The dreaming gates (minScore >= 0.8, minRecallCount >= 3, minUniqueQueries >= 3) were designed for individual agent memory. Are they appropriate for circle-level consolidation, or do circle workflows need different thresholds?
4. **Circle membership changes:** If an agent joins or leaves a circle mid-workflow, how is the shared context updated? Should in-progress tasks be redistributed?
5. **Broadcast ordering:** Is `tokio::sync::broadcast` sufficient, or do we need ordered delivery guarantees for circle broadcasts?

---

## Cross-Cutting Concerns

### Error Handling

All new traits use domain-specific error types that implement `Into<SeraError>` via `sera-errors`, following the existing pattern in `sera-a2a::A2aError`.

### Observability

Every delegation, message, and circle operation emits structured tracing spans using the `tracing` crate, consistent with existing instrumentation in `DelegationOrchestrator`.

### Authorization

- Internal skill delegation: checked by `DelegationConfig::allowed_targets` (existing).
- Agent messaging: checked by a new `MessagePolicy` on the sender/recipient pair.
- External A2A: checked by `ExternalAgentIdentity::trust_level` + `sera-auth`.
- Circle operations: checked by circle membership policy.

### Migration Path

All new fields on existing types (e.g., `DelegationRequest::skill_target`) are `Option<T>` with `#[serde(default)]`, ensuring backward compatibility. New traits (`SkillRouter`, `SessionChannel`, `A2aBridgeService`, `CircleDistributor`, `WorkflowMemoryManager`) are additive and do not modify existing trait contracts.

---

## Implementation Priority

| Priority | Component | Estimated Effort | Dependencies |
|---|---|---|---|
| P0 | `SkillTarget` + `SkillRouter` trait | S | sera-types, sera-skills |
| P0 | Callback-on-completion (`DelegationHandle`) | S | sera-runtime |
| P1 | `AgentMessage` types + `MessageKind` enum | S | sera-types |
| P1 | `SessionChannel` trait (spawn/yield/send) | M | sera-session |
| P1 | `StreamingDelegationHandle` + `DelegationEvent` | M | sera-runtime |
| P2 | `A2aBridgeService` trait | M | sera-a2a, sera-auth |
| P2 | `send_task_subscribe` (SSE streaming) | M | sera-a2a |
| P2 | `CircleDistributor` trait | M | sera-runtime, sera-skills |
| P2 | `CircleContext` + shared memory | M | sera-types::memory |
| P3 | `WorkflowMemoryManager` + consolidation | L | sera-workflow, sera-types::memory |
| P3 | `CollaborativeSessionConfig` | M | sera-session |
| P3 | External skill integration in `SkillRouter` | M | sera-a2a, sera-skills |

**S** = Small (< 1 day), **M** = Medium (1-3 days), **L** = Large (3-5 days)
