# SPEC: Circles — Multi-Agent Coordination (`sera-circles`)

> **Status:** DRAFT
> **Source:** PRD §11.2, plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §10.3 (Paperclip atomic optimistic-lock checkout, 4-trigger wakeup taxonomy, `PluginEvent` envelope, three-layer failure model, `revision_requested` state, `ConcurrencyPolicy`, `reportsTo` with `assertNoCycle`), §10.5 (openclaw `subagent_delivery_target`, `parent_session_key` + `spawned_by`), §10.7 (opencode `task_id` subagent session resumption), §10.12 (ChatDev Tarjan SCC cycle detection, three loop terminators, edge-level verdicts, Map/Tree reduce, `subgraph` nesting, `blackboard` memory), §10.13 (openai-agents-python handoff-as-tool-call, `HandoffInputFilter`), §10.14 (CrewAI `Task.context` DAG wiring, `Process.hierarchical` with manager LLM, `DelegateWorkTool`, `Flow` event-driven state machines, `Task.guardrail` retry loop), §10.15 (MetaGPT `Action` vs `Tool`, `cause_by` typed routing, `_watch` declarative subscription, Environment push-to-inbox, termination triad, SOP as implicit watch-graph), §10.17 (CAMEL `TaskChannel` + `Packet` lifecycle, `WorkforceMode::{AutoDecompose, Pipeline}`, `WorkforceState::Paused` + `WorkforceSnapshot`, `validate_task_content` failure blacklist, `TaskSpecifyAgent` pre-pass, `RolePlayingCircle`, three-agent GAIA workforce), [SPEC-self-evolution](SPEC-self-evolution.md) §14.3 (approval-path separation for meta-changes)
> **Crate:** `sera-types` (model), `sera-runtime` (coordination), `sera-workflow` (task lane + atomic claim — see [SPEC-workflow-engine](SPEC-workflow-engine.md))
> **Priority:** Phase 3 (design) · Phase 4 (full implementation) — **design obligations land in Phase 2 runtime work** (the `subagent_delivery_target` hook, `parent_session_key` on sessions, `Action`/`cause_by` routing types must exist before Circle coordination can be implemented)

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
    pub sub_circles: Vec<CircleId>,              // DAG: circles can contain circles (§4)
    pub parent: Option<CircleId>,                 // Parent circle in the DAG
    pub coordination: CoordinationPolicy,         // §3
    pub concurrency: ConcurrencyPolicy,           // §3.1 — orthogonal to coordination
    pub goal: Option<String>,
    pub watch_signals: HashSet<ActionId>,         // Declarative SOP via cause_by routing (§5a)
    pub task_specifier: Option<TaskSpecifierConfig>, // Pre-execution sharpening (§3g)
    pub result_aggregator: ResultAggregatorRef,   // Trait impl per §3c
    pub convergence: ConvergenceConfig,           // Loop terminators (§3d)
    pub production_bounds: WorkforceBounds,       // Retries, timeouts, pool size (§5b)
    pub blackboard: Option<CircleBlackboardRef>,  // Shared artifact bus (§3f)
    pub operator_approver_set: PrincipalSet,      // Pinned meta-approvers (SPEC-self-evolution §7)
}

pub struct CircleMember {
    pub principal: PrincipalRef,                  // Agent, human, or another Circle (composition)
    pub role: CircleRole,
    pub can_delegate: bool,                        // Maps to `allow_delegation` in CrewAI pattern; gates DelegateWorkTool injection
    pub reports_to: Option<PrincipalRef>,          // Paperclip hierarchy (§4), `assertNoCycle()` enforced at load time
    pub capabilities: HashSet<AgentCapability>,    // `Delegation` capability injects the delegation tool
}

pub enum CircleRole {
    Lead,                       // Coordinates work, reviews output (`Process::hierarchical` manager role)
    Worker,                     // Executes tasks
    Reviewer,                   // Reviews work products (can emit CircleVerdict, §3d)
    Observer,                   // Watch-only — receives events but does not act
    TaskSpecifier,              // Dedicated pre-execution prompt sharpener (CAMEL pattern, §3g)
    Critic,                     // Optional per-turn interception in RolePlayingCircle (§3h)
}

/// Production operational bounds — defaults drawn from CAMEL Workforce.
/// Source: SPEC-dependencies §10.17.
pub struct WorkforceBounds {
    pub max_task_retries: u32,                    // Default: 3
    pub max_pending_tasks: u32,                   // Default: 20
    pub task_timeout_secs: u64,                   // Default: 600
    pub worker_pool_size: u32,                    // Default: 10
    pub cost_budget: Option<CostBudget>,          // MetaGPT `invest(...)` pattern — NoMoneyException on exhaustion
    pub n_round_limit: Option<u32>,               // MetaGPT hard round cap
}
```

---

## 3. Coordination Policies

```rust
pub enum CoordinationPolicy {
    /// Members execute in order; output of N is input to N+1.
    /// Maps to MetaGPT `react_mode = BY_ORDER`.
    Sequential,

    /// CAMEL `WorkforceMode::Pipeline` — predefined ordered steps, no dynamic decomposition.
    /// Distinct from Sequential in that the step graph is declared up front and validated at load.
    /// Source: SPEC-dependencies §10.17.
    Pipeline {
        steps: Vec<PipelineStep>,
    },

    /// All workers receive the task simultaneously; results merged via ResultAggregator (§3c).
    /// ChatDev Map (fan-out) + Tree (fan-out + reduce) patterns via the MergeStrategy field.
    Parallel {
        merge: MergeStrategy,
    },

    /// CAMEL `WorkforceMode::AutoDecompose` — LLM-driven dynamic DAG decomposition.
    /// A coordinator agent decomposes the incoming task into sub-tasks and assigns workers at runtime.
    /// Source: SPEC-dependencies §10.17.
    AutoDecompose {
        coordinator: PrincipalRef,
        task_specify: Option<TaskSpecifierConfig>,
    },

    /// CrewAI `Process.hierarchical` — a manager LLM receives the full task list + worker roster
    /// and emits DelegateWorkTool calls. Workers MUST have `allow_delegation = false`.
    /// The manager is just an LLM session with a delegation tool — NO special coordinator plumbing.
    /// Source: SPEC-dependencies §10.14.
    Supervised {
        manager: PrincipalRef,
        convergence: ConvergenceConfig,
    },

    /// Members each produce a response; ResultAggregator applies a voting strategy.
    /// NOTE: no prior-art framework implements this completely (ChatDev left it as `# TODO: consensual`);
    /// SERA builds this from scratch (SPEC-dependencies §10.12, §10.14).
    Consensus {
        quorum: f32,                  // Fraction that must agree (0.0..1.0)
        voting_fn: VotingStrategy,
        consensus_timeout: Duration,
        fallback: Box<CoordinationPolicy>, // Falls back (typically to Parallel/Collect) if quorum not reached
    },

    /// CrewAI `Flow` — event-driven method state machine with typed state.
    /// Each handler is a Circle stage; state is a typed envelope; @router is a conditional edge.
    /// Source: SPEC-dependencies §10.14.
    Flow {
        state_schema: SchemaRef,
        handlers: Vec<FlowHandler>,
        persistence: FlowPersistenceRef,
    },

    /// Hook-resolved custom policy.
    Custom(String),
}

pub enum VotingStrategy {
    Majority,                                    // >50%
    Unanimous,                                    // All agree
    Weighted(HashMap<PrincipalRef, f32>),        // Per-member weight
    LeadReview,                                   // Lead casts the deciding vote
}

pub enum MergeStrategy {
    /// Flat list to ResultAggregator (ChatDev Map pattern)
    Collect,
    /// Hierarchical recursive reduction (ChatDev Tree pattern)
    Reduce {
        group_size: u32,                         // Reduce in groups of N, recurse
        reducer: ResultAggregatorRef,
    },
    /// First non-error wins
    FirstNonError,
}
```

### 3.1 Concurrency Policy (orthogonal sub-field)

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.3 Paperclip routines.

Concurrency policy is **orthogonal to CoordinationPolicy** — it governs what happens when a trigger fires while a Circle is already running:

```rust
pub enum ConcurrencyPolicy {
    /// A new invocation is skipped if the Circle is currently running.
    SkipIfActive,

    /// A new invocation coalesces with the running one (merges task inputs).
    Coalesce,

    /// A new invocation is enqueued behind the running one (default).
    AlwaysEnqueue,
}
```

This prevents thundering-herd on timer-triggered Circles and maps to Paperclip's `routines.concurrencyPolicy`.

---

## 3a. Task Channel and Packet Lifecycle

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.17 CAMEL `TaskChannel` — production-grade atomic task checkout with explicit packet lifecycle.

Every Circle owns a `TaskChannel` that routes `Packet`-wrapped tasks to its members. The channel is backed by an async-native queue with O(1) lookup and a status index. This is SERA's lane-aware FIFO queue per Circle (see [SPEC-gateway](SPEC-gateway.md) §5 for the gateway-level equivalent).

```rust
pub struct TaskChannel {
    packets: HashMap<TaskId, Packet>,
    by_status: HashMap<PacketStatus, HashSet<TaskId>>,
    notifier: Notify,                  // async-native condvar equivalent
}

pub struct Packet {
    pub task: Task,
    pub publisher_id: PrincipalRef,
    pub assignee_id: Option<PrincipalRef>,
    pub status: PacketStatus,
    pub claim_token: Option<ClaimToken>,   // Optimistic lock per §3b
    pub created_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub returned_at: Option<DateTime<Utc>>,
}

pub enum PacketStatus {
    Sent,           // In queue, not yet claimed
    Processing,     // Claimed by a worker, in flight
    Returned,       // Worker has produced a result (may still need aggregation / validation)
    Archived,       // Terminal: result is durable and the packet is no longer in-flight
}
```

The four-state lifecycle is explicit. Workers transition packets `Sent → Processing → Returned`, the ResultAggregator transitions `Returned → Archived`.

---

## 3b. Atomic Optimistic-Lock Checkout

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.3 Paperclip `issues.checkout()` + §10.4 beads `bd update --claim` atomic protocol.

Every task dispatch uses an atomic optimistic-lock checkout to prevent double-execution in parallel policies:

```rust
pub async fn checkout(
    channel: &TaskChannel,
    task_id: TaskId,
    agent_id: PrincipalRef,
    expected_statuses: &[PacketStatus],   // e.g. &[Sent]
    run_id: RunId,
) -> Result<ClaimToken, CheckoutError>;
```

The operation is atomic against the packet's status. If the packet is no longer in one of `expected_statuses`, the checkout fails with `CheckoutError::StatusMismatch` and the caller retries with fresh state. This is identical in semantics to beads' `bd update --claim` and to Paperclip's issue checkout.

**Stale-lock recovery.** A `StaleLockReaper` background task scans for `Processing` packets whose assignee is no longer live (process liveness check via `/proc/<pid>/status` or equivalent), releases the lock, and emits a `LaneFailureClass::OrphanReaped` event. Maps to Paperclip's `adoptStaleCheckoutRun()`.

---

## 3c. ResultAggregator Trait (SERA must build)

> **Source:** **Gap identified in prior research.** Paperclip has no result-aggregation primitive (SPEC-dependencies §10.3); ChatDev leaves Consensus commented out; CrewAI only has `Task.guardrail` retry but no multi-result merge. SERA builds this from scratch. The integration point is openclaw's `subagent_delivery_target` hook (SPEC-dependencies §10.5).

```rust
#[async_trait]
pub trait ResultAggregator: Send + Sync {
    /// Aggregate multiple task results into a single output. Called for Parallel/Consensus policies
    /// after all workers return, or incrementally for hierarchical Reduce merges.
    async fn aggregate(
        &self,
        results: Vec<TaskResult>,
        context: &AggregationContext,
    ) -> Result<AggregatedResult, AggregationError>;

    /// Validate a single task result before accepting it. CAMEL `validate_task_content()`
    /// pattern — checks failure-pattern blacklist (e.g. "I cannot complete", "task failed")
    /// to prevent silent hallucinated completions from propagating.
    /// Source: SPEC-dependencies §10.17.
    async fn validate(
        &self,
        result: &TaskResult,
    ) -> Result<ValidationVerdict, ValidationError>;

    fn name(&self) -> &str;
}

pub enum ValidationVerdict {
    Accept,
    Reject { reason: String },
    RetryRequested { feedback: String }, // CrewAI Task.guardrail retry loop pattern
}

pub enum AggregatedResult {
    /// Single final result.
    Final(TaskResult),
    /// Quorum not yet reached; keep collecting.
    NeedsMoreResults { received: u32, required: u32 },
    /// Consensus failed to converge; fall back to alternative policy.
    Fallback(Box<CoordinationPolicy>),
    /// Request a revision from a specific member (CrewAI + Paperclip revision_requested pattern).
    RevisionRequested {
        target: PrincipalRef,
        feedback: String,
    },
}
```

**Built-in implementations:**

| Name | Strategy |
|---|---|
| `FirstNonError` | Pick first non-error result |
| `Majority` | Quorum vote (for `Consensus`) |
| `WeightedConsensus` | Weighted vote |
| `LeadReviewer` | Route all results to a designated Lead for final verdict |
| `StructuredMerge` | Merge structured outputs field-by-field with conflict resolution |

The `subagent_delivery_target` hook (SPEC-hooks §3) is the integration point: it fires between subagent completion and parent delivery, and invokes the Circle's `ResultAggregator::aggregate()` before the result reaches the parent.

---

## 3d. CircleVerdict and Loop Terminators

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.12 ChatDev — three orthogonal loop terminators that must be separately configurable. Supervised review loops without explicit terminators are the classic failure mode.

Review verdicts are **edge conditions on the Circle DAG**, not free-text parsing of Reviewer output. Reviewer agents emit a structured `CircleVerdict`:

```rust
pub enum CircleVerdict {
    Approved,
    RevisionRequired(String),         // Feedback routed back to the upstream worker
    Escalate,                         // Route to next approver in chain (meta-approver, human)
    Reject,                           // Hard reject; task fails
}
```

Supervised policies define **three orthogonal terminators**, all separately configurable (ChatDev pattern):

```rust
pub struct ConvergenceConfig {
    /// Keyword / verdict condition on the exit edge. Primary terminator for Supervised.
    pub convergence_signal: ConvergenceSignal,

    /// Count-based circuit breaker. Forces exit after N review cycles regardless of verdict.
    pub max_review_cycles: u32,

    /// Time-based circuit breaker. Forces exit after a configured duration.
    pub review_timeout: Duration,
}

pub enum ConvergenceSignal {
    /// Reviewer emits `CircleVerdict::Approved`
    VerdictApproved,
    /// Reviewer output contains any of these tokens (legacy pattern — less preferred)
    KeywordAny(Vec<String>),
    /// Reviewer output contains none of these tokens (e.g., loop until `ACCEPT` appears)
    KeywordNone(Vec<String>),
}
```

All three terminators fire independently — the review loop exits on whichever triggers first. This closes the "infinite review loop" failure mode that plagued ChatDev 1.0 and MetaGPT early teams.

---

## 3e. Map, Tree, Subgraph (Parallel / Subcircle composition)

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.12 ChatDev dynamic execution + `subgraph` node.

`Parallel` policies support fan-out + reduce via `MergeStrategy`:

- **Map (flat fan-out):** `MergeStrategy::Collect` — all workers process the task in parallel, results are collected as a `Vec<TaskResult>` and passed to the `ResultAggregator`.
- **Tree (hierarchical reduce):** `MergeStrategy::Reduce { group_size, reducer }` — results are reduced in groups of N via a recursive invocation of the reducer. Enables log-depth aggregation over many workers.

**Subgraph nesting.** A Circle can contain sub-Circles via `sub_circles: Vec<CircleId>`. A sub-Circle behaves exactly like a single member with a typed interface: its `start` member receives the delegated task, and the output of its `end` member is what gets returned to the parent. The parent does NOT see individual sub-Circle member outputs — only the final aggregated result. This is ChatDev's `subgraph` semantics and matches SERA's composition principle: Circles are themselves workers that can be composed.

---

## 3f. CircleBlackboard — Shared Artifact Bus

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.12 ChatDev `blackboard` memory.

For `Parallel`, `Consensus`, and `Supervised` policies, members often need to see each other's intermediate work products (drafts, code, scores) before the final aggregation step. The `CircleBlackboard` is an append-only, recency-ordered, name-scoped artifact log — distinct from per-agent `recall` memory and from the gateway's per-session event channel.

```rust
pub struct CircleBlackboard {
    pub circle_id: CircleId,
    pub entries: VecDeque<BlackboardEntry>,    // Append-only, recency-ordered
    pub retention: BlackboardRetention,         // Compaction policy
}

pub struct BlackboardEntry {
    pub id: EntryId,
    pub author: PrincipalRef,
    pub cause_by: Option<ActionId>,
    pub content: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub tags: Vec<String>,
}
```

Properties:

- **Write-many, read-all** scoped to the Circle session
- **No embeddings** — it's a simple append log, not a vector store (use `sera-memory` for retrieval-augmented needs)
- **Compacted at termination** — entries older than the retention policy are summarized before the Circle session closes
- **Not a replacement for memory** — it is specifically for intermediate work products during a single Circle run

---

## 3g. TaskSpecifier Pre-Pass

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.17 CAMEL `TaskSpecifyAgent` + `SystemMessageGenerator` keyed on `TaskType`.

Before a Circle executes, an optional **TaskSpecifier** sharpens the vague incoming task into a specific, actionable one via a dedicated LLM call. This is not a worker — it's a formal pre-execution stage with its own agent class.

```rust
pub struct TaskSpecifierConfig {
    pub enabled: bool,
    pub specifier: PrincipalRef,                 // Dedicated agent — usually a narrow planner
    pub task_type: TaskType,                      // AI_SOCIETY | CODE | SCIENCE | ... (extensible registry)
    pub system_message_template: TemplateRef,    // Keyed on task_type per CAMEL SystemMessageGenerator
    pub max_iterations: u32,                      // Default: 1 (single sharpening pass)
}
```

Flow: `user_task → TaskSpecifier → (sharpened_task, role-conditioned system prompts) → Circle dispatch`.

This fills the "no pre-execution sharpening" gap in SERA's current design. It is optional per Circle but strongly recommended for `AutoDecompose` and `Supervised` policies where vague task definitions amplify downstream coordination cost.

---

## 3h. RolePlayingCircle — Two-Agent Dialogue Primitive

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.17 CAMEL `RolePlaying` protocol (the original 2023 CAMEL academic contribution).

A specialized Circle variant for **two-agent focused dialogue**. One `user_role` issues instructions; one `assistant_role` responds and executes. An optional `CriticAgent` intercepts each turn.

```rust
pub struct RolePlayingCircle {
    pub user_role: CircleMember,        // Emits instructions
    pub assistant_role: CircleMember,   // Executes instructions
    pub critic: Option<CircleMember>,   // Optional turn-level interceptor
    pub task_specify: bool,              // Whether to run TaskSpecifier first (§3g)
    pub task_planner: bool,              // Whether to insert a TaskPlanner step after specify
    pub stop_signal: StopCondition,     // threading::Event equivalent
    pub max_react_loops: u32,
}
```

`RolePlayingCircle` composes into a larger Workforce via `RolePlayingWorker` — the two-agent dialogue becomes one worker in the parent Circle. This means a 2-agent primitive is itself a member of an N-agent Circle. Composition all the way up.

---

## 3i. Handoff-as-Tool-Call (Delegation Contract)

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.13 openai-agents-python + §10.14 CrewAI `DelegateWorkTool`.

Delegation between Circle members is **a pre-registered LLM tool call** — not framework-controlled routing. When `CircleMember.can_delegate = true` (or `capabilities` contains `Delegation`), SERA injects a `DelegateWorkTool` into the member's tool registry:

```rust
/// Schema visible to the LLM; the LLM decides when to delegate and to whom.
pub struct DelegateWorkToolSchema {
    pub task: String,
    pub context: String,
    pub coworker: String,                // Resolved against the Circle's member roster by `role` string
}
```

The runner intercepts the tool call by name (`transfer_to_*` or `delegate_work` convention), applies an optional `HandoffInputFilter` (openai-agents-python pattern — context passed to the receiving agent is fully programmable: strip, summarize, or rewrite history), and switches to the target agent's session.

```rust
pub type HandoffInputFilter = Box<dyn Fn(HandoffInputData) -> BoxFuture<'static, HandoffInputData> + Send + Sync>;
```

This is auditable in the normal tool-call trace, LLM-driven, and replaces hand-written routing logic. The `subagent_delivery_target` hook then handles the return path via `ResultAggregator`.

**Session resumption.** Following opencode's `task_id` pattern (SPEC-dependencies §10.7), delegated sub-agent sessions are addressable across parent turns. A child session is created with `parent_session_key` set; the parent may re-invoke the child on a later turn by referencing its `task_id`, and the child's conversation history is preserved.

---

## 4. DAG Structure

```
Organization Circle
├── Engineering Circle (Supervised — manager LLM + DelegateWorkTool)
│   ├── Frontend Circle (Sequential)
│   │   ├── Agent: UI-Designer (Lead)
│   │   └── Agent: Code-Writer (Worker)
│   └── Backend Circle (Parallel with MergeStrategy::Collect)
│       ├── Agent: Architect (Lead, can_delegate=true)
│       ├── Agent: Implementer (Worker)
│       └── Agent: Code-Reviewer (Reviewer) → emits CircleVerdict
└── Operations Circle (AutoDecompose)
    └── Monitoring Circle
        └── Agent: SRE-Bot (Lead)
```

### 4.1 DAG Execution Algorithm

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.12 ChatDev Tarjan SCC cycle detection.

Circle execution uses **Tarjan strongly-connected-component (SCC) detection** with super-node promotion:

1. Build the member + sub-Circle DAG
2. Detect SCCs; any cycle becomes a **super-node** that executes recursively
3. Topological sort the super-node graph
4. Within a topological layer, members run **truly in parallel** (tokio tasks)
5. Cycles execute until their `ConvergenceConfig` terminates them

**Circles remain DAG-at-the-declaration-level.** A Circle can have at most one parent. Cycles are only permitted **within** a Circle (for review loops); a Circle cannot declare itself or a sub-Circle as its parent. `assertNoCycle()` (Paperclip pattern, SPEC-dependencies §10.3) runs at config load time before any work is dispatched.

### 4.2 Explicit DAG vs. Paperclip's emergent coordination

SERA explicitly **rejects** Paperclip's implicit DAG-via-issue-graph pattern (SPEC-dependencies §10.3). Paperclip agents discover peers via shared memory and the DAG emerges from issue creation — you cannot inspect the planned execution graph before it runs. SERA's Circle DAG stays explicit and declared so the gateway can validate cycle-freedom, pre-compute critical paths, and emit audit events for the full planned execution before dispatch. Auditability and reproducibility beat emergent elegance.

---

## 5. Task Delegation Lifecycle

When a Circle receives a task, it flows through a formal lifecycle — each step is auditable via the gateway EventStream (SPEC-gateway §3.3):

```
  1. Inbound task                        →  Packet(status=Sent) created
  2. TaskSpecifier pre-pass (if enabled) →  Sharpened task replaces raw input
  3. Coordination policy routing         →  Assignment algorithm per policy type
  4. Atomic checkout (§3b)               →  Packet(status=Processing), ClaimToken issued
  5. Worker executes turn                →  Events written to CircleBlackboard (§3f)
  6. Packet(status=Returned)             →  Result submitted back to channel
  7. validate_task_content (§3c)         →  Failure-pattern blacklist check
  8. ResultAggregator.aggregate (§3c)    →  Merge / quorum / verdict / revision
  9. subagent_delivery_target hook       →  Final result transformation
  10. Packet(status=Archived)            →  Terminal; result flows back to parent
```

Each step emits structured events via the gateway EventStream. A task can transition to `Archived` via normal completion, or via `ConvergenceConfig` timeout, or via failure recovery (§5b).

### 5.1 Atomic claim protocol

See §3b. The gateway exposes `claim_task(task_id, agent_id)` as a single-transaction operation; workers race to claim packets via this API, and losers retry with fresh state. This mirrors beads `bd update --claim` (SPEC-dependencies §10.4) and is the foundation of SERA's multi-agent safety model.

### 5.2 Cross-circle delegation via DAG

Task delegation across Circles follows the DAG structure — a parent Circle can delegate to sub-Circles by treating them as single members (§4, §3e). Sub-Circle invocation uses the same atomic claim protocol as intra-Circle dispatch. There is no separate "inter-Circle" dispatch path.

---

## 5a. Declarative SOP via Watch Signals

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.15 MetaGPT `_watch` + `cause_by` — the most important architectural lesson from the research round. **SERA does NOT build a DSL for Circle workflows.**

SERA's Circle workflow is **not defined by a workflow DSL** (no `sop.yaml`, no state-machine language). Instead, the workflow *emerges* from each agent's `watch_signals` declaration, which is a declarative subscription on the `cause_by` typed routing key:

```rust
// Each CircleMember declares which Action IDs it cares about.
// The Circle coordinator pushes Messages only to members whose watch_signals
// include the Message's cause_by field. Routing is delivery-time, not read-time.
impl Circle {
    pub async fn publish(&self, message: Message) {
        for member in &self.members {
            if should_deliver(&message, &member.watch_signals, &member.principal) {
                member.inbox.push(message.clone()).await;
            }
        }
    }
}

fn should_deliver(msg: &Message, watch: &HashSet<ActionId>, recipient: &PrincipalRef) -> bool {
    msg.send_to.contains(recipient)
        || msg.cause_by.as_ref().is_some_and(|cb| watch.contains(cb))
}
```

The "SOP" is the watch-graph. This means:

- A Circle YAML declares only `members`, `watch_signals` per member, and `CoordinationPolicy`
- The execution graph emerges from the subscription topology at runtime
- The gateway can still pre-validate the graph (cycle detection, unreachable members) before dispatch
- Adding a new member to a Circle does not require modifying a central orchestrator — the new member simply declares its `watch_signals` and starts receiving relevant messages

**Push-to-inbox, NOT blackboard.** Routing is filtered at delivery time — the gateway pushes directly to each member's private event queue, matching MetaGPT's `Environment.publish_message` pattern. The `CircleBlackboard` (§3f) is separate and exists for intermediate artifact sharing, not for agent-to-agent message delivery.

---

## 5b. Three-Layer Failure Model

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.3 Paperclip.

Every Circle runs a three-layer failure-handling stack:

**Layer 1 — Orphan reaping:**
A `StaleLockReaper` background task scans `Processing` packets whose assignee principal is no longer live (process liveness check via `/proc/<pid>/status` or harness heartbeat). Stale locks are released and the packet returns to `Sent` status. Emits `LaneFailureClass::OrphanReaped`.

**Layer 2 — Process-loss retry:**
When a worker crashes mid-turn, the packet is retried **up to `WorkforceBounds.max_task_retries` times** (default 3) with exponential backoff. Retries preserve the original `idempotency_key` so tool calls are not double-executed. Maps to Paperclip's `enqueueProcessLossRetry()`.

**Layer 3 — Budget cancellation:**
When a Circle's `cost_budget` is exhausted (MetaGPT `NoMoneyException` pattern), all in-flight packets are cancelled and the Circle session transitions to `Stopped` with reason `BudgetExhausted`. Scoped cancellation: only packets in the same budget scope are killed.

**Output enforcement:**
If a worker returns control without producing the outputs required by the task contract, a follow-up wake is automatically queued with a `RevisionRequested` feedback message. Prevents loop exhaustion and silently-incomplete work. Maps to Paperclip's `finalizeIssueCommentPolicy()`.

Terminal states: `Succeeded | Failed | Cancelled | TimedOut | BudgetExhausted`. All stale checkout locks are released on terminal transition.

---

## 5c. Pause and Resume via WorkforceSnapshot

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.17 CAMEL `WorkforceState::Paused` + `WorkforceSnapshot`.

Circles support **human-intervention pause** via a dedicated session state and a serializable snapshot:

```rust
pub struct WorkforceSnapshot {
    pub circle_id: CircleId,
    pub paused_at: DateTime<Utc>,
    pub task_channel_state: TaskChannelSnapshot,
    pub blackboard_state: BlackboardSnapshot,
    pub in_flight_packets: Vec<(TaskId, Packet, ClaimToken)>,
    pub pending_review_cycles: u32,
    pub cost_accumulated: CostBudget,
    pub pause_reason: PauseReason,
}

pub enum PauseReason {
    HumanIntervention { principal: PrincipalRef },
    CircuitBreaker { trigger: CircuitBreakerTrigger },
    MetaChange { change_artifact_id: ChangeArtifactId }, // self-evolution §5.5
    KillSwitchActivated,
}
```

A paused Circle's session transitions to `SessionState::Paused` (see SPEC-gateway §6.1). The snapshot is written to durable storage via `sera-db` and can be replayed to resume. In-flight worker turns complete (workers do not see the pause), but no new packets are dispatched until the snapshot is resumed.

Pause is **not** an error state — it is a legitimate operational mode for long-running Circles that need human review at specific junctures.

---

## 5d. Four-Trigger Wakeup Taxonomy

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.3 Paperclip.

Circle scheduling uses four named wakeup trigger types, distinct from SPEC-workflow-engine's `WorkflowTrigger` (which is about *starting* a workflow). Wakeups are about *resuming* a Circle member's turn:

```rust
pub enum CircleWakeup {
    /// Timer-driven: periodic heartbeat or scheduled reactivation.
    Timer { interval: Duration, next_fire: DateTime<Utc> },

    /// Assignment-driven: a task was assigned to this member.
    Assignment { task_id: TaskId },

    /// On-demand: a human or another agent explicitly invoked this member.
    OnDemand { invoker: PrincipalRef },

    /// Automation: process-loss retry, missing-output follow-up, orphan-reap fallout.
    /// Operationally the most important and most often omitted from simpler models.
    Automation { reason: AutomationReason },
}

pub enum AutomationReason {
    ProcessLossRetry { attempt: u32 },
    OutputEnforcement { missing_fields: Vec<String> },
    OrphanReapFallout { released_task: TaskId },
    RevisionRequested { reviewer: PrincipalRef, feedback: String },
}
```

The `Automation` wakeup is the one most commonly missing from agent frameworks. Naming it explicitly gives SERA a clean extension point for robustness features (retry policies, output enforcement, feedback loops) without ad-hoc plumbing.

---

## 5e. CrewAI-Style Flow State Machines as `CoordinationPolicy::Flow`

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.14 CrewAI `Flow` + `@start / @listen / @router`.

For event-driven coordination that does not fit the Crew/Workforce task-list model, SERA supports **Flow-style state machines** as a distinct `CoordinationPolicy::Flow` variant. A Flow defines:

- A typed `FlowState` Pydantic-equivalent schema (serde, validated via `schemars`)
- Handlers annotated with `@start(condition?)` or `@listen(upstream_method | or_(...) | and_(...))`
- `@router(method) -> string` decorators for conditional branching (the string routes to named downstream handlers)
- `FlowPersistence` pluggable backends (default: SQLite via `sera-db`)
- `@human_feedback` decorator on any handler — pauses the flow, serializes state to `WorkforceSnapshot`, resumes asynchronously when feedback arrives (see §5c)

A Flow can embed a Crew/Workforce kickoff as one of its handlers — Flows and the other coordination policies compose, they don't compete. This gives SERA both task-list and state-machine models under one spec without forcing operators to pick a side.

---

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
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | PrincipalGroups for authorization; Circles for coordination; capability tokens narrow member privileges |
| `sera-runtime` | [SPEC-runtime](SPEC-runtime.md) | Subagent management, `Action` vs `Tool`, `cause_by` routing, `react_mode`, handoff-as-tool |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | **`subagent_delivery_target` hook is the `ResultAggregator` integration point**; `PluginEvent` envelope with `correlation_id`; custom policies via hooks |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Circle leads can be approval targets; `revision_requested` state; doom-loop escalation; `CircleVerdict::Escalate` routes here |
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Channel events enter the gateway pipeline; `parent_session_key` + `spawned_by` for subagent lineage; `Paused` state + `Shadow` mode |
| `sera-workflow` | [SPEC-workflow-engine](SPEC-workflow-engine.md) | Task-lane atomic claim protocol; `WorkflowTask` model backs Circle packets; `meta_scope` field for self-evolution tasks |
| `sera-memory` | [SPEC-memory](SPEC-memory.md) | `CircleBlackboard` is distinct from per-agent recall memory; sub-agent experience pool |
| `sera-meta` | [SPEC-self-evolution](SPEC-self-evolution.md) | Operator approver pinning (§7); meta-change Circles route through separate approval path; Circle config changes are Tier-2 `SingleCircleConfig` scope |
| Dependencies | [SPEC-dependencies](SPEC-dependencies.md) | §10.3 Paperclip atomic checkout + 4-trigger wakeup + 3-layer failure + `revision_requested`; §10.5 openclaw `subagent_delivery_target` + `parent_session_key`; §10.7 opencode `task_id` resumption; §10.12 ChatDev SCC + loop terminators + Map/Tree + subgraph + blackboard; §10.13 openai-agents-python handoff-as-tool + `HandoffInputFilter`; §10.14 CrewAI `Task.context` + `Process.hierarchical` + `Flow`; §10.15 MetaGPT `Action`/`cause_by`/`_watch`; §10.17 CAMEL `TaskChannel` + `WorkforceMode` + pause/resume + `validate_task_content` + `TaskSpecifier` + `RolePlayingCircle` |

---

## 8. Open Questions

1. **Circles vs. PrincipalGroups overlap** — When a Circle also needs authorization boundaries, should Circles automatically create PrincipalGroups, or remain purely coordination? (PRD §19)
2. ~~**Circle task protocol**~~ — Resolved: see §3b atomic checkout + §3i handoff-as-tool-call. Delegation is an LLM-visible tool call, not a special event type.
3. ~~**Circle session management**~~ — Resolved: each Circle member runs in its own session with `parent_session_key` lineage (§3i, openclaw pattern).
4. ~~**Result merging**~~ — Resolved: `ResultAggregator` trait (§3c) with five built-in strategies and validation via failure-pattern blacklist (CAMEL `validate_task_content`).
5. **Circle lifecycle** — Can circles be created/modified at runtime, or are they config-only? **Tentative answer:** both. Config-first at startup, runtime-modifiable as a Tier-2 self-evolution `SingleCircleConfig` scope change (SPEC-self-evolution §9.1).
6. **Human members** — The model allows human principals as circle members. What does this look like in practice? **Tentative answer:** humans receive tasks via HITL approval prompts that function as `CircleWakeup::OnDemand`. `CircleVerdict` is emitted via the approval response channel.
7. **Channel persistence** — Are channel messages persisted? For how long? Are they searchable via memory?
8. **Channel auto-creation** — Should circles automatically create associated channels, or are channels always manually configured?
9. **TaskSpecifier as meta-approver** — Can a TaskSpecifier agent double as a preliminary meta-approver for Tier-2 config changes proposed via a Circle? Reduces human bottleneck but introduces automation-bias risk. Requires more thought (ref SPEC-self-evolution §19 Q6).
10. **Consensus voting with disagreeing LLMs** — If three members return structurally different outputs, what does `VotingStrategy::Majority` even mean? Candidate: require a structured output type (§2) and vote on the serialized form field-by-field. Needs design.
11. **Cross-Circle federation** — The `wasteland` DoltHub fork/sync pattern (SPEC-dependencies §10.4) suggests how cross-organization Circle coordination could work. Not in scope for SERA 1.0 but worth a follow-up ADR.
