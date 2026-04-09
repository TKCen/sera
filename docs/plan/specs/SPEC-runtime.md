# SPEC: Agent Runtime (`sera-runtime`)

> **Status:** DRAFT  
> **Source:** PRD §4.2, §13 (AgentRuntimeService proto), §14 (invariant 15)  
> **Crate:** `sera-runtime`  
> **Priority:** Phase 2  

---

## 1. Overview

The agent runtime is the **worker that does the "thinking + doing."** It receives a dequeued event plus session context from the gateway, assembles a context window, calls the model, processes tool calls, writes memory, and delivers the response.

The runtime is **isolated, stateless per-turn, and session-scoped.** It does not own durable state — it reads from and writes to owned subsystems (memory, session transcript, tools).

The **default runtime** is a highly configurable pipeline shipped with SERA. **External runtimes** (e.g., Python-hosted, domain-specific) implement the `AgentRuntimeService` gRPC service and register with the gateway.

---

## 2. Runtime Trait

```rust
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    async fn execute_turn(&self, ctx: TurnContext) -> Result<TurnResult, RuntimeError>;
    async fn capabilities(&self) -> RuntimeCapabilities;
    async fn health(&self) -> HealthStatus;
}
```

- **`execute_turn`** — Executes one complete turn: context assembly → model call → tool loop → memory write → response delivery
- **`capabilities`** — Reports what the runtime supports (streaming, tool calls, subagent spawning, etc.)
- **`health`** — Liveness/readiness

---

## 3. Turn Loop (Default Runtime)

```
Event + Session Context
  → pre_turn hook chain
  → Context Assembly Pipeline (KV-cache optimized)
  → Model Call
  → [Tool Call Loop]
      → pre_tool hook chain
      → Tool Execution
      → post_tool hook chain
      → Tool results re-enter model
  → Response
  → post_turn hook chain
  → Memory Write Pipeline
      → pre_memory_write hook chain
      → Backend write
  → pre_deliver hook chain
  → Deliver Response
  → post_deliver hook chain
```

The tool call loop repeats until the model returns a final response (no more tool calls) or a configured max-iterations limit is reached.

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

## 10. Hook Points

| Hook Point | Fires When |
|---|---|
| `pre_turn` | After queue dequeue, before context assembly |
| `context_persona` | During persona assembly step |
| `context_memory` | During memory injection step |
| `context_skill` | During skill injection step |
| `context_tool` | During tool injection step |
| `pre_tool` | Before tool execution |
| `post_tool` | After tool execution |
| `post_turn` | After runtime, before response delivery |
| `pre_deliver` | Before response delivery to client/channel |
| `post_deliver` | After response delivery confirmed |
| `pre_memory_write` | Before durable memory write |

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
