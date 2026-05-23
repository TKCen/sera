# SERA ↔ Hermes Functional Parity While Preserving SERA Differentiation

> **For Hermes/Sera:** This is a strategic implementation plan, not a marketing claim. Use SERA coordination / subagent-driven development to turn each workstream into dependency-gated beads and PRs. Public summaries must redact private local paths, credentials, Discord details, and operator chat contents.

**Goal:** Bring SERA to functional parity with Hermes Agent as Sera's daily operating harness, while keeping SERA's core product differentiation: gateway-owned state, policy-bound action, audit/provenance, durable work orders, governed multi-agent execution, and enterprise/industrial deployability.

**Architecture:** Do not clone Hermes as a Python feature pile. Treat Hermes as the parity benchmark for user-facing agent capability, and SERA as the accountable control plane that hosts those capabilities under a stricter execution model. The implementation order is: prove one operator workcell, close the daily-use parity gaps, then lift those capabilities into SERA-native GoalRun/workflow/audit/policy abstractions.

**Tech Stack:** Rust workspace (`sera-gateway`, `sera-runtime`, `sera-tools`, `sera-workflow`, `sera-session`, `sera-telemetry`, `sera-skills`, `sera-tui`), Axum HTTP/SSE/WebSocket, Docker Compose, MiniMax primary + local Qwen fallback, Discord connector, Beads/Kanban/OMC for execution routing, OpenTelemetry/OCSF audit, MCP/A2A/AG-UI interop where useful.

---

## 0. Current Ground Truth

### Live capability now

- The Rust gateway is reachable on `:3001` and readiness reports `runtime_connected=true`.
- The runtime can run with MiniMax primary and local Qwen fallback.
- Direct `/api/chat` can produce a session and answer a nonce probe.
- Discord connector exists and routes human messages; peer-bot handoff policy exists.
- Runtime/gateway code already has substantial scaffolding for providers, fallback chain, Discord, BYOH Pattern A/B, subagents, intercom, workflow, TUI, skills, tools, and telemetry.

### Current high-pressure ready beads relevant to parity

- `sera-m1k8`: OpenAI-compatible provider conformance with MiniMax testbed.
- `sera-duo3`: CI smoke pipeline for Discord/API ingress and egress paths.
- `sera-q66q`: Immutable audit ledger for tool execution and API calls.
- `sera-ctag`: Webhook sanitization layer for hostile inbound payloads.
- `sera-s77a`: Discord ingress local buffering and rate-limit guardrails.
- `sera-tqzd`: Tool execution failures must surface to operator and audit trail.
- `sera-rj4z`: Gateway context persistence / vector semantic memory for cross-session operator state.
- `sera-yeg.2`: Discord connector typing and processing reactions.
- `sera-yj18`: Runtime handling of LM Studio/Qwen streaming tool calls with empty content.

### Non-negotiable architectural distinction

Hermes is an excellent agent harness. SERA must be an accountable agent-operations platform.

That means parity is not achieved when SERA has the same buttons. Parity is achieved when Sera can live there without losing daily capability, and differentiation is retained when those same capabilities are mediated by gateway-owned sessions, policy, audit, HITL, workflow state, and explicit authority envelopes.

---

## 1. Parity Target Definition

SERA reaches **functional Hermes parity** when Sera can use SERA as the primary harness for normal daily work without falling back to Hermes for core operation:

1. **Conversation surfaces:** Discord and direct API are reliable, session-isolated, status-visible, and recoverable.
2. **Provider stack:** OpenAI-compatible providers, MiniMax, local models, fallback chains, model overrides, token/usage reporting, and provider error classification work consistently.
3. **Tool loop:** File, terminal, process, search/web, browser/MCP bridge, vision/media where configured, message sending, and code execution equivalents exist with visible failure semantics.
4. **Memory:** Durable user/profile/operator memory, semantic recall, session search/transcripts, compacted context, and explicit memory write tools exist.
5. **Skills/procedures:** Skill discovery, loading, creation/update, profile-scoped availability, and post-task learning loops exist.
6. **Delegation:** Subagents, BYOH harnesses, OMC/Claude/Codex/Hermes profiles, and SERA-native helpers can be launched, tracked, and audited.
7. **Scheduling and triggers:** Cron-like scheduled work, webhook triggers, event triggers, and no-agent script/watchdog equivalents exist.
8. **Control plane:** CLI/TUI/API/dashboard expose health, sessions, tools, crons/workflows, skills, logs, approvals, workers, and budget state.
9. **Voice/media:** STT/TTS/image/media support exists where configured, but voice stays operator-controlled.
10. **Operational reliability:** CI/live-smoke catches Discord/API/tool/provider regressions before false-green status reaches the operator.

SERA reaches **SERA differentiation** when the same workflows also provide:

1. Gateway-owned durable state; workers are replaceable cattle.
2. CapabilityPolicy / PrincipalPolicy enforcement before execution, not merely prompt instruction.
3. HITL approval and authority envelopes for risky action.
4. Immutable, OCSF-shaped audit/provenance for tool calls, provider calls, config changes, approvals, and workflow transitions.
5. GoalRun / WorkflowTask as first-class work orders with acceptance criteria, dependencies, stop conditions, evidence, and closeout.
6. Governed circle/perspective deliberation with final accountable integration.
7. BYOH agent containment: Pattern A for auditable tool-level control, Pattern B only when opacity is explicitly accepted.
8. Enterprise/industrial extension seams: gateway-held credentials, egress controls, plugins/connectors, and SIEM/OTel readiness.

---

## 2. Build Order

### Phase 0 — Parity Matrix and Live Baseline

**Objective:** Convert “Hermes parity” from vibe into a tracked acceptance matrix.

**Files / artifacts:**

- Create: `docs/internal/plans/hermes-parity-matrix.md`.
- Create: `rust/crates/sera-e2e-harness/tests/hermes_parity_baseline.rs` or equivalent e2e fixture.
- Update beads for uncovered gaps using the existing convention.

**Steps:**

1. Build a parity matrix with columns: Hermes capability, SERA current path, SERA target path, differentiation hook, acceptance test, owner bead, status.
2. Seed the matrix with the ten parity categories from this plan.
3. Add live probes for:
   - `GET /api/health/ready`.
   - authenticated `POST /api/chat` returning HTTP 200 + `session_id`.
   - one exact-output nonce response with reasoning/thought stripping contract checked.
   - one Discord ingress/egress smoke if credentials are available.
4. Link existing beads to rows before creating new beads.
5. Create one canonical bead per uncovered gap, not one giant “Hermes parity” epic with vague children.

**Acceptance:** The matrix can answer: “What Hermes feature would still force Sera back to Hermes today, and what bead closes it?”

---

### Phase 1 — Operator Workcell v1: Make SERA Usable Before Broad Feature Expansion

**Objective:** Prove one complete non-destructive SERA-run work order from intake to evidence-backed closeout.

**Primary existing anchors:** `sera-duo3`, `sera-tqzd`, `sera-yeg.2`, `sera-yj18`, `sera-m1k8`.

**Implementation focus:**

1. API/Discord intake reliability.
2. Session isolation and no stale-frame / one-turn-late behavior.
3. Provider conformance for MiniMax/OpenAI-compatible responses.
4. Strict response contract handling: no leaked `<think>` blocks unless explicitly configured.
5. Visible progress: typing/reaction/status-card equivalents.
6. Truthful blocked/failed/complete outcomes.

**Acceptance smoke:**

- Submit one work order through `/api/chat` and one through Discord.
- SERA creates or uses a session, records transcript, invokes at least one helper/tool path, returns a concise status card, and records audit/evidence IDs.
- If provider/tool failure occurs, the operator sees a typed failure class rather than silence or a fake-green response.

**Differentiation preserved:** The work order is not “chat completed”; it is a manufacturing order with state, evidence, and closeout truth.

---

### Phase 2 — Tool Parity Under Gateway Authority

**Objective:** Close the Hermes daily-use tool gap while moving execution authority toward gateway-owned dispatch.

**Hermes benchmark:** `terminal`, `file`, `patch`, `search_files`, `web`, `browser`, `vision`, `image_gen`, `tts`, `send_message`, `session_search`, `delegate_task`, `cronjob`, skills/memory tools.

**SERA target shape:**

1. **Tool registry:** DynamicToolSpec + Rust Tool trait unified behind one registry.
2. **Visibility != authority:** model-visible schema is independent from execution permission.
3. **Execution policy:** CapabilityPolicy / PrincipalPolicy checked per call.
4. **Result semantics:** all failures become model-visible, operator-visible, and audit-visible.
5. **Gateway dispatch migration:** move from current MVS runtime-owned tool dispatch toward `dispatch_mode=gateway` / embedded where SERA can intercept tool calls.

**Suggested PR slices:**

1. File/search/read/patch parity with file mtime conflict checks.
2. Terminal/process parity with sandbox policy and approval gates.
3. Web/browser parity via MCP/browser bridge first, native later.
4. Media/voice parity behind explicit capability flags.
5. Tool failure surfacing and audit closeout (`sera-tqzd`, `sera-q66q`).
6. Negative tests for denied tools, missing credentials, and sandbox violations.

**Acceptance:** A SERA turn can perform the same common work I do in Hermes — inspect files, patch code, run tests, search web/docs, send a message — while every call has policy, provenance, and recoverable failure state.

**Differentiation preserved:** Hermes-style convenience is allowed only when wrapped in SERA's authority/audit envelope.

---

### Phase 3 — Memory, Skills, and Session Continuity

**Objective:** Make SERA remember, retrieve, and improve procedures at least as well as Hermes, without collapsing memory into ungoverned prompt stuffing.

**Primary existing anchor:** `sera-rj4z`.

**Implementation focus:**

1. Gateway-owned durable memory backend with agent/user/session/circle scopes.
2. Two-tier injection model:
   - compact always-present MemoryBlock;
   - semantic search tool for demand retrieval.
3. Session transcript search and scroll recovery.
4. Skill pack format compatibility/import from Hermes where useful.
5. Skill lifecycle:
   - discover/list/load;
   - create/update/patch;
   - profile-scoped availability checks;
   - post-complex-task skill suggestion.
6. Memory write governance:
   - durable memory vs wisp/ephemeral scratch;
   - redaction and privacy classification;
   - audit for writes/removals.

**Acceptance:** SERA can answer “what did we decide about X?”, load relevant skills before action, retain durable operator preferences, and update procedures after hard tasks — with scoped retrieval and audit.

**Differentiation preserved:** SERA memory is not just convenience; it is scoped operator state with provenance and deployment-mode separation.

---

### Phase 4 — Delegation, BYOH Agents, and GoalRun Workflows

**Objective:** Reach Hermes delegation/cron/subagent usefulness, but represent work as SERA-native GoalRuns and WorkflowTasks.

**Implementation focus:**

1. Pattern A BYOH harnesses for auditable runtimes where possible.
2. Pattern B profiles for opaque Claude Code / OMC / Codex / Hermes sessions, explicitly marked as opaque.
3. Subagent spawn/list/status APIs become GoalRun steps, not loose helper calls.
4. Workflow engine covers:
   - cron schedule;
   - event trigger;
   - threshold trigger;
   - manual trigger;
   - watchdog/no-agent script equivalent.
5. Work order contract:
   - objective;
   - authority envelope;
   - tool/capability budget;
   - helper/delegation plan;
   - evidence requirements;
   - stop conditions;
   - evaluator;
   - closeout summary.

**Acceptance:** Any Hermes-style `delegate_task` or `cronjob` use case can be represented as a SERA GoalRun/WorkflowTask with status, logs, evidence, and a kill condition.

**Differentiation preserved:** SERA does not merely spawn agents; it governs work.

---

### Phase 5 — Messaging, Thin Clients, and Operator UX

**Objective:** Make SERA comfortable enough to use daily, not just technically correct.

**Implementation focus:**

1. Discord reliability:
   - buffering/rate-limit guardrails (`sera-s77a`);
   - typing/processing feedback (`sera-yeg.2`);
   - peer-bot handoff routing;
   - private/public channel policy.
2. Direct API:
   - stable chat contract;
   - streaming SSE events;
   - session controls;
   - response sanitizer / structured output mode.
3. TUI/CLI:
   - sessions;
   - approvals;
   - agents;
   - workflow/GoalRun state;
   - logs and health.
4. Dashboard/control plane:
   - health dots;
   - active work orders;
   - worker truth;
   - provider/budget status;
   - audit drilldown;
   - approvals queue.

**Acceptance:** Sebastian can see whether SERA is thinking, blocked, waiting for approval, using a helper, or done — without reading logs.

**Differentiation preserved:** UX is an operator console for accountable work, not a generic chatbot UI.

---

### Phase 6 — Provider, Budget, and Model Routing Parity

**Objective:** Match Hermes provider flexibility and improve it with SERA policy and observability.

**Implementation focus:**

1. OpenAI-compatible provider conformance using MiniMax as the active testbed (`sera-m1k8`).
2. Local model fallback and failure taxonomy.
3. Per-turn model override and effort controls.
4. Provider usage accounting and budget bands.
5. Routing policy:
   - high-stakes synthesis;
   - bulk/cheap work;
   - local/private lanes;
   - fallback rules;
   - no silent fallback on security-sensitive work unless allowed.
6. Redaction of raw provider/internal errors before operator-facing output.

**Acceptance:** SERA can route work across MiniMax/local/OpenAI-compatible providers with explicit fallback semantics, audit entries, and useful operator-visible failure classes.

**Differentiation preserved:** Model routing becomes an accountable policy decision, not just a config toggle.

---

### Phase 7 — Audit, Security, and Enterprise/Industrial Readiness

**Objective:** Convert SERA from “feature-comparable harness” into the thing Hermes is not trying to be: an auditable operations substrate.

**Implementation focus:**

1. Immutable audit ledger for tool execution and API calls (`sera-q66q`).
2. OCSF-shaped audit events and OpenTelemetry traces.
3. Webhook sanitization (`sera-ctag`).
4. Egress policy, SSRF validation, credential injection, and secret access audit.
5. HITL approval flows for write/execute/admin operations.
6. Policy pack tests:
   - deny dangerous command;
   - require approval;
   - redact secrets;
   - block unauthorized principal;
   - recover after worker crash.
7. Industrial plugin seams: gRPC/RPC plugins, mTLS, pinned binary/image identity, external connector isolation.

**Acceptance:** A serious operator can inspect what happened, who/what authorized it, what evidence exists, and why a risky action was allowed or denied.

**Differentiation preserved:** This is the moat. Do not trade it away for short-term parity speed.

---

## 3. Execution Rules

1. **No broad rewrite lane.** Every parity category becomes one or more narrow beads with acceptance tests.
2. **No false-green closeout.** A feature is not “parity” until direct API and, where relevant, Discord/live smoke prove it.
3. **No invisible failures.** Silent tool/provider/runtime failures are P1/P2 parity blockers because they destroy operator trust.
4. **No Pattern B laundering.** Opaque external workers are allowed, but must be labeled opaque and cannot be claimed as full SERA governance.
5. **No Hermes cargo culting.** If Hermes does it for convenience and SERA needs policy/audit, keep the convenience but change the substrate.
6. **No public leakage.** Public docs/issues/PRs must not include private chat contents, local secret names/values, home-network details, or personal details.
7. **Proof before expansion.** Each phase needs a live smoke artifact and at least one regression test before broadening scope.

---

## 4. Recommended Immediate Next Actions

### Action 1 — Create the parity matrix artifact

Create `docs/internal/plans/hermes-parity-matrix.md` from this plan with the following initial rows:

- API chat intake.
- Discord intake/egress.
- Provider/fallback conformance.
- Response sanitization / strict output.
- File/search/patch tools.
- Terminal/process tools.
- Web/browser/MCP tools.
- Memory/session search.
- Skills/procedure lifecycle.
- Delegation/subagents.
- Workflow/cron/webhooks.
- TUI/dashboard/control plane.
- Audit/HITL/policy.

### Action 2 — Turn the matrix into beads

For each row:

1. link an existing bead if one exists;
2. create a new bead only if uncovered;
3. add acceptance criteria with exact smoke/test commands;
4. assign priority based on daily-use blocker severity.

### Action 3 — Run the first live parity gate

Run one controlled SERA probe bundle:

1. readiness;
2. direct `/api/chat` nonce;
3. strict-output sanitizer expectation;
4. provider/fallback log check;
5. Discord one-turn smoke if safe;
6. tool failure visibility probe with a harmless nonexistent tool/file.

Expected output: a small JSON/Markdown artifact under `artifacts/reports/` or `docs/internal/sessions/` with pass/fail and linked beads.

### Action 4 — Seed exactly one execution lane

Do not launch a giant parity swarm. Seed one lane for the first blocker discovered by the parity gate. Likely candidates from current evidence:

- response sanitizer / hidden reasoning stripping;
- provider conformance edge cases;
- Discord/API CI smoke;
- visible tool failure/audit surfacing.

---

## 5. Phase Exit Criteria

### SERA can replace Hermes for Sera's daily operation when:

- Discord + API are reliable enough for normal conversation and work orders.
- The core tool loop covers file/search/patch/terminal/web/message/delegation/scheduling.
- Durable memory and skills work across sessions.
- Provider fallback and local model paths are observable and truthful.
- Failure states are visible rather than silent.
- SERA can run its own improvement/workcell loop with evidence-backed closeout.

### SERA is meaningfully differentiated when:

- tool execution is governed at the gateway;
- sessions/workflows/audit survive worker restart;
- every risky action has policy and HITL hooks;
- work is represented as GoalRuns/WorkflowTasks with evidence;
- opaque BYOH workers are treated honestly;
- observability/audit can satisfy an enterprise operator, not just a developer reading logs.

---

## 6. Suggested North-Star Sentence

SERA should reach Hermes-level usefulness for Sera while exceeding Hermes-level accountability: the same practical agent powers, but run as governed work orders with durable state, bounded authority, audit, provenance, and operator-visible truth.
