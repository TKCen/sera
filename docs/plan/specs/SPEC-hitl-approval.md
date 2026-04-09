# SPEC: Human-in-the-Loop & Agent-in-the-Loop Approval System (`sera-hitl`)

> **Status:** DRAFT  
> **Source:** PRD §9 (all subsections), §14 (invariant 12)  
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
    pub scope: ApprovalScope,            // ToolCall, SessionAction, MemoryWrite, ConfigChange
    pub description: String,             // Human-readable description
    pub urgency: ApprovalUrgency,        // Low, Medium, High, Critical
    pub routing: ApprovalRouting,         // How to determine the escalation chain
    pub timeout: Duration,               // Before auto-escalation to next in chain
    pub required_approvals: u32,         // For multi-approval gates (e.g., 2-of-3)
    pub evidence: ApprovalEvidence,      // Context for the approver
}

pub enum ApprovalScope {
    ToolCall(ToolRef),
    SessionAction(SessionAction),
    MemoryWrite(MemoryScope),
    ConfigChange(ConfigPath),
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
    pub additional: HashMap<String, serde_json::Value>,
}
```

---

## 3. Approval Routing

```rust
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

## 7. Approval State Machine

When an action triggers approval:

1. Session enters `WaitingForApproval` state
2. An `ApprovalTicket` is created and stored
3. Notification is delivered to the target approver(s)
4. The system waits for responses (with timeout)
5. On approval: session returns to `Active`, action executes
6. On rejection: session returns to `Active`, rejection delivered to agent
7. On timeout: escalate to next target or reject

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
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | `NeedsApproval` decision from AuthZ |
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Session state transition to WaitingForApproval |
| `sera-runtime` | [SPEC-runtime](SPEC-runtime.md) | Turn suspension during approval wait |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | `on_approval_request` hook |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | Tool risk levels trigger approval checks |

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
