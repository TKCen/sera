# sera-domain Crate Documentation

> Documentation for SERA 2.0 domain types — the shared type definitions shared across all SERA crates.
> Matches BYOH contract schemas and the full sera-core domain model.

## Overview

The `sera-domain` crate (published as `sera-types`) contains all shared type definitions for SERA 2.0. It is a leaf crate with no internal dependencies, making it safe to use from any other crate in the workspace.

## Module Map

| Module | File | Purpose |
|--------|------|---------|
| `principal` | `principal.rs` | Identity for any acting entity (human, agent, service, system) |
| `event` | `event.rs` | The unit of work flowing through the gateway |
| `tool` | `tool.rs` | Tool definitions, schemas, execution, and policies |
| `memory` | `memory.rs` | RecallSignals, DreamingScore, MemoryBackend trait |
| `session` | `session.rs` | SessionStateMachine, transcript, content blocks |
| `runtime` | `runtime.rs` | AgentRuntime trait, TurnContext, runtime capabilities |
| `model` | `model.rs` | ModelAdapter, LLM client types, provider configuration |
| `queue` | `queue.rs` | QueueBackend trait, queue operations |
| `connector` | `connector.rs` | ChannelConnector, inbound/outbound routing |
| `observability` | `observability.rs` | Tracing, metrics, audit backends |
| `skill` | `skill.rs` | Skill system for capability discovery |
| `hook` | `hook.rs` | In-process hook registry and chain executor |
| `audit` | `audit.rs` | Audit trail definitions |
| `agent` | `agent.rs` | Agent instance management |
| `manifest` | `manifest.rs` | AgentTemplate, YAML manifest loading |
| `config_manifest` | `config_manifest.rs` | K8s-style config with secret resolution |
| `capability` | `capability.rs` | CapabilityPolicy definitions |
| `policy` | `policy.rs` | Tier policies, sandbox boundaries |
| `sandbox` | `sandbox.rs` | Sandbox tier info, status tracking |
| `secrets` | `secrets.rs` | Secret management types |
| `metering` | `metering.rs` | Usage tracking, budgets |
| `chat` | `chat.rs` | Chat messages, tool calls, agent actions |
| `content_block` | `content_block.rs` | ConversationMessage, role types |
| `envelope` | `envelope.rs` | Submission, Op, EventMsg, approval types |
| `harness` | `harness.rs` | AgentHarness trait, plugin system |
| `evolution` | `evolution.rs` | Self-improvement types |
| `versioning` | `versioning.rs` | BuildIdentity for version tracking |
| `intercom` | `intercom.rs` | Inter-process communication |

## Core Types

### Principal — Identity Model

```rust
pub enum PrincipalKind {
    Human,           // Human operator via CLI, TUI, or Web UI
    Agent,           // Agent instance in container or in-process
    ExternalAgent,  // External agent via A2A or ACP protocol
    Service,        // API integrations, webhooks, connectors
    System,         // SERA system itself
}

pub struct PrincipalId(String);

pub struct Principal {
    pub id: PrincipalId,
    pub kind: PrincipalKind,
    pub display_name: String,
    // ... more fields
}
```

Every acting entity in SERA is a Principal, enabling uniform audit trails and authorization checks. See MVS simplification: no groups, no external agents per mvs-review-plan §6.5.

### Event — Unit of Work

```rust
pub struct EventId(String);

pub struct Event {
    pub id: EventId,
    pub principal: PrincipalRef,
    pub channel: ChannelRef,
    pub content: ContentBlock,
    pub timestamp: DateTime<Utc>,
    // ... metadata
}
```

Events are the gateway's lingua franca: every inbound message, webhook, cron trigger, or system action is wrapped in an Event before entering the routing pipeline.

### Tool — Capability System

```rust
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub function: FunctionDefinition,
    pub metadata: ToolMetadata,
}

pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: FunctionParameters,
}

pub enum RiskLevel {
    Safe,
    Checked,
    Limited,
    Restricted,
    Dangerous,
}

pub enum ExecutionTarget {
    Local,       // Run locally (bash, python, etc.)
    Container,   // Run in sandboxed container
    External,   // Run via external service
}
```

Tools are the primary capability interface. RiskLevel determines what operations are allowed at each sandbox tier.

### Memory — Recall & Dreaming

```rust
pub trait MemoryBackend: Send + Sync {
    fn recall(&self, query: &str, limit: usize) -> impl Future<Output = Result<Vec<RecallSignal>, MemoryError>>;
    fn store(&self, signal: RecallSignal) -> impl Future<Output = Result<(), MemoryError>>;
    fn dreaming_score(&self) -> impl Future<Output = Result<DreamingScore, MemoryError>>;
    // ... compaction, stats
}

pub struct RecallSignal {
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub timestamp: DateTime<Utc>,
    pub importance: f32,
    pub source: String,
}

pub struct DreamingScore {
    pub score: f32,
    pub factors: Vec<String>,
    pub recommended_actions: Vec<String>,
}
```

The MemoryBackend trait defines the recall interface. RecallSignals are stored with embeddings for semantic search. DreamingScore determines when to trigger memory consolidation during idle periods.

### Session — State Machine

```rust
pub enum SessionState {
    New,
    PendingApproval,
    Running,
    Paused,
    AwaitingHITL,
    Completed,
    Failed,
}

pub struct SessionStateMachine {
    pub current: SessionState,
    pub history: Vec<SessionTransition>,
}

pub struct TranscriptEntry {
    pub timestamp: DateTime<Utc>,
    pub role: ConversationRole,
    pub content: ContentBlock,
    pub tool_calls: Vec<ToolCall>,
}
```

SessionStateMachine manages 6-state workflow (see SPEC-session). TranscriptEntry tracks the full conversation history.

### Runtime — Agent Execution

```rust
pub trait AgentRuntime: Send + Sync {
    fn run_turn(&self, context: TurnContext) -> impl Future<Output = Result<TurnOutcome, RuntimeError>>;
    fn get_capabilities(&self) -> RuntimeCapabilities;
    fn health_status(&self) -> HealthStatus;
}

pub struct TurnContext {
    pub agent_id: AgentId,
    pub session_id: SessionId,
    pub input: ContentBlock,
    pub tools: Vec<ToolDefinition>,
    // ... context fields
}

pub enum TurnOutcome {
    Response(ContentBlock),
    ToolCalls(Vec<ToolCall>),
    ApprovalRequired(ApprovalDecision),
    Error(RuntimeError),
}
```

AgentRuntime is the core trait for agent execution. TurnContext flows through each reasoning loop.

### Model — LLM Interface

```rust
pub trait ModelAdapter: Send + Sync {
    fn complete(&self, prompt: &str, params: CompletionParams) -> impl Future<Output = Result<Completion, ModelError>>;
    fn stream(&self, prompt: &str, params: CompletionParams) -> impl Stream<Item = Result<Delta, ModelError>>;
    fn embed(&self, text: &str) -> impl Future<Output = Result<Vec<f32>, ModelError>>;
}
```

ModelAdapter abstracts LLM providers (OpenAI, Anthropic, Ollama, etc.) behind a uniform interface.

### Queue — Async Queue

```rust
pub trait QueueBackend: Send + Sync {
    fn enqueue(&self, task: QueueTask) -> impl Future<Output = Result<(), QueueError>>;
    fn dequeue(&self) -> impl Future<Output = Result<Option<QueueTask>, QueueError>>;
    fn ack(&self, task_id: &TaskId) -> impl Future<Output = Result<(), QueueError>>;
    fn nack(&self, task_id: &TaskId, requeue: bool) -> impl Future<Output = Result<(), QueueError>>;
}
```

QueueBackend provides async task queuing for background processing.

### Connector — Channel Routing

```rust
pub trait ChannelConnector: Send + Sync {
    fn send(&self, channel: &ChannelRef, message: &ContentBlock) -> impl Future<Output = Result<(), ConnectorError>>;
    fn receive(&self, channel: &ChannelRef) -> impl Future<Output = Result<Option<Event>, ConnectorError>>;
    fn subscribe(&self, channel: &ChannelRef) -> impl Future<Output = Result<Subscription, ConnectorError>>;
}
```

ChannelConnector routes messages to/from Discord, Slack, Telegram, etc.

### Observability — Tracing & Metrics

```rust
pub trait AuditBackend: Send + Sync {
    fn emit(&self, entry: AuditEntry) -> impl Future<Output = Result<(), AuditError>>;
    fn query(&self, filter: AuditFilter) -> impl Future<Output = Result<Vec<AuditRecord>, AuditError>>;
}
```

AuditBackend emits audit trails to storage backends.

### Hook — In-Process Extension

```rust
pub enum HookPoint {
    PrePrompt,
    PostPrompt,
    PreToolExecution,
    PostToolExecution,
    PreResponse,
    PostResponse,
    OnError,
    OnApprovalRequired,
}

pub trait Hook: Send + Sync {
    fn name(&self) -> &str;
    fn hook_point(&self) -> HookPoint;
    fn execute(&self, context: HookContext) -> impl Future<Output = HookResult>;
}

pub struct HookChain {
    pub hooks: Vec<Box<dyn Hook>>,
    pub hook_point: HookPoint,
}
```

Hooks provide in-process extension at defined points. When WASM lands, WasmHookAdapter will implement the same trait.

## Type Relationships

```
Principal
  ↑
  └── Event ← ChannelRef
               ↑
               └── Event ← ContentBlock
                              ↑
                              ├── ConversationMessage ← TranscriptEntry
                              |                    ↑
                              |                    └── Session (has many TranscriptEntry)
                              |
                              └── ToolCall → ToolDefinition → ToolPolicy → RiskLevel
                                                          ↓
                                                      SandboxTier
```

```
Session → SessionStateMachine
           ↑
           └── SessionState (6 states)

TurnContext → AgentRuntime.run_turn() → TurnOutcome
                  ↓
            RuntimeCapabilities ← ModelAdapter ← ModelProvider
                  ↓
            QueueBackend (background tasks)
                  ↓
            ChannelConnector (outbound)
```

## Usage Examples

### Creating an Event

```rust
use sera_types::{Event, EventId, PrincipalRef, ContentBlock, ChannelRef};

let event = Event {
    id: EventId::generate(),
    principal: PrincipalRef::new("user-123"),
    channel: ChannelRef::new("discord", "channel-456"),
    content: ContentBlock::text("Hello, agent!"),
    timestamp: Utc::now(),
    // ... metadata
};
```

### Defining a Tool

```rust
use sera_types::{ToolDefinition, FunctionDefinition, FunctionParameters, RiskLevel, ExecutionTarget};

let tool = ToolDefinition {
    name: "bash".to_string(),
    description: "Execute a bash command".to_string(),
    function: FunctionDefinition {
        name: "bash".to_string(),
        description: "Execute a bash command in the sandbox".to_string(),
        parameters: FunctionParameters {
            strict: true,
            schema: ParameterSchema {
                // JSON Schema for parameters
            },
        },
    },
    metadata: ToolMetadata {
        risk_level: RiskLevel::Limited,
        execution_target: ExecutionTarget::Container,
        timeout_ms: 30_000,
        // ... more
    },
};
```

### Implementing MemoryBackend

```rust
use sera_types::{MemoryBackend, RecallSignal, MemoryError};
use async_trait::async_trait;

struct InMemoryStore {
    signals: Vec<RecallSignal>,
    recall_index: Vec<(String, Vec<f32>)>,
}

#[async_trait]
impl MemoryBackend for InMemoryStore {
    async fn recall(&self, query: &str, limit: usize) -> Result<Vec<RecallSignal>, MemoryError> {
        // Semantic search implementation
        Ok(self.signals.iter().take(limit).cloned().collect())
    }

    async fn store(&self, signal: RecallSignal) -> Result<(), MemoryError> {
        self.signals.push(signal);
        Ok(())
    }

    async fn dreaming_score(&self) -> Result<DreamingScore, MemoryError> {
        Ok(DreamingScore {
            score: 0.5,
            factors: vec!["recent activity".to_string()],
            recommended_actions: vec![],
        })
    }
}
```

## Feature Flags

| Feature | Description |
|---------|--------------|
| `default` | Core types only |
| `db` | PostgreSQL/sqlx support |
| `k8s` | K8s specific helpers |
| `partial` | Enable partial JSON deserialization |

## Versioning

BuildIdentity tracks the version of each component:

```rust
use sera_types::BuildIdentity;

let build = BuildIdentity {
    version: env!("CARGO_PKG_VERSION"),
    commit: env!("VERGEN_GIT_SHA"),
    timestamp: env!("VERGEN_BUILD_TIMESTAMP"),
};
```

## Related Documentation

- [Rust Migration Plan](../plan/RUST-MIGRATION-PLAN.md)
- [MVS Review Plan](../plan/mvs-review-plan.md)
- [Architecture](../ARCHITECTURE.md)
- [Self-Improvement Epic](../epics/30-closed-loop-self-improvement.md)