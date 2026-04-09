# SERA MVS Spec Review Plan

> **Status:** APPROVED (Ralplan consensus: Planner + Architect + Critic)  
> **Date:** 2026-04-09  
> **Scope:** Critical review of 19 specs to reach MVS implementation readiness  
> **Derived from:** [plan.md](plan.md) (PRD v0.3)  

---

## 1. Goal

Elevate the SERA spec suite to the point where the first MVS (Minimal Viable SERA) has a clear implementation path through transitioning the current TypeScript codebase to the new Rust target state. This means every spec section is either tagged as MVS-required or explicitly deferred, all implementation-blocking ambiguities are surfaced as open questions, and concrete artifacts (acceptance test, crate subset, tool mapping) are produced.

---

## 2. Principles

| # | Principle | Meaning |
|---|-----------|---------|
| 1 | **MVS-traceable** | Every spec section must be traceable to an MVS requirement or explicitly marked `[POST-MVS]` |
| 2 | **Implementable specificity** | Specs must have enough detail that a developer can start coding without needing to make architectural decisions mid-implementation |
| 3 | **Cross-spec consistency** | Shared concepts (Principal, Event, Session, Tool, HookContext) must be consistent in type definitions, naming, and semantics across all 19 specs |
| 4 | **Codebase-aware transition** | The review must account for what exists in the current TS codebase (agent-runtime, tools, Discord connector, context management) and identify the minimum parity target |
| 5 | **Dependency-ordered resolution** | Questions must be tagged by phase (0/1/2) so they can be resolved in build order, not all at once |

---

## 3. Decision Drivers

1. **The MVS definition is underspecified** — SPEC-migration §3 lists 7 capabilities but doesn't map them to concrete crate deliverables, API contracts, or acceptance tests
2. **The 26-crate decomposition is premature for MVS** — Phase 0 alone has 9 crates; the MVS needs only 8 total
3. **WASM hook system adds Phase 1 complexity that is not MVS-blocking** — The existing TS agent-runtime runs a reasoning loop with zero WASM hooks; native Rust extension points suffice for MVS

---

## 4. Approach

**Option B: MVS-critical path review** — Selected via consensus.

Single-pass review of all 19 specs with depth proportional to MVS relevance:
- **Deep review**: Phase 0-2 specs (13 specs on the MVS critical path)
- **Light review**: Phase 3/4 specs (6 specs — interface contracts only)

### Alternatives Considered

| Option | Verdict | Rationale |
|--------|---------|-----------|
| A: Full breadth review (all 19 equally) | Rejected | 6 specs are Phase 3/4 with no MVS bearing; dilutes focus |
| B: MVS-critical path review | **Selected** | Focused, produces actionable artifacts, fastest to implementation readiness |
| C: Bottom-up crate validation | Rejected | Crate boundaries should be driven by domain requirements, not vice versa |
| Skip review, start porting | Rejected (Architect antithesis) | Risks building wrong abstractions; but acknowledged — Phase A produces artifacts, not just analysis, and Phase B overlaps with implementation |

---

## 5. Execution Plan

### Phase A — Single-Pass MVS-Critical Review (this session)

For each spec, in one pass:
1. Tag each section as `[MVS]` or `[POST-MVS]`
2. Identify implementation-blocking gaps
3. Append concrete open questions with `[MVS-BLOCKER]` or `[DEFERRED]` tags
4. Cross-validate shared types for consistency

### Phase B — Implementation Readiness (follow-up session)

- Overlapping spec refinement and Phase 0 implementation
- Spec updates from implementation learnings
- Phase B begins immediately after Phase A, not sequentially

---

## 6. MVS Definition

### 6.1 MVS Acceptance Test

```
1. sera init
   → Interactive: pick LM Studio provider, set API key, set Discord token
   → Writes sera.yaml config file

2. sera agent create "sera"
   → Creates agent with basic tools (memory_read, memory_write, memory_search, shell, file_read, file_write, session_reset)
   → Writes agent config to sera.yaml

3. sera start
   → Gateway starts (HTTP + WS on configured ports)
   → Discord connector connects (in-process, raw WebSocket to Discord Gateway)
   → SQLite database initialized (sessions, queue, audit tables)
   → Agent registered and ready

4. Discord DM → User sends message
   → Message deduped (idempotency check)
   → Principal resolved (Discord user → auto-created principal)
   → Session created or resumed (session key: agent:sera:main)
   → Event enqueued to session lane

5. Agent turn executes
   → Event dequeued
   → Context assembled: persona + tool schemas + memory excerpts + history + current message
   → LM Studio called via OpenAI-compatible API (streaming)
   → Agent decides to call memory_write tool
   → Tool executed (file written to agent workspace)
   → Tool result re-enters model
   → Final response generated

6. Response delivered via Discord DM
   → Streaming text sent back through Discord WebSocket

7. Session persists
   → Transcript saved to SQLite
   → On sera restart, session is reloaded from SQLite
   → Next Discord DM continues the same session context
```

### 6.2 MVS Crate Subset (8 crates)

| Crate | MVS Scope | Phase |
|-------|-----------|-------|
| `sera-types` | Principal (simplified — no groups, no external agents), Event, Session, AgentId, tool metadata types | 0 |
| `sera-config` | Single-file YAML with `---`-separated typed manifests (SPEC-config §2.4 format). No directory discovery, no hot-reload, no schema registry, no version history | 0 |
| `sera-errors` | Unified error types: `GatewayError`, `RuntimeError`, `ToolError`, `MemoryError`, `ModelError`, `ConfigError`, `DbError` | 0 |
| `sera-db` | SQLite only. Tables: sessions, transcript, queue, audit_log. Simple migrations (embedded SQL). No PostgreSQL | 0 |
| `sera-memory` | File-based only. Markdown files in agent workspace. Keyword/heading search (no embeddings). No git management. Simple `index.md` + topic files | 1 |
| `sera-tools` | Built-in tools only: `memory_read`, `memory_write`, `memory_search`, `file_read`, `file_write`, `shell`, `session_reset`. No progressive disclosure, no sandbox, no profiles | 1 |
| `sera-models` | OpenAI-compatible provider only (covers LM Studio, Ollama, OpenAI, any compatible API). Streaming completion. No structured generation, no multi-model routing | 2 |
| `sera-gateway` | HTTP+WS server (axum). Discord connector (in-process, raw WebSocket). Simple session state machine (Active/Archived only). SQLite-backed queue (inline, not separate crate). Agent turn loop. Context assembly pipeline (no hook injection points). Response delivery | 2 |

### 6.3 MVS Session Scope

- **Two states only**: `Active` and `Archived`
- **No hook-driven transitions**, no `WaitingForApproval`, no `Compacting`, no `Suspended`
- **Transcript persisted to SQLite** and reloaded on startup
- **Session key**: `agent:{agent_id}:main` (single-user, single-session per agent)
- **No session scoping strategies** — just `main` for MVS

### 6.4 MVS Config Format

Single-file with `---`-separated typed manifests per SPEC-config §2.4:

```yaml
# sera.yaml
---
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: "my-sera"
spec:
  tier: "local"
---
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: "lm-studio"
spec:
  kind: "openai-compatible"
  base_url: "http://localhost:1234/v1"
  default_model: "gemma-4-12b"
---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: "sera"
spec:
  provider: "lm-studio"
  model: "gemma-4-12b"
  persona:
    immutable_anchor: |
      You are Sera, an autonomous assistant.
  tools:
    allow: ["memory_*", "file_*", "shell", "session_*"]
---
apiVersion: sera.dev/v1
kind: Connector
metadata:
  name: "discord-main"
spec:
  kind: "discord"
  token: { secret: "connectors/discord-main/token" }
  agent: "sera"
```

Directory discovery, hot-reload, schema registry, version history, and rollback are all `[POST-MVS]`.

### 6.5 MVS Auth

**Autonomous mode** (Tier 1):
- No login required
- Gateway auto-creates a default `admin` principal on first start
- Discord users are auto-mapped to principals by Discord user ID
- All principals have full access (no RBAC for MVS)
- API key auth for programmatic access (single bootstrap key in config)

### 6.6 Explicit Deferral List

| Feature | Spec | Rationale |
|---------|------|-----------|
| WASM hook system | SPEC-hooks | wasmtime dependency; 11 hook points add complexity; native Rust extension points suffice for MVS |
| K8s-style config (directory discovery, hot-reload, schema registry) | SPEC-config | Single-file YAML suffices |
| Embedding-based memory search | SPEC-memory | Requires local embedding model; keyword search suffices |
| Git memory management | SPEC-memory | Auto-commit, branches, conflict resolution — file-write suffices |
| Multi-model routing | SPEC-runtime | One model per agent suffices |
| Structured generation | SPEC-runtime | Nice-to-have but not MVS-blocking |
| Dynamic model parameters / sampler profiles | SPEC-runtime | Single parameter set suffices |
| Dreaming workflow | SPEC-workflow-engine | Background consolidation is post-MVS |
| HITL approval system | SPEC-hitl-approval | Autonomous mode = no approvals needed |
| PostgreSQL support | SPEC-deployment | SQLite for Tier 1 |
| Redis cache | SPEC-deployment | In-memory cache suffices |
| Interop protocols (MCP, A2A, ACP, AG-UI) | SPEC-interop | All Phase 3+ |
| Clients (CLI, TUI, Web, SDK) | SPEC-clients | Gateway + Discord is the MVS interface; `sera init/start/agent` are subcommands of the gateway binary |
| Thin clients / HMIs | SPEC-thin-clients | Phase 3+ |
| Circles (multi-agent coordination) | SPEC-circles | Phase 4 |
| Enterprise auth (OIDC, SCIM, AuthZen, SSF) | SPEC-identity-authz | Phase 4 |
| OpenTelemetry export | SPEC-observability | Basic `tracing` crate logging suffices; no OTel collector needed |
| PII tokenization | SPEC-security | Defense-in-depth, not MVS-required |
| Persona introspection | SPEC-runtime | Mutable persona overflow handling is post-MVS |
| Skills system | SPEC-runtime | Formal SkillDef is post-MVS; agent persona handles instructions |
| Progressive tool disclosure | SPEC-tools | < 15 tools, all injected |
| Sandbox execution | SPEC-tools | Shell runs locally for MVS |
| Tool profiles | SPEC-tools | Simple allow list suffices |
| Credential injection hooks | SPEC-secrets | Env-based secrets resolved directly, no hook chain |
| Side-routed secret entry | SPEC-secrets | Secrets configured by operator, not requested by agent |
| Webhook ingress/egress | SPEC-gateway | Not MVS-required |
| Queue modes (steer, collect, followup) | SPEC-gateway | Simple FIFO suffices |
| Session state machine extensibility | SPEC-gateway | Two fixed states suffice |
| External runtimes (gRPC) | SPEC-runtime | In-process only |
| External tools (gRPC) | SPEC-tools | Built-in only |
| Cost attribution | SPEC-observability | Post-MVS |
| Run evidence / proof bundles | SPEC-observability | Post-MVS |
| Config version history / rollback | SPEC-config | Post-MVS |
| Interface versioning (all surfaces) | SPEC-versioning | Define conventions now, enforce post-MVS |

---

## 7. MVS Architecture (Pruned)

```
┌─────────────────────────────────────────────┐
│  sera start (single binary)                  │
│                                              │
│  ┌────────────────────────────────────────┐  │
│  │  sera-gateway                          │  │
│  │  ├── HTTP/WS server (axum)             │  │
│  │  ├── Discord connector (in-process)    │  │
│  │  │   └── WebSocket to Discord Gateway  │  │
│  │  ├── Event router (no hooks)           │  │
│  │  ├── SQLite-backed queue (inline)      │  │
│  │  ├── Session manager (Active/Archived) │  │
│  │  └── Agent turn loop                   │  │
│  │      ├── Context assembly              │  │
│  │      │   ├── Persona injection         │  │
│  │      │   ├── Tool schema injection     │  │
│  │      │   ├── Memory injection          │  │
│  │      │   ├── History injection          │  │
│  │      │   └── Current turn              │  │
│  │      ├── Model call (streaming)        │  │
│  │      ├── Tool execution loop           │  │
│  │      ├── Memory write                  │  │
│  │      └── Response delivery             │  │
│  └────────────────────────────────────────┘  │
│                                              │
│  ┌──────────────┐  ┌──────────────────────┐  │
│  │ sera-models   │  │ sera-memory          │  │
│  │ OpenAI-compat │  │ File-based           │  │
│  │ (LM Studio)   │  │ (keyword search)     │  │
│  └──────────────┘  └──────────────────────┘  │
│                                              │
│  ┌──────────────┐  ┌──────────────────────┐  │
│  │ sera-tools    │  │ sera-db (SQLite)     │  │
│  │ 7 built-in    │  │ sessions, transcript │  │
│  │ tools          │  │ queue, audit_log     │  │
│  └──────────────┘  └──────────────────────┘  │
│                                              │
│  ┌──────────────┐  ┌────────┐  ┌──────────┐ │
│  │ sera-config   │  │sera-   │  │sera-     │ │
│  │ (single YAML) │  │types   │  │errors    │ │
│  └──────────────┘  └────────┘  └──────────┘ │
└─────────────────────────────────────────────┘
         │                    │
         ▼                    ▼
   Discord Gateway       LM Studio
   (WebSocket)           (HTTP REST)
```

---

## 8. Critical Findings to Address

| # | Finding | Severity | Resolution | Spec |
|---|---------|----------|------------|------|
| 1 | MVS has no acceptance test | HIGH | Add §6.1 above to SPEC-migration | SPEC-migration |
| 2 | Built-in tool catalog undefined | HIGH | Add TS→MVS tool mapping to SPEC-tools | SPEC-tools |
| 3 | Tier 1 auth "autonomous mode" undefined | HIGH | Add §6.5 above to SPEC-identity-authz | SPEC-identity-authz |
| 4 | WASM hooks over-scoped for MVS | HIGH | Defer entire SPEC-hooks to post-MVS; define native extension points | SPEC-hooks |
| 5 | Config system over-engineered for MVS | HIGH | Scope to single-file manifest mode (§6.4) | SPEC-config |
| 6 | LM Studio/model provider specifics missing | MEDIUM | Document OpenAI-compat API specifics | SPEC-runtime |
| 7 | Discord connector spec missing | HIGH | Add subsection to SPEC-gateway | SPEC-gateway |
| 8 | Queue as separate crate unnecessary for MVS | MEDIUM | Inline into sera-gateway | SPEC-crate-decomposition |
| 9 | No error handling strategy | MEDIUM | Define sera-errors taxonomy | SPEC-crate-decomposition |
| 10 | Phase timing optimistic | LOW | Acknowledge; realistic timing TBD with team size | SPEC-migration |
| 11 | Existing TUI disconnected from crate plan | LOW | Acknowledge; TUI is post-MVS | SPEC-crate-decomposition |
| 12 | Dual DB support in Phase 0 unnecessary | MEDIUM | SQLite-only for MVS | SPEC-deployment |
| 13 | No pruned MVS architecture | MEDIUM | Add §7 above to SPEC-crate-decomposition | SPEC-crate-decomposition |

---

## 9. TS-to-MVS Tool Mapping

| TS Tool (current codebase) | MVS Tool (Rust) | Status |
|---------------------------|-----------------|--------|
| `knowledge-store` | `memory_write` | MVS-INCLUDE (renamed) |
| `knowledge-query` | `memory_search` | MVS-INCLUDE (renamed) |
| `knowledge-update` | `memory_write` (overwrite mode) | MVS-INCLUDE (merged) |
| `knowledge-delete` | — | MVS-DEFER |
| `knowledge-rewrite` | — | MVS-DEFER |
| `core-memory-append` | `memory_write` | MVS-INCLUDE (merged) |
| `core-memory-replace` | `memory_write` | MVS-INCLUDE (merged) |
| `file-read` | `file_read` | MVS-INCLUDE |
| `file-write` | `file_write` | MVS-INCLUDE |
| `file-list` | — | MVS-DEFER |
| `shell-exec` | `shell` | MVS-INCLUDE |
| `web-search` | — | MVS-DEFER |
| `web-fetch` | — | MVS-DEFER |
| `code-eval` | — | MVS-DEFER |
| `http-request` | — | MVS-DEFER |
| `schedule-task` | — | MVS-DEFER |
| `delegate-task` | — | MVS-DEFER |
| `manage-agent` | — | MVS-DEFER |
| `image-view` | — | MVS-DEFER |
| `pdf-read` | — | MVS-DEFER |
| `conversation-search` | `memory_search` | MVS-INCLUDE (merged) |
| — (new) | `session_reset` | MVS-INCLUDE |
| — (new) | `memory_read` | MVS-INCLUDE |

**MVS Tool Catalog (7 tools):**
1. `memory_read` — Read a specific memory file by path
2. `memory_write` — Write/append/overwrite a memory file
3. `memory_search` — Keyword/heading search across memory files
4. `file_read` — Read a file from the agent workspace
5. `file_write` — Write a file to the agent workspace
6. `shell` — Execute a shell command (local, no sandbox)
7. `session_reset` — Archive current session and start fresh

---

## 10. Review Checklist per Spec

For each of the 19 specs, the reviewer should:

- [ ] Tag each section as `[MVS]` or `[POST-MVS]`
- [ ] Verify cross-references are accurate
- [ ] Check shared type definitions match sera-types
- [ ] Identify implementation-blocking ambiguities
- [ ] Append open questions with `[MVS-BLOCKER]` or `[DEFERRED]` tags
- [ ] Verify phase assignments match SPEC-migration and README index
- [ ] For Phase 0-2 specs: verify enough detail for implementation
- [ ] For Phase 3/4 specs: verify interface contracts don't conflict with Phase 0-2

---

## 11. Consensus Record

| Role | Verdict | Key Feedback |
|------|---------|-------------|
| Planner | Draft | 6-phase review with 10 findings |
| Architect | ITERATE | Compress to 2 phases; add MVS crate subset (8 not 26); defer WASM hooks; add Discord spec; add pruned architecture; acknowledged antithesis (analysis paralysis risk) |
| Planner | Revised | Incorporated all Architect feedback |
| Critic | ACCEPT-WITH-RESERVATIONS | 3 major additions: TS→MVS tool mapping, MVS session scope statement, MVS config format decision; minor: CLI as gateway subcommands, basic tracing logging, streaming in scope |
| **Final** | **APPROVED** | All reservations incorporated |
