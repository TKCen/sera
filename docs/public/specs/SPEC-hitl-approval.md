# SPEC: Human-in-the-Loop & Agent-in-the-Loop Approval System (`sera-hitl`)

> **Status:** DRAFT
> **Source:** PRD §9 (all subsections), §14 (invariant 12), plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §10.2 (Codex five-level `AskForApproval` + `GranularApprovalConfig` + Guardian pre-gate), §10.3 (Paperclip `revision_requested` state), §10.5 (openclaw `ExecApprovalsFileSchema` with per-agent `argPattern` + `autoAllowSkills`), §10.7 (opencode `CorrectedError { feedback }` tool-result variant, `Permission.Ruleset` wildcard evaluator, doom-loop escalation), §10.10 (OpenHands `SecurityAnalyzer` trait with `ActionSecurityRisk` enum + `confirmation_mode` hold-pending pattern), §10.13 (openai-agents-python `InputGuardrail`/`OutputGuardrail` running concurrently with LLM + `is_enabled` / `needs_approval` callbacks, `NextStep::Interruption`), §10.14 (CrewAI `Task.guardrail` retry loop + `@human_feedback` async state serialization), [SPEC-self-evolution](SPEC-self-evolution.md) §7 (meta-change approval path with pinned approvers + operator offline key)
> **Crate:** `sera-hitl`
> **Priority:** Phase 1

---

## 1. Overview

The approval system is a **first-class citizen** in SERA, not a bolt-on. It provides configurable escalation chains that can involve agents, humans, or both as approvers. The system supports:

- **Configurable escalation chains** (subagent → supervisor agent → human)
- **Agent review and approval** — agents can be approvers, enabling automatic review flows
- **Dynamic, risk-based routing** — approval routing decisions based on runtime risk assessment
- **Multi-approval gates** and enterprise policy-driven routing
- **Configurable enforcement** — from fully autonomous (zero approvals) to strict (all tool calls require approval)

---

## 2. Approval Model

```rust
pub struct ApprovalSpec {
    pub scope: ApprovalScope,
    pub description: String,
    pub urgency: ApprovalUrgency,
    pub routing: ApprovalRouting,
    pub timeout: Duration,
    pub required_approvals: u32,          // For multi-approval gates (e.g., 2-of-3)
    pub evidence: ApprovalEvidence,
    pub security_risk: ActionSecurityRisk, // From SecurityAnalyzer (§2a)
    pub meta_change: Option<MetaChangeContext>, // Present only for self-evolution (§5a)
}

pub enum ApprovalScope {
    ToolCall(ToolRef),
    SessionAction(SessionAction),
    MemoryWrite(MemoryScope),
    ConfigChange(ConfigPath),
    ChangeArtifact(ChangeArtifactId),     // Self-evolution Tier 2/3 (SPEC-self-evolution §9)
    MetaChange(MetaChangeContext),        // Approval-path self-modification
}

pub enum ApprovalUrgency {
    Low,
    Medium,
    High,
    Critical,
}

pub struct ApprovalEvidence {
    pub tool_args: Option<serde_json::Value>,
    pub risk_score: Option<f64>,
    pub principal: PrincipalRef,
    pub session_context: Option<String>,
    pub guardian_assessment: Option<GuardianAssessment>, // Pre-approval LLM risk assessor (§2b)
    pub additional: HashMap<String, serde_json::Value>,
}
```

### 2a. `SecurityAnalyzer` Trait and `ActionSecurityRisk` Enum

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.10 OpenHands `SecurityAnalyzer`.

Per-action risk classification happens **before** the static approval matrix applies. A pluggable `SecurityAnalyzer` trait runs an async risk assessment on every proposed action; the returned `ActionSecurityRisk` feeds into routing decisions and determines whether `confirmation_mode` holds the action pending.

```rust
#[async_trait]
pub trait SecurityAnalyzer: Send + Sync {
    async fn security_risk(&self, action: &ProposedAction) -> Result<ActionSecurityRisk, AnalyzerError>;
    fn name(&self) -> &str;
}

pub enum ActionSecurityRisk {
    Low,      // Bypass approval unless policy forces it
    Medium,   // Route through standard approval chain
    High,     // Escalate; require meta-quorum if this is a change scope
}
```

**Reference backends** (from SPEC-dependencies §10.10):

- `InvariantAnalyzer` — integration with [Invariant Labs](https://invariantlabs.ai/) for policy-based risk scoring
- `GraySwanAnalyzer` — integration with GraySwan AI safety evaluation
- `HeuristicAnalyzer` — built-in rule-based fallback for Tier-1 deployments

**`confirmation_mode` hold-pending pattern.** When `AgentController.confirmation_mode: bool` is true, every action above a threshold risk level is held in `_pending_action_info` until the user issues `ActionConfirmationStatus::Confirmed` or `Rejected` via a `MessageAction`. This maps directly onto SERA's `TurnOutcome::Interruption` and the session's `WaitingForApproval` state.

### 2b. Guardian Pre-Approval Risk Assessor

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.2 Codex Guardian.

Before any HITL approval is surfaced to a user, a **Guardian** subsystem can run an LLM-based risk assessment to filter or annotate the request. This is an *optional* pre-gate that adds LLM-informed context to the `ApprovalEvidence` without replacing the downstream approval chain.

```rust
pub struct GuardianAssessment {
    pub risk_level: GuardianRiskLevel,  // Low | Medium | High
    pub rationale: String,
    pub recommended_action: GuardianRecommendation, // Auto-approve | Surface to user | Block
}
```

Guardian assessments are emitted as `EventMsg::GuardianAssessment` on the EQ channel so clients can display the reasoning inline when the approval request is surfaced.

---

## 3. Approval Routing

```rust
/// Five-level enumeration aligned with Codex `AskForApproval` (SPEC-dependencies §10.2).
/// Approval responses flow through the gateway SQ as Op::ApprovalResponse — no parallel RPC surface.
pub enum AskForApproval {
    /// Ask for everything except known-safe read-only operations.
    UnlessTrusted,

    /// Model decides when to ask. Default for Tier-2 standard mode.
    OnRequest,

    /// Per-category fine control — see §5a.
    Granular(GranularApprovalConfig),

    /// Full-auto; no HITL ever. Tier-1 autonomous sandbox only.
    Never,

    /// Static, dynamic, or delegated policy resolution — the original SERA model kept for backward compat.
    Policy(ApprovalRouting),
}

pub enum ApprovalRouting {
    /// Static chain — always the same approvers in order
    Static(Vec<ApprovalTarget>),

    /// Dynamic — resolved at runtime based on risk score, context, policy
    Dynamic(ApprovalPolicy),

    /// None — fully autonomous, no approval needed
    Autonomous,
}

pub struct ApprovalPolicy {
    pub risk_thresholds: Vec<RiskThreshold>,
    pub fallback_chain: Vec<ApprovalTarget>,
}

pub struct RiskThreshold {
    pub min_risk_score: f64,
    pub chain: Vec<ApprovalTarget>,
    pub required_approvals: u32,
}

pub enum ApprovalTarget {
    Agent(AgentRef),                     // An agent acts as reviewer/approver
    Principal(PrincipalRef),             // A specific principal (human or service)
    PrincipalGroup(PrincipalGroupId),    // Any member of a group
    Role(RoleName),                      // Any principal with this role
    ExternalPDP,                         // Delegate to the AuthZ provider
}
```

---

## 4. Escalation Flow

```
Action triggers approval check
  → Dynamic risk assessment (compute risk score)
  → Resolve approval policy (risk score determines routing)
  → Route to first target in chain

If target is Agent:
  → Agent reviews evidence
  → Agent decides: Approve / Reject / Escalate
    → Approved → Execute
    → Rejected → Reject with reason
    → Uncertain/Escalate → Move to next target in chain

If target is Human:
  → Deliver approval notification (CLI, TUI, Web, channel)
  → Human decides: Approve / Reject
    → Approved → Execute
    → Rejected → Reject with reason
    → Timeout → Escalate to next target

If no more targets:
  → Reject (escalation exhausted)
```

### Combined Approvals

A risk assessment might determine that a low-risk tool call can be approved by a supervisor agent automatically, while a high-risk tool call requires both agent review AND human confirmation. The `required_approvals` field combined with multiple targets enables this (e.g., `Agent("safety-checker") + Principal("operator")`, both required).

---

## 5. Enterprise Routing Examples

| Scenario | Risk | Routing | Approval Count |
|---|---|---|---|
| Agent reads a file | Low | `Autonomous` | 0 |
| Agent deletes a file | Medium | `Dynamic → [Agent("safety-checker"), Principal("operator")]` | 1 |
| Agent sends external email | Medium | `Static → [Principal("team-lead")]` | 1 |
| Agent modifies production config | High | `Dynamic → [Role("ops-lead"), Role("security")]` | 2-of-2 |
| Agent executes high-risk tool | High | `Dynamic → [Agent("risk-assessor"), PrincipalGroup("senior-engineers")]` | 2-of-2 |
| Factory agent changes PLC parameters | Critical | `Static → [Principal("floor-supervisor"), Principal("safety-officer")]` | 2-of-2 |

---

## 6. Enforcement Modes

```yaml
sera:
  approval:
    mode: "standard"
```

| Mode | Behavior |
|---|---|
| `autonomous` | No approvals ever. Private sandbox, development. |
| `standard` | Approval policy applies. Default. |
| `strict` | All tool calls require at minimum one approval. High-security enterprise. |

---

## 5a. GranularApprovalConfig — Per-Category Fine Control

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.2 Codex + §10.5 openclaw `ExecApprovalsFileSchema`.

Approval routing can be specified **per risk category** (exec, patch, file write, network, MCP call). Each category can have its own chain, its own required approvals, and its own argument patterns.

```rust
pub struct GranularApprovalConfig {
    pub exec: CategoryRouting,
    pub patch: CategoryRouting,
    pub file_write: CategoryRouting,
    pub network: CategoryRouting,
    pub mcp_call: CategoryRouting,
    pub memory_write: CategoryRouting,
    pub config_change: CategoryRouting,
    pub meta_change: Option<CategoryRouting>,  // Required for Tier-2/3 self-evolution
}

pub struct CategoryRouting {
    pub default: ApprovalRouting,

    /// Per-agent allowlists with argument patterns (openclaw ExecApprovals pattern).
    /// Pattern + argPattern = fine-grained gating beyond command-level.
    pub allow_list: Vec<ExecAllowRule>,

    /// `autoAllowSkills: true` grants trust-tier shortcut — skill-bound tools bypass approval.
    pub auto_allow_skills: bool,
}

pub struct ExecAllowRule {
    pub agent_ref: AgentRef,
    pub pattern: String,                // Command or tool name glob
    pub arg_pattern: Option<String>,    // Argument regex (separate from command pattern)
    pub reason: String,                 // Audit trail
}
```

**Wildcard evaluator semantics** (opencode `Permission.Ruleset` pattern): rules are evaluated in order, stricter-wins; `deny` outranks `ask` outranks `allow`. The session runtime layer can add temporary per-session overrides (e.g., "always allow `edit *.md` for the rest of this session") via `always/once/reject+feedback` replies that extend the ruleset.

---

## 5b. `CorrectedError` — In-Turn Self-Correction

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.7 opencode. **The highest-leverage new pattern in the entire HITL spec.**

When a user rejects a tool call with a written reason, SERA does NOT simply return a generic error. Instead, the user's rejection reason flows back into the LLM's turn as a structured tool-result error with the user's feedback as the error body. The LLM can then self-correct in the **same turn** without a turn restart:

```rust
pub enum ToolResult {
    Ok { output: serde_json::Value },
    Err { error: String },
    /// User rejected the tool call with a feedback message.
    /// The LLM sees this as a tool error whose body is the user's rejection text.
    /// This enables in-turn self-correction without starting a new turn.
    Rejected { feedback: String },
}
```

When the HITL layer produces `ToolResult::Rejected`, the runtime feeds it back into the model as the tool call's result on the same turn. The next model response usually either asks a clarifying question or proposes an alternative approach — all without a turn boundary. This matches opencode's `CorrectedError` pattern exactly.

---

## 5c. `revision_requested` Approval State

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.3 Paperclip.

Approvals are not binary approve/reject — they support a **two-step revision cycle**:

```rust
pub enum ApprovalState {
    Pending,
    Approved,
    Rejected { reason: String },

    /// Reviewer found issues but believes the proposer can revise and retry.
    /// The artifact returns to Pending after revision is submitted.
    RevisionRequested { feedback: String },
}
```

A `RevisionRequested` verdict keeps the original ApprovalTicket alive but transitions it to a new `Pending` state with the reviewer's feedback attached. The proposer (whether human or agent) sees the feedback, revises the proposal, and resubmits — no new ticket is created. This is particularly valuable for `Supervised` Circles where partial rework is more common than hard rejection.

---

## 5d. Doom-Loop Escalation Category

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.7 opencode `DOOM_LOOP_THRESHOLD = 3`.

When the runtime's `DoomLoopDetector` fires (see [SPEC-runtime](SPEC-runtime.md) §3.1) because an agent has made 3 consecutive identical tool calls, SERA does **not** hard-fail. Instead it escalates to a dedicated `doom_loop` approval category with `ActionSecurityRisk::Medium`:

- The user is surfaced the loop pattern with a `GuardianAssessment` explaining what the agent was trying to do
- The user can `Approve` (let the agent continue), `Reject` (stop the agent), or `Rewrite` (provide a manual intervention via `CorrectedError { feedback }`)
- Approved retries reset the doom-loop counter

This turns a potential stuck-agent failure into a recoverable HITL moment.

---

## 5e. Meta-Change Approval Path (Self-Evolution)

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §7. **Critical:** this closes the "approval self-loop" deadlock.

When the approval scope is `ApprovalScope::MetaChange` — meaning the change being proposed would alter the approval infrastructure itself — routing flows through a **separate, pinned approval path**:

1. **Approver-pinning:** the current `MetaApprover` principal set is frozen at the moment the meta-change is proposed. The meta-change is evaluated against that frozen set, not the live set. This prevents the "remove the approver then push the change" attack.
2. **Meta-quorum:** signatures from at least `CON-07.min_signers` principals (default 2) from the pinned set are required.
3. **Observability escalation:** meta-change proposals emit a high-priority audit event and notify out-of-band channels (operator email, Slack, PagerDuty).
4. **No self-approval:** the proposing principal cannot be a signer for its own meta-change.
5. **Replay lock:** during a meta-change's effective window, no other meta-change can be in flight.
6. **Operator offline key:** for the four most dangerous scopes (`ConstitutionalRuleSet`, `KillSwitchProtocol`, `AuditLogBackend`, `SelfEvolutionPipeline` per [SPEC-self-evolution](SPEC-self-evolution.md) §9.1), an additional signature from a key held outside the running SERA instance is required. The key lives on an operator HSM or air-gapped device. These changes are, by design, slow and manual.

```rust
pub struct MetaChangeContext {
    pub change_artifact: ChangeArtifactId,
    pub pinned_approvers: HashSet<PrincipalRef>, // Frozen at proposal time
    pub required_signers: u32,                    // Meta-quorum size
    pub offline_key_required: bool,               // For the four dangerous scopes
    pub observability_escalation: bool,           // Out-of-band notification
}
```

---

## 6a. `InputGuardrail` / `OutputGuardrail` — Pre/Post Action Gates

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.13 openai-agents-python.

Beyond the approval chain, SERA supports **guardrails** — pre-execution and post-execution gates that run a validation function and can short-circuit with a tripwire. Input guardrails run **concurrently with the LLM call** by default (unusual but intentional — most frameworks run guardrails sequentially before the LLM, which adds latency for no safety gain).

```rust
pub struct GuardrailResult {
    pub tripwire_triggered: bool,
    pub output_info: serde_json::Value,
}

#[async_trait]
pub trait InputGuardrail: Send + Sync {
    async fn run(
        &self,
        ctx: &RunContext,
        agent: &Agent,
        input: &[Message],
    ) -> Result<GuardrailResult, GuardrailError>;

    fn name(&self) -> &str;
    fn run_in_parallel(&self) -> bool { true } // Default: run concurrently with LLM call
}

#[async_trait]
pub trait OutputGuardrail: Send + Sync {
    async fn run(
        &self,
        ctx: &RunContext,
        agent: &Agent,
        output: &AgentOutput,
    ) -> Result<GuardrailResult, GuardrailError>;

    fn name(&self) -> &str;
}
```

**Tripwire semantics.** When `tripwire_triggered == true`, the guardrail halts the turn and raises either `InputGuardrailTripwireTriggered` or `OutputGuardrailTripwireTriggered`. Input guardrails halt before the LLM call completes (if the LLM is still in flight, it is aborted via the abort signal). Output guardrails halt before the final output is delivered.

**Concurrent execution in Rust:** SERA uses `tokio::join!` to run `InputGuardrail::run()` and the LLM call in parallel. The turn completes when both return; a guardrail tripwire racing against a slow LLM call is a net win.

---

## 7. Approval State Machine

When an action triggers approval:

1. Session enters `WaitingForApproval` state
2. An `ApprovalTicket` is created and stored
3. Notification is delivered to the target approver(s)
4. The system waits for responses (with timeout)
5. On approval: session returns to `Active`, action executes
6. On rejection: session returns to `Active`, rejection delivered to agent — via `ToolResult::Rejected { feedback }` if the user provided a reason (§5b)
7. On `RevisionRequested`: ticket stays alive, feedback attached, proposer revises
8. On timeout: escalate to next target or reject

### Approval Notification Delivery

Approval requests are delivered through whatever channel the approver is connected to:
- CLI/TUI users see approval prompts inline
- Web users see approval UI
- Channel users (Discord, Slack) receive approval messages with approve/reject actions
- Agent approvers receive the approval as a turn context (with evidence)

### 7a. Speculative Execution During HITL Wait (Deferred: Phase 4+)

> **Enhancement: OpenSwarm v3 §3 (Speculative Decoding for HITL)**

When a session is in `WaitingForApproval` state, the system is idle. As an optimization, the runtime can **speculatively compute the next steps** assuming approval will be granted:

```
Action triggers approval → session enters WaitingForApproval
  → Speculative fork: compute next DAG steps assuming approval
  → Results held in shadow context (not committed)

If approved:
  → Shadow results committed → zero latency to next action

If rejected:
  → Shadow results discarded → no side effects
```

**Constraints:**
- Speculative execution **MUST NOT** produce side effects (no tool execution, no memory writes, no external calls)
- Only context assembly and model reasoning are speculated
- Speculative results are discarded if the approval is rejected or times out
- This is strictly an optimization — correctness is identical with or without speculation

**Configuration:**

```yaml
sera:
  approval:
    speculative_execution: false         # Opt-in, default off
```

> [!NOTE]
> This optimization is most impactful when (a) the model provider supports KV cache forking (save/restore cache state), and (b) the approval wait time is significant. It is a Phase 4+ optimization and should be implemented only after core approval flows are stable.

---

## 8. Hook Points

| Hook Point | Fires When |
|---|---|
| `on_approval_request` | When HITL approval is triggered — routing to correct approver, escalation logic |

---

## 9. Configuration

```yaml
sera:
  approval:
    mode: "standard"                    # autonomous | standard | strict
    default_timeout: "5m"               # Default timeout before escalation
    notification_channels:              # Where to deliver approval requests
      - "cli"
      - "web"
      - "discord"

agents:
  - name: "sera"
    approval:
      tool_overrides:
        shell:
          routing:
            type: "static"
            chain:
              - type: "principal"
                id: "operator"
          required_approvals: 1
        admin_*:
          routing:
            type: "dynamic"
            risk_thresholds:
              - min_risk_score: 0.7
                chain:
                  - type: "role"
                    name: "admin"
                required_approvals: 1
```

---

## 10. Invariants

| # | Invariant | Enforcement |
|---|---|---|
| 12 | HITL gates respect policy | Mode-dependent: autonomous / standard / strict |

---

## 11. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | `NeedsApproval` decision from AuthZ; `MetaApprover` capability; pinned approver sets |
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Session state `WaitingForApproval`; approval responses flow through SQ `Op::ApprovalResponse` (no parallel RPC surface) |
| `sera-runtime` | [SPEC-runtime](SPEC-runtime.md) | `TurnOutcome::Interruption` during approval wait; doom-loop detection (§5d); guardrails concurrent with LLM (§6a) |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | `on_approval_request` hook; hook-contributed authz checks |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | Tool risk levels trigger approval checks; `needs_approval: bool \| Fn` callback on tools |
| `sera-meta` | [SPEC-self-evolution](SPEC-self-evolution.md) | Meta-change approval path with pinned approvers (§5e); operator offline key for CON-07 |
| Dependencies | [SPEC-dependencies](SPEC-dependencies.md) | §10.2 Codex five-level `AskForApproval` + `GranularApprovalConfig` + Guardian; §10.3 Paperclip `revision_requested`; §10.5 openclaw `ExecApprovals` argPattern; §10.7 opencode `CorrectedError` + doom-loop; §10.10 OpenHands `SecurityAnalyzer`; §10.13 openai-agents-python guardrails; §10.14 CrewAI `@human_feedback` |

---

## 12. Open Questions

1. **Approval persistence** — Where are ApprovalTickets stored? Database? What's the lifecycle? Expiry?
2. **Approval audit trail** — Are approval decisions (who approved, when, with what evidence) stored as first-class audit events?
3. **Concurrent approvals** — If multiple approval requests are pending for the same session, how are they managed?
4. **Approval UI surface** — What's the approval UX in each client? Is there a dedicated approval queue view?
5. **Agent-as-approver DX** — How does an agent reviewer receive and process approval requests? Is it a special tool or a special turn type?
6. **Speculative execution scope** — How deep should speculative execution go? First tool call only? Full sub-tree?

---

## 13. Success Criteria

| Metric | Target |
|---|---|
| HITL approval roundtrip | < 500ms from trigger to notification delivery |
