# Architecture Addendum — 2026-04-13

> **Purpose:** Canonical record of architectural decisions made in the design session of 2026-04-13.
> Read this before the full spec set if you want to understand the *direction* of the architecture without reading everything.
> Each decision is cross-referenced to the spec sections it produced or modified.

---

## What Changed and Why

### 1. Gateway = Manufacturing Execution System

**Decision:** The gateway is explicitly framed as a Manufacturing Execution System (MES) for AI agents. Workers (runtimes) are cattle — ephemeral, replaceable, stateless. All durable state lives at the gateway.

**Why:** This framing resolves a recurring design ambiguity: "should the runtime hold X?" The answer is always no. Sessions, memory, audit records, credentials — all gateway-owned. A worker crash loses nothing.

**Spec impact:** SPEC-gateway §1a (new section — MES framing and mapping table).

**Key invariants introduced:**
- Sessions persist at the gateway across worker restarts
- Memory is owned and injected by the gateway, not held by the runtime
- Runtimes are stateless between turns — they receive context, run the turn loop, and return a result

---

### 2. Runtime = Library Crate, Not a Required Process

**Decision:** `sera-runtime` is a library crate embedded in the gateway by default. No process boundary, no gRPC overhead. It also compiles as a standalone binary for pet mode and as a BYOH reference implementation.

**Why:** The original spec implied the runtime was always a separate subprocess. This added latency and complexity with no benefit for the common case. The library embedding pattern (like embedding SQLite) gives the simplicity of in-process execution while preserving the option to run the runtime as a remote process (BYOH harnesses).

**Spec impact:** SPEC-runtime §1a (new section — library vs binary, deployment modes, what the runtime does and does NOT do).

**Key invariants introduced:**
- The runtime does NOT execute tools — tool dispatch is gateway-side
- The runtime does NOT hold credentials — credential resolution is gateway-side
- The runtime does NOT know the network topology — it receives an assembled context window

---

### 3. Tool Layer Belongs Entirely to the Gateway

**Decision:** Tool dispatch, AuthZ, credential injection, and execution are gateway responsibilities. The harness forwards tool call events to the gateway and receives results. It never has a direct path to any tool executor.

**Why:** This is the mechanism that makes it safe to expose sensitive infrastructure (PLCs, SCADA, enterprise APIs) to agents. The harness is untrusted cattle; the gateway is the policy enforcement boundary. Credentials never flow through the harness.

**Spec impact:**
- SPEC-tools §6 (revised tool execution flow diagram — gateway-centric)
- SPEC-gateway §1b (new section — tool dispatch ownership and security rationale)
- SPEC-tools §13a (new section — BYOH tool injection as future target)

---

### 4. Hook System Split by Layer

**Decision:** Hooks split cleanly into harness-side (operational) and gateway-side (security-critical). `pre_tool` and `post_tool` are gateway-side. This was previously implicit; it is now explicit.

**Why:** Conflating the two groups leads to incorrect security models. A harness can ignore harness-side hooks without violating security invariants — they govern formatting and context assembly, not policy. Gateway-side hooks enforce policy and cannot be bypassed by any harness, including BYOH harnesses.

**Spec impact:** SPEC-hooks §1a (new section — layer assignment table with rationale for each hook point).

---

### 5. Memory = Context Injection + Tools (Two Backends)

**Decision:** From the LLM's perspective, memory is exactly two things: injected context (arrives in the system prompt automatically) and tools (explicit LLM calls). Two backend implementations:

- **File backend (pet/standalone):** Runtime `ContextEngine` reads `soul.md`, `memory.md`, `knowledge/*.md` from workspace files
- **Gateway backend (enterprise):** Gateway assembles context from PostgreSQL + Qdrant; runtime receives an opaque context window and does not know the source

**Why:** The original spec described memory as a unified system without making the injection responsibility clear by mode. In cattle mode, the runtime never reads files directly — the gateway handles all context assembly.

**Spec impact:** SPEC-memory §1a (new section — two backends, injection responsibility by mode, switching via config).

---

### 6. Pet Mode = Gateway Always Present, Features Dial-Up

**Decision:** There is no "standalone binary vs enterprise deployment" architectural split. `sera start` = single process, full gateway, everything embedded, features disabled by default. Add features by configuration.

**Why:** Two architectural variants means two codepaths to test, document, and maintain. One system with feature activation is simpler, more testable, and easier to reason about. A developer on `sera start` runs the same gateway code as an enterprise deployment — the difference is what backends are active.

**Spec impact:** SPEC-deployment §1a (new section — feature activation model, default feature table).

---

### 7. Single Binary, All Backends Compiled In

**Decision:** Recompilation is a deployment event. Configuration is an operational event. These must never be conflated. All officially supported backends ship in the binary. Config selects which are active.

**Why:** In regulated environments, changing a binary triggers a full change management process. Changing a config file is an operational procedure. This invariant means that memory backend migrations, auth provider changes, and sandbox provider switches are all config changes — not deployments.

**Spec impact:**
- SPEC-config §1a (new section — recompilation vs configuration invariant and rationale)
- SPEC-deployment §1a (feature activation model replacing the tier-as-architecture framing)

---

### 8. Three Distinct Extension Points

**Decision:** SERA has exactly three extension points. They must not be conflated:

| Extension point | Mechanism | When to use |
|---|---|---|
| **Compiled-in backends** | Ships in binary, config selects | Switching between supported implementations |
| **WASM hooks** | Runtime-loaded, sandboxed middleware | Custom authz, content policies — small, fast, stateless, inline |
| **gRPC/RPC plugins** | Out-of-process services | Custom backends, enterprise connectors — stateful, any language |

**Why:** The original spec mentioned plugins and hooks without clearly distinguishing them. A Siemens PLC connector is a gRPC plugin — it has its own process, its own state, its own lifecycle. It is not a WASM hook and does not require gateway recompilation.

**Spec impact:** SPEC-plugins.md (new file — full gRPC plugin interface spec). SPEC-hooks §1a cross-references the distinction.

---

### 9. Gateway as Universal LLM Proxy

**Decision:** The gateway CAN act as an LLM proxy for any connected harness. Budget enforcement, cost attribution, provider routing, audit, and content filtering all happen at the gateway, opaque to the harness.

**Why:** For regulated environments (industrial, healthcare), all LLM calls MUST go through a compliance boundary. This is not optional — it is a regulatory requirement. The gateway is that boundary. By making it universal (applying to BYOH harnesses, not just the embedded runtime), the gateway becomes the single enforcement point for a heterogeneous agent ecosystem.

**Spec impact:** SPEC-gateway §13a (new section — LLM proxy surface, compliance framing, `inference.local` virtual host).

---

### 10. BYOH Tool Injection (Future Target)

**Decision:** Recorded as an explicit future architectural target: when a BYOH harness (Claude Code, Codex) connects, the gateway can augment its tool schema with sera-managed tools (`knowledge_query`, `memory_write`). These calls route through the full gateway dispatch pipeline — same hooks, same AuthZ, same audit.

**Why:** This makes the gateway a universal policy enforcement layer for a heterogeneous agent ecosystem. Constitutional gates apply to Claude Code the same way they apply to `sera-runtime`. Memory and knowledge are unified across all connected agents.

**Spec impact:** SPEC-tools §13a (future target — implementation sketch and dependencies).

---

## New Specs Added

- **SPEC-plugins.md** — gRPC/RPC plugin interface. Covers plugin registration, lifecycle, the ToolExecutor and MemoryBackend trait contracts over gRPC, mTLS security, the Siemens S7 motivating example, and SDK.

## Existing Specs Modified

| Spec | Sections added/modified |
|---|---|
| SPEC-runtime.md | §1a added — library vs binary, what runtime does/does NOT do |
| SPEC-gateway.md | §1a added (MES framing), §1b added (tool dispatch ownership), §1c added (context injection responsibility), §13a added (LLM proxy surface) |
| SPEC-hooks.md | §1a added — layer assignment table (harness-side vs gateway-side) |
| SPEC-memory.md | §1a added — two backends (file vs gateway), injection responsibility by mode |
| SPEC-tools.md | §6 revised (gateway-centric dispatch diagram), §13a added (BYOH tool injection future target) |
| SPEC-deployment.md | §1 revised (single binary framing), §1a added (feature activation model, default feature table), §2 note added |
| SPEC-config.md | §1a added — recompilation vs configuration invariant |

---

## Design Principles Established

These are the durable principles that should inform future spec and implementation decisions:

1. **Workers are cattle; state is gateway-owned.** Any state that needs to survive a worker restart must live at the gateway.
2. **Runtime is a tool consumer, not a tool executor.** The runtime forwards tool calls; the gateway dispatches and executes them.
3. **Policy hooks belong to the gateway.** Any hook that enforces a security or compliance decision must run in the gateway process.
4. **Recompilation ≠ configuration.** Switching backends is a config change; adding new backends is a code change. Never conflate them.
5. **Three extension points, never conflated.** Compiled-in (config selects), WASM hooks (inline middleware), gRPC plugins (out-of-process services).
6. **One binary, all modes.** `sera start` and an enterprise Kubernetes deployment run the same binary. The difference is what config activates.
