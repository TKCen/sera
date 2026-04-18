# SPEC: Agent Runtime (`sera-runtime`)

> **Status:** DRAFT
> **Source:** PRD §4.2, §13 (AgentRuntimeService proto), §14 (invariant 15), plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §10.1 (claw-code `ContentBlock`), §10.2 (Codex `Op::UserTurn`, `NextStep`, `DynamicToolSpec`, two-mode compaction, five hook points), §10.5 (openclaw `AgentHarness.supports()`, `ContextEngine` as distinct axis, compaction checkpoint reason discriminant), §10.6 (hermes-agent parser registry + two-tier normalization + reasoning extraction + `extra_body` passthrough), §10.7 (opencode `TurnOutcome`, `CorrectedError`, doom-loop threshold, `task_id` subagent resumption, `Tool.Context::ask()` inline), §10.10 (OpenHands `PipelineCondenser`, three-tier Microagents, `SecurityAnalyzer` trait), §10.13 (openai-agents-python `Agent` field inventory, handoff-as-tool, two-level hook lifecycle, guardrails concurrent with LLM, `tool_use_behavior` discriminated union, `is_enabled`/`needs_approval` callbacks, `Session` protocol), §10.15 (MetaGPT `Action` vs `Tool`, `cause_by`, `react_mode`, four-method role lifecycle), §10.16 (BeeAI four-tier memory ABC), §10.17 (CAMEL `TaskSpecifier` pre-pass, `validate_task_content` failure-pattern blacklist, `SystemMessageGenerator` keyed on `TaskType`), [SPEC-self-evolution](SPEC-self-evolution.md) §5.5 `ShadowSession` replay mode
> **Crate:** `sera-runtime`
> **Priority:** Phase 2

---

## 1. Overview

The agent runtime is the **worker that does the "thinking + doing."** It receives a dequeued event plus session context from the gateway, assembles a context window, calls the model, processes tool calls, writes memory, and delivers the response.

The runtime is **isolated, stateless per-turn, and session-scoped.** It does not own durable state — it reads from and writes to owned subsystems (memory, session transcript, tools).

The **default runtime** is a highly configurable pipeline shipped with SERA. **External runtimes** (e.g., Python-hosted, domain-specific) implement the `AgentRuntimeService` gRPC service and register with the gateway.

---

## 1a. Library Crate vs Binary — Deployment Modes

> **Design decision — 2026-04-13.** The runtime ships in two forms. These are **not** different architectures; they are the same crate compiled with different entry points.

### Library mode (default — embedded in gateway)

`sera-runtime` is a **library crate** that the gateway calls directly via a Rust function call. In this mode:

- There is **no process boundary** — no gRPC hop, no serialization overhead, no IPC latency
- The gateway allocates a `TurnContext`, calls `runtime.execute_turn(ctx)`, and receives a `TurnOutcome`
- Tool call events emitted by the runtime are handled by the gateway in the same async task via a channel — there is no network call
- This is the path for all standard local and enterprise deployments

### Binary mode (standalone / BYOH reference)

The same crate compiles as an executable for two use cases:

1. **Pet / standalone mode** — `sera start` with a minimal config; the runtime binary manages its own file-based memory and workspace. No database, no external services required.
2. **BYOH reference implementation** — The binary demonstrates the ACP/gRPC harness protocol so third-party runtime authors (Claude Code, Codex, Hermes) can implement compatible BYOH workers. The binary connects to the gateway via the `Grpc` or `WebSocket` `AppServerTransport` variant (see SPEC-gateway §7a).

### What the runtime does and does NOT do

The runtime's **only responsibilities** are:

1. Receive session context injected by the gateway (soul/persona, memory, tool schemas)
2. Run the turn loop — call the LLM, read tool call requests from the response
3. Forward tool call events to the gateway and wait for results
4. Return the turn result (`TurnOutcome`) to the gateway

The runtime **does NOT**:

- Execute tools — tool dispatch, AuthZ, and execution are gateway responsibilities (see SPEC-tools §6 and SPEC-gateway §2)
- Hold credentials — the gateway resolves credentials and injects results; the runtime sees only tool outputs
- Know the network topology — the runtime receives injected context; it does not know where memory is stored, which sandbox provider is active, or which LLM provider is in use
- Own durable state — all session state, memory, and audit records live at the gateway; the runtime is stateless between turns

This design is what makes runtimes "cattle" — they can be restarted, cloned, or replaced without data loss. All durable state is gateway-owned.

---

## 2. Agent + Runtime Traits

SERA's runtime layer has two orthogonal axes, each with its own trait. This follows the openclaw pattern (SPEC-dependencies §10.5) where harness selection (turn loop) and context assembly are **separate, independently pluggable slots** — replacing the context engine does not require replacing the harness.

### 2.1 `Agent` field inventory

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.13 openai-agents-python `Agent` dataclass.

```rust
pub struct Agent<TContext> {
    pub name: String,
    pub handoff_description: Option<String>,
    pub instructions: Instructions,                 // static string OR dynamic callable with context
    pub prompt: Option<Prompt>,                     // optional template (distinct from instructions)
    pub handoffs: Vec<Handoff<TContext>>,           // handoff-as-tool targets (§9)
    pub model: Option<ModelRef>,
    pub model_settings: ModelSettings,
    pub input_guardrails: Vec<InputGuardrail<TContext>>,
    pub output_guardrails: Vec<OutputGuardrail<TContext>>,
    pub output_type: Option<OutputSchema>,           // structured output constraint (schemars-derived)
    pub hooks: Option<AgentHooks<TContext>>,         // per-agent hooks (§7.2)
    pub tools: Vec<Tool>,
    pub mcp_servers: Vec<McpServerRef>,              // MCP tools re-fetched per turn (§6.2)
    pub mcp_config: McpConfig,
    pub tool_use_behavior: ToolUseBehavior,          // discriminated union (§6.3)
    pub reset_tool_choice: bool,
    pub capabilities: HashSet<AgentCapability>,      // includes Delegation → injects DelegateWorkTool per SPEC-dependencies §10.14 CrewAI
    pub react_mode: ReactMode,                        // per-role: React | ByOrder | PlanAndAct (SPEC-dependencies §10.15 MetaGPT)
    pub watch_signals: HashSet<ActionId>,            // declarative subscription by cause_by (§9a)
}

pub enum Instructions {
    Static(String),
    Dynamic(Box<dyn Fn(&RunContextWrapper, &Agent<TContext>) -> BoxFuture<'static, String> + Send + Sync>),
}

pub enum ReactMode {
    React,           // LLM picks next action each turn (default)
    ByOrder,         // Sequential through tools[] (deterministic)
    PlanAndAct,      // LLM builds a plan first, then executes steps via a planner
}
```

### 2.2 `AgentRuntime` / Harness trait

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.5 openclaw + §10.13 openai-agents-python `Runner`.

```rust
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    fn id(&self) -> HarnessId;
    fn label(&self) -> &str;

    /// Capability-negotiated harness selection. Returns whether this runtime can handle the
    /// given context and at what priority. Gateway ranks registered harnesses by priority.
    async fn supports(&self, ctx: &HarnessSupportContext) -> HarnessSupport;

    /// Execute one complete turn. Returns a typed NextStep outcome so the gateway can decide
    /// whether to compact, stop, run again, or pause for HITL.
    async fn execute_turn(&self, ctx: TurnContext) -> Result<TurnOutcome, RuntimeError>;

    async fn capabilities(&self) -> RuntimeCapabilities;
    async fn health(&self) -> HealthStatus;

    /// Optional lifecycle hooks.
    async fn reset(&self, params: ResetParams) -> Result<(), RuntimeError> { Ok(()) }
    async fn dispose(self: Box<Self>) -> Result<(), RuntimeError> { Ok(()) }
}
```

### 2.3 `TurnOutcome` — the turn-evaluation return type

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.7 opencode `Result = "compact" | "stop" | "continue"` + §10.13 openai-agents-python `NextStep`.

```rust
pub enum TurnOutcome {
    /// Continue the turn loop with tool results re-fed into the model.
    RunAgain,

    /// Delegate to a sub-agent via handoff-as-tool-call (§9).
    Handoff {
        target: Agent<TContext>,
        filtered_input: HandoffInputData,
    },

    /// Final assistant output ready for delivery. Carries typed structured output if `agent.output_type` is set.
    FinalOutput {
        content: FinalContent,
        typed: Option<Box<dyn Any + Send + Sync>>,
    },

    /// Compaction requested (opencode pattern: compaction is a first-class turn outcome,
    /// not an implicit side effect). Gateway schedules compaction via the Condenser pipeline (§5).
    Compact {
        trigger: CompactionTrigger,
        preserve_recent: usize,
    },

    /// HITL pause — turn is suspended until an approval response arrives via SQ.
    Interruption {
        approval_id: ApprovalId,
        risk: ActionSecurityRisk,
        reason: String,
    },

    /// Stop without a final output (error, cancellation, or explicit stop).
    Stop { reason: StopReason },
}
```

### 2.4 `ContextEngine` — the separately pluggable context assembly trait

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.5 openclaw `ContextEngine` — distinct from harness.

Context assembly is **orthogonal** to the runtime. A plugin can replace the context engine without touching the turn loop. An agent declares its context engine separately from its harness:

```rust
#[async_trait]
pub trait ContextEngine: Send + Sync {
    async fn bootstrap(&self, params: BootstrapParams) -> Result<BootstrapResult, ContextError>;
    async fn ingest(&self, params: IngestParams) -> Result<IngestResult, ContextError>;

    /// Primary entry point: produce the context window for a turn.
    async fn assemble(&self, ctx: &TurnContext) -> Result<ContextWindow, ContextError>;

    /// Pluggable compaction (distinct from turn-level CompactionStrategy in §5).
    async fn compact(&self, params: CompactParams) -> Result<CompactResult, ContextError>;

    async fn maintain(&self, params: MaintenanceParams) -> Result<MaintenanceResult, ContextError> {
        Ok(MaintenanceResult::NoOp)
    }

    async fn after_turn(&self, params: AfterTurnParams) -> Result<(), ContextError> { Ok(()) }

    fn describe(&self) -> EngineDescription;
}

pub struct ContextWindow {
    pub messages: Vec<Message>,
    pub estimated_tokens: u64,
    pub system_prompt_addition: Option<String>,
}
```

Both `AgentRuntime` and `ContextEngine` are registered independently via their respective registries. Per-agent configuration binds a runtime to a context engine at session boot.

### 2.5 `ContentBlock` — the message atom

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.1 claw-code — matches the Anthropic wire format and prevents ToolUse/ToolResult split bugs during compaction.

```rust
pub enum ContentBlock {
    Text(String),
    ToolUse { id: ToolCallId, name: String, input: serde_json::Value },
    ToolResult {
        tool_use_id: ToolCallId,
        tool_name: String,
        output: serde_json::Value,
        error: bool,
    },
}

pub struct ConversationMessage {
    pub role: MessageRole,            // System | User | Assistant | Tool
    pub content: Vec<ContentBlock>,
    pub usage: Option<TokenUsage>,
    pub cause_by: Option<ActionId>,   // MetaGPT routing key per §9a
}
```

**Compaction invariant:** the compaction pipeline NEVER splits a `ToolUse` block from its paired `ToolResult`. If the `ToolUse` would be removed, both are kept or both are removed. This is a hard Anthropic API requirement.

---

## 3. Turn Loop (Default Runtime)

The turn loop follows a **four-method lifecycle** adapted from MetaGPT's Role pattern (SPEC-dependencies §10.15):

```
                            ┌─ _observe ─┐
                            │            │
                            ↓            │
session event +    → content-addressed   │
turn context      filter by cause_by     │
                  ∈ watch_signals         │
                            │            │
                            ↓            │
                     _think (LLM call)   │
                      or deterministic   │
                      if react_mode      │ (up to max_react_loop,
                      = ByOrder          │  subject to cost bounds §5.5)
                            │            │
                            ↓            │
                      _act (run tool)    │
                            │            │
                            ↓            │
                     _react decides:     │
                     RunAgain → top ─────┘
                     Handoff / Compact / FinalOutput / Interruption / Stop
```

Every phase boundary fires a hook chain:

```
Event + Session Context
  → constitutional_gate hook chain (fail-closed, see SPEC-hooks)
  → pre_turn hook chain
  → input_guardrails (run CONCURRENTLY with the LLM call — see §7.3)
  → Context Engine::assemble() [pluggable, see §2.4]
  → [on_llm_start hook]
  → Model Call (via parser registry §5.4 for non-native tool-call formats)
  → [on_llm_end hook]
  → [Tool Call Loop]
      → pre_tool hook chain (may call .ask() inline for approval — SPEC-dependencies §10.7)
      → SecurityAnalyzer::security_risk(action) — SPEC-dependencies §10.10
      → is_enabled callback check — SPEC-dependencies §10.13
      → Tool Execution (with turn_id + call_id scoping per SPEC-dependencies §10.2)
      → post_tool hook chain
      → Tool results re-enter model
      → Doom-loop threshold check (DOOM_LOOP_THRESHOLD = 3, SPEC-dependencies §10.7)
  → _react determines TurnOutcome (§2.3)
  → output_guardrails (sequential, after final output)
  → post_turn hook chain
  → Memory Write Pipeline
      → pre_memory_write hook chain
      → Backend write
  → pre_deliver hook chain
  → Deliver Response
  → post_deliver hook chain
```

The tool call loop repeats until `TurnOutcome::FinalOutput`, `Compact`, `Handoff`, `Interruption`, or `Stop` is returned — or a cost bound is hit (§5.5). `RunAgain` loops back.

### 3.1 Doom-loop detection

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.7 opencode.

A `DoomLoopDetector` tracks repeated identical tool calls within the turn loop. When `DOOM_LOOP_THRESHOLD` (default 3) is reached, the runtime does NOT hard-fail — it emits a `doom_loop` permission check to the HITL chain, letting the user observe and intervene. Maps to `TurnOutcome::Interruption` with `ActionSecurityRisk::Medium`.

### 3.2 Per-turn policy overrides

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.2 Codex `Op::UserTurn`.

Per-turn policy fields (approval policy, sandbox policy, model override, cwd, final output schema) are carried on the `Op::UserTurn` submission (see SPEC-gateway §3.1) and applied for the duration of that turn only. They do NOT mutate session-level state. This enables per-request policy scoping without session mutation.

### 3.3 Shadow-session replay mode

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §5.5, §11.

When the gateway marks a session as `SessionState::Shadow`, the runtime replays the captured event stream against the current (proposed) config/binary without mutating durable state. Replay uses the same turn loop as live execution, but:

- No events are written to the live `EventStream`
- Memory writes go to a scratch store that is discarded at the end of the dry-run
- Tool execution is gated through a `ShadowSandboxProvider` that mocks side effects
- The final comparison (diff against the recorded output) populates the `DryRunResult` on the originating Change Artifact

This is the foundation for the Tier-2 and Tier-3 dry-run gates.

---

## 4. Context Assembly Pipeline (KV Cache Optimized)

> [!IMPORTANT]  
> Context assembly **must be ordered to maximize KV cache prefix hits** across turns within a session. Segments that remain stable across turns are placed at the **front** (prefix). Segments that change per-turn are placed at the **tail**. This ensures LLM serving engines with prefix caching (vLLM, SGLang, TensorRT-LLM, etc.) reuse KV cache from previous turns, reducing time-to-first-token.

### 4.1 Pipeline Architecture

```rust
pub struct ContextPipeline {
    pub steps: Vec<Box<dyn ContextStep>>,
}

#[async_trait]
pub trait ContextStep: Send + Sync {
    fn name(&self) -> &str;
    /// Position hint for KV cache optimization — lower = more stable = placed earlier
    fn stability_rank(&self) -> u32;
    async fn execute(&self, ctx: &mut TurnContext, hooks: &HookChain) -> Result<(), PipelineError>;
}
```

The pipeline **sorts steps by `stability_rank()`** before assembly, ensuring optimal prefix sharing even with custom steps.

### 4.2 Default Step Ordering

| Order | Step | Stability | Hookable | Rationale |
|---|---|---|---|---|
| 1 | Persona Injection | 🟢 Stable | ✅ `context_persona` | System prompt, personality — rarely changes within a session |
| 2 | Tool Injection | 🟢 Stable | ✅ `context_tool` | Available tool schemas — changes only on policy updates |
| 3 | Skill Injection | 🟡 Semi-stable | ✅ `context_skill` | Active skills, mode context — changes on mode transition |
| 4 | Memory Injection | 🟡 Semi-stable | ✅ `context_memory` | Long-term memory excerpts — changes on memory writes |
| 5 | History Injection | 🔴 Volatile | ✅ | Session transcript (sliding window) — grows each turn |
| 6 | Current Turn | 🔴 Volatile | ✅ | Current user message and dynamic context |
| 7 | Custom Steps | Configurable | ✅ | User-defined enrichment — stability hint configurable |

### 4.3 Persona Architecture

> **Enhancement: OpenSwarm §4, v3 §5**

The persona injection step (step 1) supports a structured persona format with distinct sections:

```rust
pub struct PersonaConfig {
    /// Core safety directives, foundational identity, behavioral boundaries.
    /// The system CANNOT modify this section. Administered by operators only.
    pub immutable_anchor: String,

    /// Adaptable behavioral traits, tone, style, domain expertise.
    /// The agent CAN propose modifications to this section (via config_propose).
    pub mutable_persona: String,

    /// Maximum token budget for the mutable_persona section.
    /// When exceeded, an introspection workflow is triggered.
    pub mutable_token_budget: u32,
}
```

**Immutable Anchor:** Contains non-negotiable directives — safety boundaries, core identity, operational constraints. Only modifiable by operators via `config_propose` with admin authorization.

**Mutable Persona:** Contains the agent's evolving personality, domain expertise, learned behavioral preferences. The agent can self-edit this section via `config_propose`, subject to authorization policy.

**User Soul (Preference Learning):** The agent progressively learns user preferences (technical proficiency, preferred stacks, communication style) and stores them in its memory wiki. A `context_persona` hook (or the persona injection step itself) can inject relevant user-specific preferences from memory, eliminating repetitive prompting.

**Introspection Loop:** When the mutable persona exceeds its token budget, a scheduled workflow triggers an "introspection" task. A specialized prompt analyzes the overflowing traits, resolves contradictions, and condenses them into unified abstract behavioral principles. This prevents persona fragmentation over long agent lifetimes.

```yaml
agents:
  - name: "sera"
    persona:
      immutable_anchor: |
        You are Sera, an autonomous assistant. You never reveal secrets.
        You always cite sources. You ask for clarification when uncertain.
      mutable_persona: |
        You prefer concise technical answers. You favor Rust over Python.
        You use dry humor occasionally.
      mutable_token_budget: 300
      introspection:
        enabled: true
        trigger: "token_overflow"      # or "scheduled"
```

### 4.4 Custom Steps

Operators and agents (with permission) can define custom context steps that plug into the pipeline. Each custom step declares its own `stability_rank()` so it slots into the correct position for KV cache optimization.

### 4.5 Pipeline Ownership

The context pipeline is **per-agent configurable** via the config system. An agent can modify its own pipeline definition at runtime via config tools, subject to authorization policy. This supports the self-bootstrapping story — an agent can propose adding a custom context step.

---

## 5. Model Call

The runtime calls the model via the model adapter trait or gRPC:

```rust
#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionStream, ModelError>;
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ModelError>;
    async fn health(&self) -> HealthStatus;
}
```

In the simplified architecture, **all model providers are accessed via gRPC** — even local ones. This provides a uniform interface regardless of provider type.

### 5.1 Structured / Constrained Generation

> **Enhancement: OpenSwarm §2 (Grammar Constraints & Semantic Validation)**

Model adapters **MUST** support structured output where the underlying provider allows it. Structured generation ensures agent output conforms to a schema rather than relying on hope.

```rust
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    pub parameters: ModelParameters,
    /// Optional: constrain model output to conform to a JSON schema.
    /// Provider adapters translate this to the appropriate mechanism:
    /// - OpenAI/Gemini: `response_format` with JSON schema
    /// - vLLM/SGLang: guided decoding / structured output API
    /// - llama.cpp: GBNF grammar constraint
    pub structured_output: Option<StructuredOutputConfig>,
}

pub enum StructuredOutputConfig {
    /// JSON schema — model output must be valid JSON matching this schema
    JsonSchema(serde_json::Value),
    /// Grammar constraint — model output must match this grammar (GBNF format)
    Grammar(String),
    /// Enum — model output must be one of these values
    Enum(Vec<String>),
}
```

**Dual validation** is applied to structured output:
1. **Grammatical validation** — does the output parse as valid JSON matching the schema?
2. **Semantic validation** — does the output satisfy the acceptance criteria for the task? (Evaluated via post_turn hooks or a reviewer agent.)

If structured output fails validation, the runtime can retry the model call (up to a configurable retry limit) before failing the turn.

### 5.2 Multi-Model Task Routing

> **Enhancement: Strategic Rearchitecture §oh-my ecosystem**

Each agent has a **default model**, but the model can be overridden per-skill or per-task to route different work to specialized models.

```yaml
agents:
  - name: "sera"
    provider: "lm-studio"
    model: "gemma-4-12b"                 # Default model
    model_routing:
      coding:
        provider: "fast-local"
        model: "qwen-7b"                 # Fast, cheap for code generation
      architecture:
        provider: "api"
        model: "gemini-2.5-pro"           # Deep reasoning for design
      visual:
        provider: "api"
        model: "gemini-2.5-flash"         # Multimodal for UI work
```

**Resolution order:**
1. Skill-specific model override (if the active skill has a model binding)
2. Task-classified model routing (if `model_routing` config matches the task type)
3. Agent default model

A `pre_turn` hook can also override the model selection dynamically based on runtime context (e.g., a hook that classifies the incoming message and selects the appropriate model).

### 5.3 Dynamic Model Parameters

> **Enhancement: OpenSwarm v3 §3 (Dynamic Parameter Heuristics & Entropy Kicker)**

Model inference parameters are **not globally static**. The system supports sampler profiles bound to specific skills or task types.

```rust
pub struct ModelParameters {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u32>,
    pub min_p: Option<f64>,
    pub repetition_penalty: Option<f64>,
    pub max_tokens: Option<u32>,
    pub stop_sequences: Vec<String>,
}
```

**Sampler profiles** per skill:

```yaml
agents:
  - name: "sera"
    model_parameters:                    # Default parameters
      temperature: 0.7
      top_p: 0.9
    sampler_profiles:
      coding:
        temperature: 0.2
        top_p: 0.95
        min_p: 0.05                      # Strict, deterministic
      creative_writing:
        temperature: 0.9
        repetition_penalty: 1.1          # High entropy, creative
      wiki_consolidation:
        temperature: 0.3
        max_tokens: 2000                 # Concise summaries
```

**Entropy Kicker:** When an agent enters a failure loop (e.g., repeated tool call failures, repeated identical outputs), a `pre_turn` hook can dynamically increase `temperature` and `repetition_penalty` to "shake" the model out of localized logic traps. This is implementable as a hook without core changes — the hook reads the failure count from session metadata and adjusts parameters accordingly.

### 5.4 gRPC Service (for external providers)

```protobuf
service ModelProviderService {
    rpc Complete(CompletionRequest) returns (stream CompletionChunk);
    rpc ListModels(Empty) returns (ModelList);
    rpc Health(Empty) returns (HealthResponse);
}
```

---

### 5.5 Harness Patterns for Long-Running Turns

> **Enhancement: Anthropic Harness Design, Effective Harnesses**

Long-running agent turns (multi-step tool chains spanning dozens of tool calls) require production hardening beyond simple iteration limits.

#### Turn-Level Cost Bounds

The runtime enforces configurable cost budgets per turn:

```yaml
agents:
  - name: "sera"
    runtime:
      cost_bounds:
        max_tokens_per_turn: 50000     # Total tokens (prompt + completion) per turn
        max_tool_calls_per_turn: 25    # Maximum tool invocations per turn
        on_budget_exceeded: "summarize" # summarize | abort | warn
```

When a budget is approaching its limit, the runtime injects a system message advising the agent to wrap up. When exceeded, behavior depends on `on_budget_exceeded`:
- **summarize**: Inject a "summarize your progress" instruction and stop the tool loop
- **abort**: Fail the turn with a budget error
- **warn**: Log a warning but continue

#### Tool Result Filtering

Large tool results (e.g., file contents, search results, command output) bloat the context window. The runtime applies configurable truncation:

```yaml
agents:
  - name: "sera"
    runtime:
      tool_results:
        max_result_chars: 8000        # Truncate tool results exceeding this size
        truncation_strategy: "tail"   # head | tail | middle | summarize
        summarize_prompt: null        # Custom prompt for summarize strategy
```

The `post_tool` hook chain can also filter or transform tool results before they re-enter the context.

#### Turn Heartbeat

During long tool loops, the runtime emits periodic heartbeat events to the gateway so clients can display progress indicators:

```rust
pub struct TurnHeartbeat {
    pub turn_id: TurnId,
    pub status: TurnStatus,           // Working, WaitingForTool, WaitingForApproval
    pub tool_calls_completed: u32,
    pub current_tool: Option<ToolRef>,
    pub elapsed: Duration,
    pub tokens_used: u64,
}
```

Heartbeat interval is configurable (default: 5 seconds).

#### Turn Checkpointing

For crash recovery during long-running turns, the runtime can optionally checkpoint progress after each tool call:

- **Checkpoint data:** Session transcript up to current tool call, completed tool results, accumulated state
- **Storage:** Written to the session's durable state (via `sera-db`)
- **Resume:** On crash recovery, the runtime can resume from the last checkpoint instead of replaying the entire turn
- **Cleanup:** Checkpoints are garbage-collected when the turn completes successfully

```yaml
agents:
  - name: "sera"
    runtime:
      checkpointing:
        enabled: false                 # Opt-in, disabled by default
        interval: "per_tool_call"      # per_tool_call | every_n_calls
        retention: "last_turn"         # last_turn | last_n_turns
```

> [!NOTE]
> Checkpointing is a Phase 3+ reliability feature. The design ensures the runtime can adopt it without breaking changes.

---

## 6. Tool Call Loop

When the model returns tool calls:

1. Resolve tool from `sera-tools` registry
2. Execute `pre_tool` hook chain (risk checks, approval gates, argument validation, secret injection)
3. Check authorization via `sera-auth` (can this principal/agent run this tool in this context?)
4. If `NeedsApproval` → route to `sera-hitl`, suspend turn (session enters `WaitingForApproval`)
5. Generate or use provided **idempotency key** for the tool call
6. Execute tool
7. Execute `post_tool` hook chain (result sanitization, audit, risk assessment, PII tokenization)
8. Tool results re-enter the model
9. **Check steer queue** — if a `steer` message is queued, skip remaining tool calls and inject user message

### 6.1 Idempotency and Retry Boundaries

> **Enhancement: OpenClaw Part 6, Anthropic Effective Harnesses**

Retries must be narrow (per-step), not broad (per-turn). Replaying an entire multi-step flow risks repeating completed non-idempotent side effects.

```rust
pub struct ToolCallMetadata {
    pub idempotency_key: Option<String>,  // Unique key for this specific call
    pub attempt: u32,                     // Current retry attempt
    pub max_retries: u32,                 // Per-tool retry limit
    pub completed: bool,                  // Has this call completed successfully?
}
```

**Rules:**
- Each tool call carries an optional `idempotency_key`. If provided, the tool registry checks whether this key has already been executed successfully.
- On turn retry (e.g., after crash recovery), the runtime skips tool calls whose idempotency keys are marked as completed.
- Retry policy is configurable per-tool: `retries: 0` (no retry) to `retries: 3` (up to 3 attempts with exponential backoff).

```yaml
tools:
  web_search:
    retries: 2
    retry_backoff_ms: 1000
  file_write:
    retries: 0                         # Never retry writes
    idempotency: "required"            # Must provide idempotency key
```

---

## 6a. Compaction Pipeline (new)

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.10 OpenHands `PipelineCondenser` — the most complete compaction architecture in the research set — plus §10.5 openclaw `SessionCompactionCheckpoint` reason discriminant.

Compaction is a **composable pipeline**, not a monolithic strategy. The `Condenser` trait returns either a transparent new `View` (compaction applied invisibly) or a `Condensation` event (first-class, replayable via the EventStream).

### 6a.1 Trait

```rust
#[async_trait]
pub trait Condenser: Send + Sync {
    fn name(&self) -> &str;

    /// Condense a view of the session history. Returns either a new view (pass-through) or
    /// a Condensation signaling the controller to emit a first-class CondensationAction event.
    async fn condense(&self, view: View) -> Result<CondenseOutcome, CondenseError>;
}

pub enum CondenseOutcome {
    View(View),                      // Pass through (possibly modified)
    Condensation(Condensation),      // First-class event — emits into the EventStream
}

pub struct Condensation {
    pub forgotten_event_ids: Option<Vec<EventId>>,   // OR forgotten range (exclusive choice)
    pub forgotten_range: Option<(EventId, EventId)>,
    pub summary: Option<String>,
    pub summary_offset: Option<usize>,               // Insertion point for the summary
    pub injection_mode: InitialContextInjection,
    pub reason: CheckpointReason,                    // Openclaw discriminant
}

pub enum InitialContextInjection {
    /// Pre-turn / manual: replace history + clear reference context; initial context re-injected next turn
    DoNotInject,

    /// Mid-turn: model is trained to see the summary just above the last user message
    BeforeLastUserMessage,
}

pub enum CheckpointReason {
    Manual,
    AutoThreshold,
    OverflowRetry,
    TimeoutRetry,
}
```

### 6a.2 Built-in implementations

SERA ships nine built-in condensers, directly modeled on OpenHands:

| Condenser | Strategy | Use case |
|---|---|---|
| `NoOpCondenser` | Pass-through | Disabled / debugging |
| `RecentEventsCondenser` | Sliding window by count | Default cheap option |
| `ConversationWindowCondenser` | Sliding window by token budget | Most common default |
| `AmortizedForgettingCondenser` | Probabilistic dropping | Long sessions with lots of low-value history |
| `ObservationMaskingCondenser` | Masks verbose observation bodies | Code/data-heavy tool outputs |
| `BrowserOutputCondenser` | Truncates browser tool output specifically | Web-browsing agents |
| `LLMSummarizingCondenser` | LLM call produces a structured summary; tracks `USER_CONTEXT` and `TASK_TRACKING` sections explicitly across compactions | Long multi-turn tasks |
| `LLMAttentionCondenser` | Attention-scored event selection | Very long contexts with selective recall needs |
| `StructuredSummaryCondenser` | Structured JSON summary with typed sections | Downstream machine consumption |

### 6a.3 `PipelineCondenser` — composition

```rust
pub struct PipelineCondenser {
    pub stages: Vec<Box<dyn Condenser>>,
}
```

Stages are applied in order. Each stage receives the output of the previous stage. The pipeline short-circuits on the first `CondenseOutcome::Condensation` (which emits into the EventStream and replaces history).

### 6a.4 Compaction checkpoint model

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.5 openclaw — directly reusable.

Every compaction operation produces a **checkpoint** with dual transcript references for reversibility:

```rust
pub struct CompactionCheckpoint {
    pub checkpoint_id: CheckpointId,
    pub session_key: SessionKey,
    pub reason: CheckpointReason,
    pub pre_compaction: TranscriptRef,      // Points to the frozen pre-compaction transcript
    pub post_compaction: TranscriptRef,     // Points to the post-compaction transcript
    pub tokens_before: u64,
    pub tokens_after: u64,
    pub summary: Option<String>,
    pub created_at: DateTime<Utc>,
}
```

**Rolling window cap:** `MAX_COMPACTION_CHECKPOINTS_PER_SESSION = 25` — older checkpoints are GC'd.

### 6a.5 Agent-as-Summarizer continuation

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.1 claw-code.

When `LLMSummarizingCondenser` fires, the runtime injects a synthetic continuation message into the post-compaction session explaining the summarization to the LLM: *"This session was compacted — the summary above represents earlier context. Continue from here."* This prevents the LLM from being confused by a truncated context.

---

## 7. Subagent Management

The runtime supports **subagent spawning** — an agent can delegate subtasks to other agents. Subagent turns are:
- Scoped to the parent session or a child session (configuration-dependent)
- Subject to the same hook chains and authorization as regular turns
- Tracked in the audit trail with parent-child relationship

> [!NOTE]  
> Subagent management interacts with the Circle model (see [SPEC-circles](SPEC-circles.md)) for multi-agent coordination patterns.

---

## 8. External Runtime (gRPC)

```protobuf
service AgentRuntimeService {
    rpc ExecuteTurn(TurnRequest) returns (stream TurnEvent);
    rpc GetCapabilities(Empty) returns (RuntimeCapabilities);
    rpc Health(Empty) returns (HealthResponse);
}
```

External runtimes register with the gateway and can be assigned to specific agents via config. They receive the same `TurnContext` (serialized via protobuf) and must return `TurnEvent` streams compatible with the gateway's delivery pipeline.

---

## 9. Invariants

| # | Invariant | Enforcement |
|---|---|---|
| 15 | Context is KV-cache-optimized | Stable prefix, volatile tail — enforced by `stability_rank()` sort |
| 5 | Capability ≠ execution | Tool schemas exposed ≠ execution authorized — checked at tool call time |

---

## 9a. Action vs Tool

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.15 MetaGPT `Action`.

SERA distinguishes **Action** from **Tool**:

- **Tool** is a low-level callable (shell, HTTP, file I/O, MCP tool). Stateless, schema-driven, no LLM binding.
- **Action** is a typed, reusable unit of work that carries its own LLM binding, system prompt prefix, and structured output schema. An Action may invoke Tools internally. Actions are reused across Agents via `Agent::set_actions([WriteCode, WriteTest])`.

```rust
pub struct Action<TInput, TOutput> {
    pub name: ActionId,
    pub description: String,
    pub system_prompt_prefix: String,
    pub model_binding: Option<ModelRef>,         // Per-action model override
    pub output_schema: Option<schemars::Schema>, // Structured output via ActionNode pattern (MetaGPT)
    pub executor: Box<dyn ActionExecutor<TInput, TOutput>>,
}
```

**`cause_by` routing key.** Every `Message` carries `cause_by: Option<ActionId>` — the Action that produced it. Agents declare `watch_signals: HashSet<ActionId>` to subscribe to Messages caused by specific Actions. This enables declarative SOP composition without a central coordinator. See [SPEC-circles](SPEC-circles.md) for the multi-agent implications.

---

## 9b. Handoff-as-Tool-Call

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.13 openai-agents-python + §10.14 CrewAI `DelegateWorkTool`.

Sub-agent delegation is **first-class a tool call** — not framework-controlled routing. Every `Agent` in `handoffs[]` is wrapped into a `Handoff` object with a tool schema the LLM can see and call.

```rust
pub struct Handoff<TContext> {
    pub tool_name: String,                   // e.g. "transfer_to_billing_agent"
    pub tool_description: String,
    pub input_json_schema: serde_json::Value,
    pub on_invoke_handoff: Box<dyn HandoffCallback<TContext>>,
    pub input_filter: Option<HandoffInputFilter>,
}

/// Fully programmable context filter — strip, summarize, or rewrite history at the handoff boundary.
/// Matches openai-agents-python HandoffInputFilter shape and SERA's subagent_delivery_target hook.
pub type HandoffInputFilter = Box<dyn Fn(HandoffInputData) -> BoxFuture<'static, HandoffInputData> + Send + Sync>;

pub struct HandoffInputData {
    pub input_history: Vec<ConversationMessage>,
    pub pre_handoff_items: Vec<ContentBlock>,
    pub new_items: Vec<ContentBlock>,
}
```

The runner intercepts the tool call by name prefix (`transfer_to_*` convention), calls `on_invoke_handoff`, applies the optional `input_filter`, and switches the active agent. This is auditable in the normal tool-call trace — no special plumbing.

---

## 10. Hook Points

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.2 Codex five hook points + §10.13 openai-agents-python `on_llm_start/end`. SERA keeps its full 16-hook superset but aligns names with Codex for the overlapping subset.

| Hook Point | Fires When | Codex alignment |
|---|---|---|
| `constitutional_gate` | **Before every other hook on any Submission** — fail-closed, never `fail_open` | — |
| `session_start` | When a session enters `Created` | `SessionStart` |
| `pre_turn` | After queue dequeue, before context assembly | `UserPromptSubmit` |
| `context_persona` | During persona assembly step | — |
| `context_memory` | During memory injection step | — |
| `context_skill` | During skill injection step | — |
| `context_tool` | During tool injection step | — |
| `on_llm_start` | Immediately before the model call | — (new) |
| `on_llm_end` | Immediately after the model call, before tool dispatch | — (new) |
| `pre_tool` | Before tool execution | `PreToolUse` |
| `post_tool` | After tool execution | `PostToolUse` |
| `subagent_delivery_target` | Between subagent completion and parent session delivery (openclaw pattern, SPEC-dependencies §10.5) | — |
| `post_turn` | After runtime, before response delivery | `Stop` |
| `pre_deliver` | Before response delivery to client/channel | — |
| `post_deliver` | After response delivery confirmed | — |
| `pre_memory_write` | Before durable memory write | — |

**Two-level hook lifecycle** (SPEC-dependencies §10.13): `RunHooks` observe cross-agent orchestration; `AgentHooks` are scoped to a specific agent instance (receiver side fires on handoff). Both exist for every hook point above, and are registered independently.

---

## 11. Configuration

```yaml
agents:
  - name: "sera"
    provider: "lm-studio"
    model: "gemma-4-12b"
    persona:
      immutable_anchor: |
        You are Sera, an autonomous assistant.
      mutable_persona: |
        You prefer concise technical answers.
      mutable_token_budget: 300
      introspection:
        enabled: true
    runtime:
      type: "default"              # default | external
      max_tool_iterations: 10      # Safety limit on tool call loops
      structured_output:
        retry_on_failure: 3        # Retry model call if structured output fails validation
      context_pipeline:
        steps:
          - "persona"
          - "tools"
          - "skills"
          - "memory"
          - "history"
          - "current_turn"
          # Custom steps can be added here with stability_rank
    model_routing:                 # Optional per-task model overrides
      coding:
        provider: "fast-local"
        model: "qwen-7b"
    model_parameters:              # Default inference parameters
      temperature: 0.7
      top_p: 0.9
    sampler_profiles:              # Per-skill parameter overrides
      coding:
        temperature: 0.2
        min_p: 0.05
```

---

## 12. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | Tool registry and execution |
| `sera-memory` | [SPEC-memory](SPEC-memory.md) | Memory read/write |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Hook chain execution at all turn points |
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | AuthZ checks for tool execution |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Approval gating for tool calls |
| `sera-models` | This spec (§5) | Model provider abstraction |
| `sera-skills` | This spec (§4.2, step 3) | Skill injection |
| `sera-session` | [SPEC-gateway](SPEC-gateway.md) | Session transcript read/write |

---

## 13. Skills System

> **Enhancement: Anthropic Skills + Karpathy LLM-Wiki — Compilation over Retrieval**

A **Skill** is a named, versioned unit of agent capability that combines instructions, code, and tool bindings into a reusable, composable unit. Skills represent the agent's compiled knowledge — synthesized and action-ready, not raw retrieval.

### 13.1 Skill Definition

```rust
pub struct SkillDef {
    pub name: String,                      // e.g., "code-review", "deploy-staging"
    pub version: String,                   // Semantic version
    pub description: String,               // Human-readable description
    pub instructions: String,              // Markdown instructions injected into context
    pub code: Option<Vec<CodeArtifact>>,   // Optional: saved scripts/functions
    pub tool_bindings: Vec<ToolRef>,       // Tools this skill uses
    pub sampler_profile: Option<String>,   // Optional: linked sampler profile name
    pub model_override: Option<ModelRef>,  // Optional: use a specific model for this skill
    pub context_budget_tokens: u32,        // Max tokens for skill injection
    pub tags: Vec<String>,                 // Categorization tags
}

pub struct CodeArtifact {
    pub name: String,
    pub language: String,                  // "rust", "python", "bash", etc.
    pub code: String,
    pub description: String,
}
```

### 13.2 Skill Storage

Skills are stored in the agent's workspace as markdown files:

```
agents/sera/skills/
  ├── code-review.md
  ├── deploy-staging.md
  ├── data-analysis.md
  └── index.md                            # Skill registry, agent-maintained
```

Each skill file follows a structured format:

```markdown
# Skill: code-review
Version: 1.2
Tags: development, quality
Sampler: coding
Tools: file_read, file_write, run_command

## Instructions
[Markdown instructions injected into context when skill is active]

## Code
```python
# Reusable code artifact
def analyze_diff(diff_text):
    ...
```

## Notes
[Agent's self-maintained notes about this skill's effectiveness]
```

### 13.3 Skill Lifecycle

- **Discovery:** The `context_skill` step searches the skill index for skills relevant to the current task
- **Activation:** The most relevant skill's instructions are injected into the context pipeline
- **Creation:** Agents can create new skills by writing skill files to their workspace (subject to `config_propose` authorization)
- **Refinement:** Agents can update skill instructions based on success/failure feedback
- **Compilation:** The dreaming workflow can extract recurring successful patterns from memory and propose new skills

### 13.4 Configuration

```yaml
agents:
  - name: "sera"
    skills:
      enabled: true
      workspace: "./agents/sera/skills"
      max_active_skills: 3              # Maximum skills injected per turn
      auto_create: true                 # Agent can create new skills (via config_propose)
```
## 13a. ToolUseBehavior

> **Source:** `SPEC-dependencies §10.13` — openai-agents-python `tool_use_behavior` discriminated union.
> **Implementation:** `sera_types::tool::ToolUseBehavior` (type path).

### 13a.1 Type Definition

`ToolUseBehavior` is a tagged-union enum controlling how the LLM selects among available tools on a given turn. It is the SERA canonical representation of the OpenAI `tool_choice` / Anthropic `tool_choice` concept. Every provider integration translates from this type to the provider's wire format; the runtime never constructs provider-specific values directly.

```rust
/// Policy telling the LLM how to choose among available tools.
/// Maps to the OpenAI/Anthropic tool_choice concept.
///
/// Type path: `sera_types::tool::ToolUseBehavior`
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum ToolUseBehavior {
    /// Model decides freely whether to call a tool.
    #[default]
    Auto,
    /// Model MUST call at least one tool this turn (any of the available).
    Required,
    /// Model MUST NOT call any tools this turn.
    None,
    /// Model MUST call the named tool (must be in the available set).
    Specific { name: String },
}
```

**SERA wire format** (JSON — `tag = "mode"`):

| Variant | JSON |
|---|---|
| `Auto` | `{"mode":"auto"}` |
| `Required` | `{"mode":"required"}` |
| `None` | `{"mode":"none"}` |
| `Specific { name }` | `{"mode":"specific","name":"<name>"}` |

### 13a.2 Provider Wire Mapping

| SERA variant | OpenAI `tool_choice` | Anthropic `tool_choice` |
|---|---|---|
| `Auto` | `"auto"` | `{"type":"auto"}` |
| `Required` | `"required"` | `{"type":"any"}` |
| `None` | `"none"` | `{"type":"none"}` |
| `Specific { name }` | `{"type":"function","function":{"name":"<name>"}}` | `{"type":"tool","name":"<name>"}` |

Translation methods live on the enum:
- `ToolUseBehavior::to_openai_tool_choice(&self) -> serde_json::Value`
- `ToolUseBehavior::to_anthropic_tool_choice(&self) -> serde_json::Value`

Provider adapters are expected to call the appropriate translator; they must not hard-code wire strings.

### 13a.3 Validation Contract

Before passing the behavior to the LLM client, callers should invoke:

```rust
pub fn validate(&self, available_tools: &[String]) -> Result<(), ToolUseValidationError>
```

Rules:
- `Specific { name }` → `name` must be in `available_tools`; else `ToolUseValidationError::UnknownTool`.
- `Required` with empty `available_tools` → `ToolUseValidationError::NoToolsAvailable`.
- `Auto` and `None` → always valid.

### 13a.4 Threading into the Turn Loop

`ToolUseBehavior` is threaded through the turn loop as follows:

1. **`sera_types::runtime::TurnContext`** carries `tool_use_behavior: ToolUseBehavior` (`#[serde(default)]` → `Auto` for legacy frames).
2. **`sera_runtime::turn::TurnContext`** (internal loop context) mirrors this field.
3. **`OnLlmStart` hook point** fires immediately before the model call; a hook implementation may mutate `turn_ctx.tool_use_behavior` to enforce per-turn policy gates (e.g., force `Required` when a specific skill is active).
4. **`turn::think()`** forwards the behavior to `LlmProvider::chat_with_behavior()`.
5. **`LlmClient`** translates to `tool_choice` via `to_openai_tool_choice()` and includes it in the request body when tools are present or when `None` is specified.

### 13a.5 Out of Scope

- Per-tool runtime gating based on `ToolUseBehavior` — SkillBoundGate lives in `sera-mcp` (separate lane).
- Streaming tool-call parsing — separate concern.
- Full provider adapter rewrite — adapters call the translator methods; no rewrite needed.

---

## 15. PlanAndAct Multi-Phase Reasoning

> **Status:** DRAFT (fills gap `sera-taso`)

### 15.1 Motivation

The current single-phase turn loop in `turn.rs` conflates planning and action in a single LLM call: the model simultaneously reasons about what to do and emits tool calls to do it. For short-horizon tasks this is adequate, but for multi-step engineering work (refactors, migrations, codebase-wide changes) the model silently interleaves strategy and execution — meaning the plan is never recorded as a first-class artifact and is lost if an action step fails. A separated `Plan → Act → Observe → Replan` loop keeps the plan as a durable, inspectable artifact, allows HITL operators to review and veto plans before the first action fires, and enables individual `Act` steps to be retried against the same plan without forcing the model to rebuild its strategy from scratch. The `ReactMode::PlanAndAct` variant is already declared in §2.1 (sourced from SPEC-dependencies §10.15 MetaGPT) — this section fully specifies its behaviour.

**Tradeoff:** PlanAndAct adds latency (at minimum one extra LLM call for the Plan phase before the first action) and requires durable plan storage. It is opt-in per agent and not appropriate for single-turn or low-latency workloads.

---

### 15.2 Phase Model

PlanAndAct defines four distinct phases executed in sequence. Each phase is a separate turn in the SERA turn loop — the phase progression is **internal** to `plan_and_act.rs` and invisible to callers of `AgentRuntime::execute_turn`.

#### Phase definitions

| Phase | LLM call | `ToolUseBehavior` | Description |
|---|---|---|---|
| `Plan` | Yes | `None` (forced) | Produces a structured `Plan`; no tool calls permitted. Parse failure → `PlanParseFailed`. |
| `Act` | Yes | `Auto` (default) | Executes the current `PlanStep` via existing `turn::act` / `turn::react`. |
| `Observe` | No | N/A | Deterministic pass: collects tool outputs, advances `PlanStep::status` to `Completed` or `Failed`. |
| `Replan` | Yes | `None` (forced) | Receives plan state + observation; returns `ReplanDecision`. |

#### Rust type sketches (design — no `impl` blocks)

```rust
pub type PlanId = Uuid;

pub struct Plan {
    pub id: PlanId,
    pub steps: Vec<PlanStep>,
    pub rationale: String,
    pub estimated_turns: u32,
    pub created_at: chrono::DateTime<Utc>,
    pub status: PlanStatus,          // draft | active | completed | aborted
    pub parent_plan_id: Option<PlanId>,
}

pub struct PlanStep {
    pub idx: u32,
    pub description: String,
    pub expected_tools: Vec<String>, // used for enforce_step_tool (§15.8)
    pub acceptance: String,
    pub status: StepStatus,          // Pending | InProgress | Completed | Failed | Skipped
}

pub enum ReplanDecision {
    Continue,
    ReplanFrom { step_idx: u32, reason: String },
    Abort { reason: String },
}
```

**Tradeoff:** Structured `Plan` JSON is machine-parseable and enables reliable step tracking, but it constrains the LLM's output format and may require retries on parse failure. Free-form markdown plans are more flexible but fragile to parse — see §15.12 open question 1.

---

### 15.3 Persistence

Plans are stored in a dedicated `plan` table in the memory store. This table is owned by `sera-db` and follows the same migration pattern used by all other SERA tables (append-only, timestamped, no destructive updates).

#### Schema

```sql
CREATE TABLE plan (
    plan_id        UUID        PRIMARY KEY,
    agent_id       TEXT        NOT NULL,
    session_id     TEXT        NOT NULL,
    payload        JSONB       NOT NULL,       -- serialized Plan struct
    status         TEXT        NOT NULL        -- 'draft' | 'active' | 'completed' | 'aborted'
                   CHECK (status IN ('draft', 'active', 'completed', 'aborted')),
    parent_plan_id UUID        REFERENCES plan(plan_id),   -- NULL for initial plans
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

```

Plans are **append-only**: a `Replan` decision does not mutate the existing row. Instead a new row is inserted with `parent_plan_id` pointing to the superseded plan. The superseded plan's `status` is updated to `aborted`. This gives a full replan audit trail queryable by following `parent_plan_id` chains.

Cross-reference: SPEC-memory.md §3 (Built-in Backends) for the PostgreSQL backend that hosts this table. The migration file follows the naming convention already established in `sera-db` (`V<n>__add_plan_table.sql`).

**Tradeoff:** Append-only rows provide full auditability; add a GC policy (archive completed plans after N days) in Phase 4+.

---

### 15.4 Integration with `turn.rs` and `default_runtime.rs`

PlanAndAct extends the existing turn infrastructure at two points, leaving the public `AgentRuntime::execute_turn` signature unchanged.

#### Extension points

`turn::TurnContext` gains `pub active_plan: Option<Plan>` (`None` for all single-phase turns). `DefaultRuntime::execute_turn` branches before the existing loop:

```rust
if turn_ctx.active_plan.is_some() {
    return plan_and_act::run_turn(turn_ctx, self.llm.as_deref(), self.tool_dispatcher.as_deref()).await
        .map_err(RuntimeError::from);
}
// existing single-phase loop unchanged
```

#### `plan_and_act.rs` module — phase dispatcher

```rust
async fn run_turn(ctx: TurnContext, llm, dispatcher) -> Result<TurnOutcome, PlanAndActError> {
    match ctx.active_plan.as_ref().unwrap().current_phase() {
        Phase::Plan    => run_plan_phase(&ctx, llm).await,
        Phase::Act     => run_act_phase(&ctx, plan.current_step(), llm, dispatcher).await,
        Phase::Observe => run_observe_phase(&ctx).await,
        Phase::Replan  => run_replan_phase(&ctx, llm).await,
    }
}
```

**One phase per turn.** `execute_turn` is called once per phase; the phase state machine lives in the gateway session loop, not inside `execute_turn`. This keeps the runtime stateless between calls (§1a) and allows HITL insertion between phases.

**Tradeoff:** The gateway must understand PlanAndAct phase transitions. An internal multi-phase loop inside a single `execute_turn` call would be simpler but breaks the stateless contract.

---

### 15.5 HITL on Plans

Plan approval integrates with the existing HITL approval machinery (SPEC-hitl-approval.md §3, §7).

#### Configuration

```yaml
agents:
  - name: "sera"
    runtime:
      plan_and_act:
        plan_approval_required: true  # default: false
        replan_policy: auto           # auto | manual | never
        enforce_step_tool: false
        replan_loop_limit: 10
```

#### Approval flow

When `plan_approval_required: true`: plan is persisted with `status = 'draft'`; `execute_turn` returns `TurnOutcome::Interruption` with `hook_point = "plan_approval"`; a `PlanApprovalRequired` event is emitted (§15.7); the system waits for `POST /api/plans/{plan_id}/approve` or `/reject`, re-using the `ApprovalTicket` state machine from SPEC-hitl-approval.md §7. On approval, plan → `active` and the Act phase proceeds. On rejection, plan → `aborted` and the rejection reason is delivered to the agent as a user-role message.

When `plan_approval_required: false` (default), the plan transitions `draft → active` immediately.

**Tradeoff:** Approval adds latency proportional to operator response time. Default `false`; enable for high-impact tasks (production deploys, schema migrations).

---

### 15.6 Replanning Triggers

Three conditions trigger the Replan phase:

1. **Step failure** — `StepStatus::Failed` after Observe. Auto-replan fires unless `replan_policy: never`; `manual` freezes the loop and waits for operator instruction.
2. **Operator steer** — A steer message (§3 steer queue) while a plan is active forces replan regardless of `replan_policy`; steer content is included in the Replan context.
3. **Precondition violation** — Observe detects an environmental change invalidating a step (deleted file, changed dependency). Sets `StepStatus::Failed { reason: "precondition_violated" }` → triggers step-failure path.

#### `replan_policy` enum

```rust
pub enum ReplanPolicy {
    Auto,    // default — enter Replan phase on step failure
    Manual,  // freeze turn loop; wait for operator instruction
    Never,   // abort on first step failure
}
```

The default is `Auto`. `Never` is appropriate when partial execution is worse than no execution (e.g., a schema migration that must be atomic).

**Tradeoff:** `Auto` may loop if the model repeatedly produces bad plans for the same step. The `replan_loop_limit` (default 10, §15.9) is the safety valve.

---

### 15.7 Streaming and UX

Plans and phase transitions are streamed to connected clients via the existing event channel in `sera-events` / Centrifugo (SPEC-observability.md §2.1a, SPEC-gateway for the Centrifugo publish path).

#### Typed events emitted per phase transition

| Event | When emitted |
|---|---|
| `PlanCreated` | Plan phase complete |
| `PlanApprovalRequired` | Draft awaiting operator sign-off |
| `StepStarted` | Act phase begins for step N |
| `StepCompleted` | Observe marks step Completed |
| `StepFailed` | Observe marks step Failed |
| `Replanned` | Replan phase produces new plan |
| `PlanCompleted` | All steps Completed |
| `PlanAborted` | Abort decision or approval rejected |

All events carry `plan_id`, `agent_id`, `session_id`, and a W3C trace context (SPEC-observability.md §2.1).

UIs receiving these events can render a step-by-step progress tracker without polling.

---

### 15.8 Interaction with ToolUseBehavior

PlanAndAct overrides `ToolUseBehavior` per phase, taking precedence over any session-level or hook-set value. The interaction is:

| Phase | `ToolUseBehavior` | Rationale |
|---|---|---|
| `Plan` | `None` (forced) | Planning is pure reasoning; tool calls in this phase indicate prompt leakage and must be rejected |
| `Act` | `Auto` (default) | Agent executes freely; individual step overrides possible (see below) |
| `Observe` | N/A — no LLM call | Deterministic system pass |
| `Replan` | `None` (forced) | Decision reasoning; no tool calls permitted |

#### Per-step tool enforcement

When `enforce_step_tool: true` and a `PlanStep` lists exactly one `expected_tools` entry, the Act phase sets `ToolUseBehavior::Specific { name }` for that LLM call (catching plan/execution divergence). Otherwise `Auto` is used. The override is applied at the `OnLlmStart` hook point (§10, §13a.4).

Cross-reference: `ToolUseBehavior` type definition and provider wire mapping at §13a.

---

### 15.9 Error Surface

```rust
#[derive(Debug, thiserror::Error)]
pub enum PlanAndActError {
    #[error("plan parse failed: {reason}")]
    PlanParseFailed { reason: String },
    #[error("step {step_idx} failed: {reason}")]
    StepFailed { step_idx: u32, reason: String },
    #[error("replan loop limit reached after {hits} replans")]
    ReplanLoopLimit { hits: u32 },
    #[error("plan approval timed out for plan {plan_id}")]
    ApprovalTimeout { plan_id: PlanId },
    #[error("invalid phase transition from {from:?} to {to:?}")]
    InvalidTransition { from: Phase, to: Phase },
}
```

`StepFailed` / `ReplanLoopLimit` → `TurnOutcome::Stop`; `ApprovalTimeout` / `InvalidTransition` → `TurnOutcome::Interruption`; `PlanParseFailed` → retry up to `structured_output.retry_on_failure` (§11) then `Stop`. Default `replan_loop_limit`: **10** (hard cap).

---

### 15.10 Metrics and Observability

The following Prometheus-style counters and histograms are emitted by `plan_and_act.rs` via `sera-telemetry` (SPEC-observability.md §2.2). These are named here for the implementor; metric registration is not part of this spec.

| Metric name | Type | Labels | Description |
|---|---|---|---|
| `plan_created_total` | Counter | `agent` | Plans produced by the Plan phase |
| `plan_steps_completed_total` | Counter | `agent` | Individual steps successfully completed |
| `plan_steps_failed_total` | Counter | `agent`, `step_idx` | Individual step failures |
| `plan_replans_total` | Counter | `agent`, `trigger` | Replan decisions, labelled by trigger (`step_failure`, `operator_steer`, `precondition_violated`) |
| `plan_approval_latency_seconds` | Histogram | `agent` | Time from `PlanApprovalRequired` event to operator decision |
| `plan_duration_seconds` | Histogram | `agent` | End-to-end wall time from Plan phase start to `PlanCompleted` or `PlanAborted` |

The `trigger` label values (`step_failure`, `operator_steer`, `precondition_violated`) map directly to the three triggers in §15.6.

---

### 15.11 Test Strategy

#### Unit tests

- Phase transitions: every valid `(from, to)` pair passes; every invalid pair returns `InvalidTransition`. Valid sequence: `Plan → Act → Observe → Replan → Act → ...`
- `StepStatus` DAG: `Pending → InProgress → {Completed, Failed, Skipped}`; backwards moves rejected.
- `ToolUseBehavior` override: each phase sets the expected value on `TurnContext::tool_use_behavior` before the LLM call.
- `ReplanPolicy::Never` + `StepFailed` → `TurnOutcome::Stop` without entering Replan.

#### Integration test (end-to-end)

Mock `LlmProvider` returns canned Plan JSON (Plan phase), then a tool call (Act phase), then `ReplanDecision::Continue` (Replan phase). Mock `ToolDispatcher` fails once then succeeds. Assertions: plan row persisted after Plan phase; `StepFailed` event emitted; new plan row written with `parent_plan_id` set; second Act phase emits `PlanCompleted`.

#### Property tests (`proptest` / `quickcheck`)

- `StepStatus` DAG: transition validator rejects all backwards moves (`Completed → Pending`, etc.) and accepts all forward moves (`Pending → InProgress → {Completed, Failed, Skipped}`).
- `Plan` serde round-trip: `from_str(to_string(&plan)?) == plan` for any generated `Plan`.

---

### 15.12 Open Questions

- **Plan serialization format** — JSON schema via `StructuredOutputConfig::JsonSchema` (§5.1) is preferred (reliable, parseable) but not all providers support it. Should a markdown fallback parser be required for providers without structured output?

- **Plan visibility to sub-agents** — When a planner spawns sub-agents via handoff (§9b) or circles (SPEC-circles.md), should the active plan be forwarded as read-only context? The `HandoffInputFilter` (§9b) could strip or include it per-handoff.

- **Replan budget — hard cap vs. adaptive escalation** — The current hard `replan_loop_limit` (default 10) aborts on exhaustion. An adaptive policy could escalate to HITL after N replans instead of aborting. Threshold needs human input.

- **Partial plan execution and rollback** — Aborted plans leave completed-step side effects in place. Should `PlanStep` carry an optional `rollback_description` for compensating actions? Interacts with §6.1 idempotency; likely Phase 4+.

---

## 14. Open Questions

1. ~~**Skill system definition**~~ — Resolved: See §13.
2. **Subagent session scoping** — Do subagent turns run in the parent session or their own child session?
3. **Max tool iterations** — What is the default limit on tool call loops? How does the runtime handle infinite loops?
4. **Streaming** — How does the runtime stream partial responses to the gateway during a turn? Token-by-token? Chunk-by-chunk?
5. **Context window overflow** — How does the pipeline handle context that exceeds the model's window? Truncation strategy? Compaction trigger?
6. **Task classification for model routing** — How is a task classified (e.g., "coding" vs. "architecture") for model routing? LLM-driven classification? Keyword heuristics? Skill-based?
7. **Structured output provider support matrix** — Which model providers support which structured output mechanisms? What's the fallback for providers without native support?

---

## 14. Success Criteria

| Metric | Target |
|---|---|
| KV cache hit rate | ≥ 80% prefix reuse across turns in same session |
| Turn execution | End-to-end turn within model latency + overhead < 50ms (excluding model time) |
