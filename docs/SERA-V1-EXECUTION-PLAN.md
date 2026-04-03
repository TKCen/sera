# SERA V1 Execution Plan: BYOH Interface, Rust Bootstrap, Universal Skill Registry

**Date:** 2026-04-03
**Status:** APPROVED — Consensus reached (Planner + Architect + Critic, iteration 1)
**Streams:** 3 (BYOH Interface | Rust Bootstrap | Skill Registry)
**Estimated PRs:** 14-18 across all streams

---

## RALPLAN-DR Summary

### Principles

1. **Container boundary is the API** — SERA's value proposition is that ANY process inside its containers inherits governance (egress proxy, LLM metering, audit) via environment variables and network topology, not code-level integration.
2. **OpenAI-compat is the universal adapter** — Any harness that speaks the OpenAI `/v1/chat/completions` SSE protocol can route LLM calls through sera-core's proxy. Do not invent a new protocol.
3. **Strangler fig, not big bang** — Rust migration proceeds subsystem-by-subsystem with dual-stack coexistence. No service goes dark during migration.
4. **Skills are data, not code** — The skill registry treats skills as declarative manifests with metadata. Execution is delegated to whatever runtime the harness provides.
5. **Minimal mandatory contract** — The BYOH interface must be small enough that a shell script could implement it. Health endpoint + env var consumption + stdout result. Everything else is optional enrichment.

### Decision Drivers (Top 3)

1. **Time to third-party adoption** — How quickly can someone run a non-SERA agent harness (OMC, OpenClaw, LangChain, custom Python) inside a SERA container and get metering + audit for free?
2. **Migration safety** — Can the Rust migration proceed without breaking existing TypeScript agents or requiring a flag day?
3. **Skill portability** — Can skills written for one harness be discoverable (not necessarily executable) by agents running a different harness?

### Viable Options Per Stream

#### Stream 1: BYOH Container Interface

**Option A: Env-var-only contract (RECOMMENDED)**

- Harness reads `SERA_CORE_URL`, `SERA_IDENTITY_TOKEN`, `SERA_LLM_PROXY_URL` (new), `HTTP_PROXY`/`HTTPS_PROXY`, `AGENT_INSTANCE_ID`
- Harness implements `GET /health` on `AGENT_CHAT_PORT` (configurable, default 3100)
- Harness optionally implements `POST /chat` for interactive mode
- Harness writes result JSON to stdout (ephemeral) or posts to `/api/agents/:id/tasks/:taskId/complete` (persistent)
- Pros: Zero code dependency on SERA. Works with any language. Existing containers already do 90% of this.
- Cons: No structured thought streaming without Centrifugo integration. Limited observability for non-SERA harnesses.

**Option B: SDK-based contract**

- Provide a thin SDK (TypeScript + Python + Rust) that wraps the env vars, health endpoint, and Centrifugo publishing
- Pros: Richer observability out of the box. Thought streaming works immediately.
- Cons: Adds a code dependency. SDK maintenance burden per language. Slower third-party adoption.

**Invalidation rationale for Option B:** The SDK approach contradicts Principle 1 (container boundary is the API). The env-var contract already gives harnesses LLM access, egress control, and identity. Thought streaming via Centrifugo is a nice-to-have that can be added incrementally via an optional sidecar or library, not a mandatory contract requirement.

#### Stream 2: Rust Migration Bootstrap

**Option A: Minimal BYOH Rust agent (proof-of-concept) (RECOMMENDED)**

- A thin Rust binary that reads stdin, proxies to `SERA_LLM_PROXY_URL`, writes stdout. No tool execution, no context compaction, no Centrifugo, no stream wrappers.
- Uses `spec.sandbox.image` (Milestone 2.1) like any other BYOH container — no special `runtime: ts | rust` field needed.
- Proves Rust toolchain, Docker multi-stage image build, and LLM proxy integration end-to-end.
- Pros: Bounded scope (~500 LOC). High-value win (validates Rust in SERA containers). Zero control-plane risk. Reuses BYOH contract directly.
- Cons: Does not replace the TypeScript agent-runtime. No tool loop, no context management.

**Option B: Full sera-runtime-rs (port of agent-runtime)**

- Port the entire TypeScript agent-runtime (11,319 LOC across 20+ files including loop.ts at 920 LOC, contextManager.ts at 759 LOC, executor.ts at 678 LOC, toolLoopDetector at 242 LOC, toolArgumentRepair at 326 LOC, centrifugo publisher at 227 LOC, plus 10+ tool handlers across 8 files) to Rust.
- Cons: Massive scope for v1. The agent-runtime is not "~1500 LOC" — it is 11,319 LOC total (6,936 source-only). A full port is a multi-month effort.

**Invalidation rationale for Option B:** The original plan underestimated agent-runtime complexity by 4.6x (claimed ~1,500 LOC; actual source is 6,936 LOC). A full Rust port includes context compaction, tool loop detection, tool argument repair, boot context loading, stream wrappers for reasoning models, Centrifugo publishing, and 10+ tool handlers. This is not achievable in a single milestone. The minimal BYOH Rust agent validates the toolchain and Docker pipeline without attempting to replicate this complexity. A full sera-runtime-rs is deferred to a later phase.

**Option C: Start with sera-core-rs read-only shadow**

- Build health + read-only API endpoints in Rust alongside TypeScript sera-core.
- Cons: Larger initial scope. Requires database schema understanding. Longer time to first visible result.

Both A and C are viable. Option A is recommended because it delivers a user-visible result faster and directly validates the BYOH contract with a non-TypeScript harness.

#### Stream 3: Universal Skill Registry

**Option A: Manifest-index with volume mounts (RECOMMENDED)**

- SERA maintains a skill registry with declarative manifests (name, description, parameters JSON Schema, harness compatibility tags)
- Skills are mounted as read-only volumes at `/sera/skills/` inside containers
- Each skill directory contains a `sera-skill.json` manifest + the native skill files
- The harness reads the manifest index and decides how to execute (or ignore) each skill
- Pros: Works across harnesses. No execution coupling. OpenClaw skills just need a manifest wrapper.
- Cons: Harness must understand how to execute the native format. Discovery without execution is of limited value.

**Option B: Enforce SERA's RuntimeToolExecutor standard (TypeScript-only)**

- All skills must implement SERA's tool interface (name, parameters JSON Schema, execute function).
- Pros: Uniform execution. Full observability.
- Cons: Incompatible with existing OpenClaw/LangChain skill ecosystems. Kills third-party adoption.

**Option C: Language-agnostic execution interface (JSON-RPC / stdin-stdout)**

- Define a universal execution protocol (e.g., JSON-RPC over stdin/stdout or HTTP) that any language can implement.
- Pros: Cross-language execution, not just discovery. Richer than manifest-only.
- Cons: Significant design effort. Requires agreement on I/O format, error handling, streaming. Adds mandatory contract complexity that contradicts Principle 5.

**Invalidation rationale for Options B and C:** Option B is TypeScript-only, which contradicts the BYOH premise entirely. Option C (language-agnostic execution interface) is architecturally sound but adds significant contract complexity. Defining a universal execution protocol (argument serialization, streaming output, error codes, timeout handling) is a substantial design effort that would delay v1 delivery. The manifest-index approach (Option A) provides immediate discovery value while Option C can be layered on top in v2 as an optional execution protocol. The key insight is that discovery is the v1 deliverable; execution standardization is a v2 concern.

---

## ADR: BYOH Container Interface

**Decision:** Adopt the env-var-only contract (Option A) as the standard BYOH interface.

**Drivers:** Time to third-party adoption; minimal mandatory contract principle; existing container architecture already supports 90% of the interface.

**Alternatives considered:**

- SDK-based contract (Option B) — rejected because it adds a code dependency that contradicts the container-boundary-is-the-API principle.

**Why chosen:** The current SandboxManager already injects `SERA_CORE_URL`, `SERA_IDENTITY_TOKEN`, `HTTP_PROXY`/`HTTPS_PROXY`, `AGENT_NAME`, `AGENT_INSTANCE_ID` into every container. Adding `SERA_LLM_PROXY_URL` (pointing to `${SERA_CORE_URL}/v1/llm`) gives any OpenAI-compatible client automatic metering. The egress proxy means ANY outbound HTTP (regardless of harness) is filtered. The contract is: read env vars, respond to `/health`, optionally respond to `/chat`, write results to stdout.

**Consequences:**

- Harnesses without Centrifugo integration will not have real-time thought streaming in the web UI (acceptable for v1; can be added via optional sidecar later).
- SERA cannot enforce tool-level audit for non-SERA harnesses (only LLM-call-level audit via the proxy). This is acceptable because the egress proxy still captures all network activity.
- The `sera-agent-worker:latest` image remains the default but templates can specify any Docker image via `spec.sandbox.image`.
- Egress proxy compliance via `HTTP_PROXY`/`HTTPS_PROXY` is advisory for v1 — see Risk Register for honest assessment and Milestone 1.5 for enforcement work.

**Follow-ups:**

- Document the BYOH contract in `docs/BYOH-CONTRACT.md`
- Add `SERA_LLM_PROXY_URL` env var injection to SandboxManager
- Create example BYOH containers (Python, shell script) for the docs
- Design optional Centrifugo sidecar for thought streaming in v2

---

## ADR: Universal Skill Registry

**Decision:** Adopt manifest-index with volume mounts (Option A) with discovery-only schema for v1.

**Drivers:** Skill portability across harnesses; compatibility with OpenClaw's 14,000+ community skills; minimal execution coupling.

**Alternatives considered:**

- Enforce SERA's RuntimeToolExecutor standard (Option B) — rejected because it requires all skills to be TypeScript, killing cross-harness portability.
- Language-agnostic execution interface via JSON-RPC/stdin-stdout (Option C) — deferred to v2. Architecturally sound but adds significant contract complexity and design effort that would delay v1. Discovery is the v1 deliverable; execution standardization is a v2 concern.

**Why chosen:** The manifest-index approach treats skills as data (JSON Schema parameters, description, compatibility tags) rather than code. SERA mounts skill directories as read-only volumes. Each harness reads the index and executes skills it understands natively. This means an OpenClaw skill package just needs a `sera-skill.json` wrapper to be discoverable by any SERA-hosted agent, even if execution requires the OpenClaw runtime.

**Consequences:**

- A skill may be discoverable but not executable by a given harness. The manifest includes `compatibleHarnesses` tags so agents can filter.
- SERA's built-in tools (file-read, shell-exec, etc.) are exposed via the same manifest format, making them discoverable by guest harnesses.
- The skill registry API (`/api/skills`) returns the combined catalog; the volume mount at `/sera/skills/` provides filesystem access inside containers.
- v1 schema is discovery-only (no `entrypoint` or `runtime` fields). Execution semantics are deferred to v2.

**Follow-ups:**

- Define `sera-skill.json` schema in `schemas/sera-skill.schema.json`
- Write OpenClaw skill adapter that generates `sera-skill.json` from OpenClaw plugin manifests
- Add `/api/skills/registry` endpoint to sera-core
- Design v2 execution protocol (JSON-RPC or stdin/stdout) layered on top of manifest-index

---

## Execution Milestones

### Milestone 1: BYOH Contract Definition & Env Var Injection

**Dependencies:** None (can start immediately)
**Estimated PRs:** 2

#### 1.1 Define and document the BYOH contract

**Deliverables:**

- New file: `docs/BYOH-CONTRACT.md` specifying the full interface:
  - **Mandatory env vars:** `SERA_CORE_URL`, `SERA_IDENTITY_TOKEN`, `SERA_LLM_PROXY_URL` (new), `AGENT_NAME`, `AGENT_INSTANCE_ID`, `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`, `AGENT_CHAT_PORT`
  - **Observability env vars (reserved for v2 sidecar):** `SERA_CENTRIFUGO_URL`, `SERA_CENTRIFUGO_CHANNEL` — defined now to prevent contract-breaking changes later. Not injected in v1 but documented as the future path for thought streaming.
  - **Mandatory endpoint:** `GET /health` on `AGENT_CHAT_PORT` (default 3100) returning `{ "ready": bool, "busy": bool }`
  - **Optional endpoint:** `POST /chat` accepting `{ "message": string, "sessionId": string, "history?": ChatMessage[], "messageId?": string }` returning `{ "result": string | null, "error?": string }`
  - **Task input:** JSON on stdin: `{ "taskId": string, "task": string, "context?": string }`
  - **Task output:** JSON on stdout: `{ "taskId": string, "result": string | null, "error?": string }`
  - **Persistent mode:** Poll `GET ${SERA_CORE_URL}/api/agents/${AGENT_INSTANCE_ID}/tasks/next` and post results to `POST ${SERA_CORE_URL}/api/agents/${AGENT_INSTANCE_ID}/tasks/${taskId}/complete`
  - **LLM access:** POST to `${SERA_LLM_PROXY_URL}/chat/completions` with `Authorization: Bearer ${SERA_IDENTITY_TOKEN}` — standard OpenAI-compatible request/response
  - **Heartbeat:** Persistent-mode containers SHOULD POST to `${SERA_CORE_URL}/api/agents/${AGENT_INSTANCE_ID}/heartbeat` at `AGENT_HEARTBEAT_INTERVAL_MS` intervals (default 30000). Failure to heartbeat will result in the instance being marked `unresponsive` by `HeartbeatService`.
  - **Graceful shutdown:** Containers MUST handle `SIGTERM`. SERA sends SIGTERM with a 30-second timeout before SIGKILL. Containers should flush state and exit cleanly.
  - **Resource limits:** Containers inherit tier-based CPU/memory limits from `SandboxBoundary`. These are enforced by Docker via `HostConfig.CpuShares` and `HostConfig.Memory`. The BYOH contract does not need to implement resource limiting — Docker enforces it.
- New file: `schemas/byoh-contract.schema.json` with JSON Schema for task input/output and chat request/response

**Acceptance criteria:**

- [ ] `docs/BYOH-CONTRACT.md` exists and covers all 7 interface dimensions (task receive, result report, LLM access, security inheritance, health/status, heartbeat, graceful shutdown)
- [ ] Document includes reserved observability env vars (`SERA_CENTRIFUGO_URL`, `SERA_CENTRIFUGO_CHANNEL`) with "reserved for v2" annotation
- [ ] Document includes resource limit documentation (CPU/memory inherited from SandboxBoundary)
- [ ] JSON Schema validates against existing agent-runtime's actual stdin/stdout payloads
- [ ] Schema includes examples for each payload type

#### 1.2 Add `SERA_LLM_PROXY_URL` and dynamic `AGENT_CHAT_PORT` to SandboxManager

**Deliverables:**

- Modify `core/src/sandbox/SandboxManager.ts` line ~92: add `SERA_LLM_PROXY_URL=${SERA_CORE_URL}/v1/llm` to the env array
- Modify `core/src/sandbox/ContainerSecurityMapper.ts`:
  - Line ~63: change hardcoded `ExposedPorts: { '3100/tcp': {} }` to read port from manifest (`spec.sandbox.chatPort ?? 3100`) and expose dynamically
  - Line ~52: add `sera-core` to `NO_PROXY` if not already present (it is — verify)
- Modify `core/src/sandbox/SandboxManager.ts` line ~224: change hardcoded `http://${chatIp}:3100` to use the dynamic port from manifest
- Modify `core/src/sandbox/SandboxManager.ts` line ~97: change hardcoded `AGENT_CHAT_PORT=3100` to use the dynamic port
- Update `core/src/sandbox/SandboxManager.test.ts`: assert `SERA_LLM_PROXY_URL` is present in spawned container env
- Update agent-runtime `llmClient.ts` to prefer `SERA_LLM_PROXY_URL` over constructing the URL from `SERA_CORE_URL + /v1/llm`

**Acceptance criteria:**

- [ ] `SERA_LLM_PROXY_URL` appears in env of every spawned container
- [ ] `AGENT_CHAT_PORT` is configurable per manifest (default 3100 when unset)
- [ ] `ContainerSecurityMapper` exposes the correct port dynamically
- [ ] `SandboxManager` polls the correct port for chat readiness
- [ ] Existing agent-runtime works unchanged (backward compat: falls back to `SERA_CORE_URL + /v1/llm`)
- [ ] A non-SERA container (e.g. `curl` inside a test container) can POST to `$SERA_LLM_PROXY_URL/chat/completions` with the identity token and get a response
- [ ] Unit tests pass

---

### Milestone 1.5: Egress Proxy Enforcement

**Dependencies:** Milestone 1 (contract must be defined first)
**Estimated PRs:** 1

#### 1.5.1 Implement network-level egress enforcement on `agent_net`

**Context:** `HTTP_PROXY`/`HTTPS_PROXY` env vars are advisory — any container process can ignore them and connect directly to the internet. This is a real security gap, not a theoretical risk.

**Deliverables:**

- Investigate feasibility of iptables/nftables rules on the `agent_net` Docker network that DROP all outbound traffic not destined for the egress proxy, sera-core, or centrifugo
- If feasible: create a startup script or Docker Compose network configuration that applies the rules. Add to `docker-compose.yaml` (or a sidecar container with `NET_ADMIN` that configures the rules on `agent_net`)
- If NOT feasible in Docker Desktop (Windows/macOS) or standard Docker CE: honestly document this as a **known limitation** in `docs/BYOH-CONTRACT.md` with the heading "Security Limitations" — explain that egress proxy compliance is advisory on platforms where iptables rules cannot be applied to Docker networks, and that production deployments on Linux can enforce via iptables
- Add a negative compliance test: from inside a test container on `agent_net`, attempt `curl` to an external IP (bypassing proxy). The test asserts this fails (if enforcement is active) or is documented as a known gap (if not)

**Acceptance criteria:**

- [ ] Either: iptables rules are applied and the negative test passes (direct outbound fails)
- [ ] Or: limitation is honestly documented in `docs/BYOH-CONTRACT.md` with platform-specific guidance, and the negative test is marked as `skip` with a comment explaining why
- [ ] The risk register in this plan is updated to reflect the actual outcome

---

### Milestone 2.0: Image Allowlist in SandboxBoundary

**Dependencies:** None (can start in parallel with M1)
**Estimated PRs:** 1

#### 2.0.1 Add `allowedImages` to SandboxBoundary schema and enforcement

**Context:** `spec.sandbox.image` (Milestone 2.1) allows templates to specify arbitrary Docker images. Without an allowlist, any image can be used regardless of security tier. This is a prerequisite for safely enabling custom images.

**Deliverables:**

- Extend `SandboxBoundarySchema` in `core/src/agents/schemas.ts` (line ~162): add `spec.allowedImages?: string[]` (glob patterns, e.g. `["sera-agent-worker:*", "python:3.*-slim"]`)
- Extend `SandboxBoundary` type accordingly
- Update tier YAML files in `sandbox-boundaries/`:
  - `tier-1.yaml`: broad allowlist (e.g. `["*"]` or a curated list)
  - `tier-2.yaml`: moderate allowlist (e.g. `["sera-agent-worker:*", "python:3.*-slim", "node:*-slim"]`)
  - `tier-3.yaml`: strict allowlist (e.g. `["sera-agent-worker:latest"]` only)
- Add validation in `Orchestrator.startInstance()` or `ContainerSecurityMapper`: before spawning, check that the resolved image matches at least one `allowedImages` pattern for the agent's tier. Reject with a clear error if not.
- Unit test: tier-3 agent with `spec.sandbox.image: python:3.12-slim` is rejected
- Unit test: tier-1 agent with `spec.sandbox.image: python:3.12-slim` is allowed

**Acceptance criteria:**

- [ ] `SandboxBoundary` schema accepts `allowedImages` array
- [ ] All three tier YAMLs have explicit `allowedImages` entries
- [ ] Image validation runs before container creation
- [ ] Rejection produces a clear error message referencing the tier and allowlist
- [ ] Unit tests cover allow and deny cases

---

### Milestone 2.1: Custom Image Support & Template Schema Extension

**Dependencies:** Milestone 2.0 (image allowlist must exist before custom images are usable)
**Estimated PRs:** 2

#### 2.1.1 Add `spec.sandbox` sub-object to template schema

**Context:** `AgentManifest.spec` currently has no `sandbox` sub-object — it is flat. This requires creating a new `SandboxSpec` interface and wiring it through types, Zod schema, and JSON schema.

**Deliverables:**

- Create `SandboxSpec` interface in `core/src/agents/manifest/types.ts`:
  ```typescript
  export interface SandboxSpec {
    image?: string; // Docker image (default: sera-agent-worker:latest)
    entrypoint?: string[];
    command?: string[];
    chatPort?: number; // default 3100
  }
  ```
- Add `sandbox?: SandboxSpec` to the `spec` block of `AgentManifest` interface (line ~135 of `core/src/agents/manifest/types.ts`)
- Add `sandbox` to `KNOWN_TOP_LEVEL_FIELDS` set if it appears at top level, or handle it within the `spec` block parsing
- Extend Zod schema in `core/src/agents/schemas.ts` (line ~23, inside the `spec` z.object): add `sandbox: z.object({ image: z.string().optional(), entrypoint: z.array(z.string()).optional(), command: z.array(z.string()).optional(), chatPort: z.number().int().min(1).max(65535).optional() }).optional()`
- Extend `schemas/agent-manifest.schema.json` with the new `spec.sandbox` block
- Modify `ContainerSecurityMapper.mapSecurityOptions()` to read `manifest.spec?.sandbox?.image`, `manifest.spec?.sandbox?.entrypoint`, `manifest.spec?.sandbox?.command` and pass to Docker `createContainer` as `Image`, `Entrypoint`, `Cmd`
- Modify `SandboxManager.spawn()` image resolution: `manifest.spec?.sandbox?.image ?? request.image ?? 'sera-agent-worker:latest'`
- Validate image against the SandboxBoundary allowlist (from M2.0) before spawning

**Acceptance criteria:**

- [ ] A template YAML with `spec.sandbox.image: python:3.12-slim` spawns a container using that image (if allowed by tier)
- [ ] Templates without `spec.sandbox` continue to use `sera-agent-worker:latest`
- [ ] Custom `entrypoint` and `command` are passed to Docker `createContainer`
- [ ] Zod validation rejects invalid image names (empty string, whitespace)
- [ ] Zod validation rejects invalid `chatPort` (0, negative, >65535)
- [ ] `schemas/agent-manifest.schema.json` updated
- [ ] Image is validated against SandboxBoundary allowlist
- [ ] Integration test: spawn a container with `image: alpine:latest`, `command: ["sh", "-c", "echo hello"]`, verify stdout contains "hello" (requires Docker — mark as integration)

---

### Milestone 3: BYOH Example Containers

**Dependencies:** Milestones 1, 2.1
**Estimated PRs:** 1

#### 3.1 Create example BYOH agent images

**Deliverables:**

- `examples/byoh-python/` — A minimal Python agent that:
  - Reads task from stdin
  - Calls `$SERA_LLM_PROXY_URL/chat/completions` using `requests` library
  - Implements `GET /health` on `$AGENT_CHAT_PORT` using Flask/http.server
  - Implements heartbeat POST to `${SERA_CORE_URL}/api/agents/${AGENT_INSTANCE_ID}/heartbeat`
  - Handles SIGTERM gracefully
  - Writes result JSON to stdout
  - Includes `Dockerfile` and `sera-template.yaml`
- `examples/byoh-shell/` — A shell-script agent that:
  - Reads task from stdin via `read`
  - Calls the LLM proxy via `curl`
  - Implements health via `nc -l` or a simple while loop
  - Handles SIGTERM via `trap`
  - Writes result to stdout
  - Includes `Dockerfile` and `sera-template.yaml`
- `examples/byoh-langchain/` — A LangChain Python agent that:
  - Uses `ChatOpenAI(base_url=os.environ["SERA_LLM_PROXY_URL"], api_key=os.environ["SERA_IDENTITY_TOKEN"])`
  - Demonstrates that LangChain agents get SERA metering for free

**Acceptance criteria:**

- [ ] Each example has a working `Dockerfile` that builds
- [ ] Each example's template YAML uses `spec.sandbox.image` pointing to its image
- [ ] The Python example can be spawned via sera-core and returns a result
- [ ] The Python example sends heartbeats and handles SIGTERM
- [ ] The LangChain example demonstrates token usage appearing in SERA metering dashboard
- [ ] Each example has a README explaining the BYOH contract it implements

---

### Milestone 4: Rust Workspace Bootstrap

**Dependencies:** None (parallel with Milestones 1-3)
**Estimated PRs:** 2

#### 4.1 Create Cargo workspace and foundational crates

**Deliverables:**

- `rust/Cargo.toml` — workspace manifest
- `rust/crates/sera-domain/` — Core domain types: `AgentId`, `InstanceId`, `TaskId`, `SecurityTier`, `LifecycleMode`, `TaskInput`, `TaskOutput`, serde derives
- `rust/crates/sera-config/` — Config loading from env vars, `providers.json` parsing
- `rust/crates/sera-testing/` — Test utilities, fixtures
- `.github/workflows/rust-ci.yml` or equivalent CI step: `cargo check`, `cargo test`, `cargo clippy`
- `rust/.cargo/config.toml` — configure `lld` linker for faster builds on Windows

**Acceptance criteria:**

- [ ] `cargo check --workspace` passes
- [ ] `cargo test --workspace` passes (even if only trivial tests)
- [ ] `sera-domain` types serialize/deserialize to match the JSON produced by TypeScript agent-runtime (golden test against `schemas/byoh-contract.schema.json`)
- [ ] CI runs on PR

#### 4.2 Implement sera-domain manifest parser

**Deliverables:**

- `rust/crates/sera-domain/src/manifest.rs` — Parse `AGENT.yaml` (both flat and spec-wrapped formats)
- Golden test corpus: take 5 existing template YAMLs from `templates/` and verify Rust parses them identically to TypeScript
- `serde_yaml` for deserialization, `schemars` for schema generation comparison

**Acceptance criteria:**

- [ ] All 5 template golden tests pass
- [ ] Both flat and spec-wrapped manifest formats parse correctly
- [ ] Unknown fields are preserved (serde `flatten` with `HashMap<String, Value>`)

---

### Milestone 5: Minimal BYOH Rust Agent (Proof-of-Concept)

**Dependencies:** Milestone 4 (Rust workspace), Milestone 1 (BYOH contract)
**Estimated PRs:** 2

**Scope note:** This is a MINIMAL BYOH agent that validates the Rust toolchain and Docker image pipeline. It implements the BYOH contract using `spec.sandbox.image` (from M2.1) — no new `runtime: ts | rust` schema field. It does NOT attempt to replicate the TypeScript agent-runtime's full capabilities (context compaction, tool loop detection, tool argument repair, 10+ tool handlers, Centrifugo publishing, etc.). A full sera-runtime-rs is deferred to a later phase.

#### 5.1 Implement minimal Rust BYOH agent

**Deliverables:**

- `rust/crates/sera-byoh-agent/src/main.rs` — Entry point: read `TaskInput` from stdin, call LLM proxy, write `TaskOutput` to stdout
- `rust/crates/sera-byoh-agent/src/llm_client.rs` — OpenAI-compatible HTTP client using `reqwest`. Single request-response (no streaming required for PoC). Reads `SERA_LLM_PROXY_URL` and `SERA_IDENTITY_TOKEN` from env.
- `rust/crates/sera-byoh-agent/src/health.rs` — HTTP health server on `AGENT_CHAT_PORT` (default 3100) using `axum`. Returns `{"ready": true, "busy": false}`.
- `rust/crates/sera-byoh-agent/src/heartbeat.rs` — Background task that POSTs to heartbeat endpoint at `AGENT_HEARTBEAT_INTERVAL_MS` intervals.
- SIGTERM handler for graceful shutdown.
- No tool execution. No context compaction. No Centrifugo. No stream wrappers. No reasoning model handling.

**Acceptance criteria:**

- [ ] `cargo build --release -p sera-byoh-agent` produces a static binary under 10MB
- [ ] Binary reads `TaskInput` JSON from stdin, calls LLM proxy, returns `TaskOutput` JSON on stdout
- [ ] Health endpoint responds to `GET /health` with `{"ready": true, "busy": false}`
- [ ] Heartbeat runs in background for persistent mode
- [ ] SIGTERM triggers clean shutdown

#### 5.2 Create Rust BYOH Docker image and template

**Deliverables:**

- `rust/Dockerfile.byoh-agent` — Multi-stage build: `rust:1.78-slim` builder + `debian:bookworm-slim` runtime (or `scratch`/`distroless`)
- Image tagged as `sera-byoh-rust-agent:latest`
- Entrypoint: the `sera-byoh-agent` binary
- Template: `templates/byoh-rust-example.agent.yaml` using `spec.sandbox.image: sera-byoh-rust-agent:latest`

**Acceptance criteria:**

- [ ] Image builds successfully
- [ ] Image size under 30MB (vs ~200MB for bun-based image)
- [ ] Container starts and responds to `/health` within 200ms (vs ~2s for bun)
- [ ] Container can execute a simple task end-to-end against a running sera-core
- [ ] Template uses `spec.sandbox.image` — no `runtime: rust` field exists

---

### Milestone 6: Universal Skill Registry MVP

**Dependencies:** Milestone 1 (BYOH contract), Milestone 2.1 (custom image support)
**Estimated PRs:** 2

#### 6.1 Define skill manifest schema and registry integration

**Context:** The existing skill system has 4 components that must be understood:

- `SkillRegistry` (`core/src/skills/SkillRegistry.ts`) — In-memory registry of executable `SkillDefinition` objects (tools). Has `register()`, `get()`, `listAll()`, `invoke()`.
- `SkillLibrary` (`core/src/skills/SkillLibrary.ts`) — DB-backed guidance documents (markdown with frontmatter). Loaded from filesystem, stored in PostgreSQL. Not directly executable.
- `SkillRegistryService` (`core/src/skills/adapters/SkillRegistryService.ts`) — External registry orchestrator. Uses `SkillSourceAdapter` implementations (e.g. `ClawHubAdapter`) to search/import skills from external sources.
- `SkillSourceAdapter` (`core/src/skills/adapters/SkillSourceAdapter.ts`) — Interface for external skill registries (search + fetch).

The manifest-indexed skills from this milestone are **a new category**: discoverable metadata that lives on the filesystem (mounted into containers) and is indexed by a new API. They are NOT `SkillDefinition` objects (not directly executable by sera-core) and NOT `SkillDocument` objects (not markdown guidance). They are closest to `ExternalSkillEntry` but with richer schema.

**Deliverables:**

- `schemas/sera-skill.schema.json` — Discovery-only schema (no `entrypoint` or `runtime` fields in v1):
  ```json
  {
    "name": "string (unique identifier)",
    "displayName": "string",
    "description": "string",
    "version": "semver string",
    "parameters": { "JSON Schema for tool input" },
    "returns": { "JSON Schema for tool output" },
    "compatibleHarnesses": ["sera", "openclaw", "langchain", "custom"],
    "tags": ["string"],
    "author": "string",
    "license": "string"
  }
  ```
- New file: `core/src/skills/ManifestSkillIndex.ts` — Reads `sera-skill.json` files from a configured directory, builds an in-memory index, provides search. This is separate from `SkillRegistry` (which holds executable tools) and `SkillLibrary` (which holds DB-backed guidance).
- `GET /api/skills/registry` endpoint returning the combined catalog: `SkillRegistry.listAll()` (built-in executable tools) + `ManifestSkillIndex.listAll()` (manifest-indexed discoverable skills)
- `POST /api/skills/search` endpoint with text search over name/description/tags across both sources

**Acceptance criteria:**

- [ ] Schema validates against 3 sample skill manifests (one TypeScript-compatible, one Python-compatible, one shell-compatible)
- [ ] `ManifestSkillIndex` is a distinct class from `SkillRegistry` and `SkillLibrary`
- [ ] API returns combined catalog of built-in SERA tools + registered skill manifests
- [ ] Search endpoint filters by name, tag, and compatible harness
- [ ] No `entrypoint` or `runtime` fields in the v1 schema

#### 6.2 Capability-filtered skill volume mounting in containers

**Deliverables:**

- Modify `BindMountBuilder.buildMounts()` (`core/src/sandbox/BindMountBuilder.ts`) to add read-only bind mount for skills
- **Capability filtering:** `BindMountBuilder` reads the agent's `ResolvedCapabilities.skillPackages` (already present in the type at line ~255 of `types.ts`) and mounts ONLY skill directories the agent is authorized to use — not the entire skills directory. If `skillPackages` is empty/undefined, mount nothing (secure default).
- Add `SERA_SKILLS_DIR=/sera/skills` env var to SandboxManager's env array
- Write a `sera-skill-index.json` generator that produces a single index file at `/sera/skills/index.json` listing only the mounted (authorized) skills
- Agent-runtime: add `skill-search` tool that reads `/sera/skills/index.json` and returns matching skills

**Acceptance criteria:**

- [ ] Containers have `/sera/skills/` mounted read-only with only authorized skills
- [ ] An agent without `skillPackages` gets no skill mounts (secure default)
- [ ] An agent with `skillPackages: ["web-search", "code-analysis"]` gets only those skill directories
- [ ] `index.json` lists only mounted skills with their manifest metadata
- [ ] Agent-runtime's `skill-search` tool returns relevant skills for a query
- [ ] A non-SERA harness can read `/sera/skills/index.json` and list available skills

---

### Milestone 7: Contract Tests & Compatibility Verification

**Dependencies:** Milestones 5, 6
**Estimated PRs:** 2

#### 7.1 BYOH contract compliance test suite

**Deliverables:**

- `tests/byoh-compliance/` — A test harness that:
  - Spawns a container from a given image via sera-core API
  - Verifies `/health` responds within 10 seconds on the correct `AGENT_CHAT_PORT`
  - Sends a task via stdin or `/chat`
  - Verifies result appears on stdout or in the chat response
  - Verifies token usage appears in metering API
  - Verifies egress proxy logs show the LLM call
  - **Negative test (egress enforcement):** From inside a test container on `agent_net`, attempt direct `curl` to an external IP (bypassing proxy). Assert this fails (if enforcement is active per M1.5) or skip with documented reason.
  - Verifies heartbeat appears in `agent_instances.last_heartbeat_at` for persistent-mode agents
  - Verifies SIGTERM triggers clean shutdown (container exits within 30 seconds)
- Run against: `sera-agent-worker:latest` (TS), `sera-byoh-rust-agent:latest` (Rust PoC), `byoh-python` example, `byoh-shell` example

**Acceptance criteria:**

- [ ] All 4 images pass the compliance suite (excluding egress enforcement if documented as limitation)
- [ ] Test suite is runnable via `bun run test:byoh` (requires Docker)
- [ ] Failure output clearly indicates which contract requirement was violated
- [ ] Negative egress test either passes or is skip-documented

#### 7.2 Cross-runtime golden tests

**Deliverables:**

- Take 3 representative tasks (simple Q&A, context passthrough, error handling / budget exceeded)
- Record TypeScript runtime's stdout JSON for each
- Verify Rust BYOH agent produces structurally compatible output (same `TaskOutput` schema fields and types)
- Note: Rust agent does NOT have tool execution, context compaction, or multi-turn — so golden tests are limited to the BYOH contract surface (stdin → LLM proxy → stdout)

**Acceptance criteria:**

- [ ] All 3 golden tests pass for both runtimes
- [ ] `TaskOutput` schema is identical between TS and Rust (validated by JSON Schema)
- [ ] Usage fields (promptTokens, completionTokens, etc.) are present and numeric in both

---

## Dependency Graph

```
M1 (BYOH Contract + Env Vars)
  |
  +--> M1.5 (Egress Enforcement)
  |
  v
M2.0 (Image Allowlist) --> M2.1 (Custom Image + SandboxSpec) --> M3 (Example BYOH Containers)
                                |                                      |
                                v                                      v
                           M6 (Skill Registry MVP) -------------> M7 (Contract Tests)
                                                                    ^
M4 (Rust Workspace Bootstrap)                                       |
  |                                                                 |
  v                                                                 |
M5 (Minimal BYOH Rust Agent) -------------------------------------+
```

**Parallelism:**

- M1, M2.0, and M4 can all start in parallel (no dependencies between them).
- M1.5 can start after M1 is done.
- M2.1 requires M2.0.
- M3, M5, and M6 can proceed in parallel after their respective dependencies.
- M7 is the final integration milestone requiring M5 and M6.

---

## Risk Register

| Risk                                                                                              | Likelihood                                               | Impact | Mitigation                                                                                                                                                                                                                                                                                                                                                | Status                                               |
| ------------------------------------------------------------------------------------------------- | -------------------------------------------------------- | ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| **Egress proxy compliance is advisory** — containers can ignore `HTTP_PROXY` and connect directly | **Certain** on Docker Desktop (Win/Mac); Medium on Linux | High   | M1.5 investigates iptables enforcement. On Linux CE, iptables rules on `agent_net` can DROP non-proxy outbound. On Docker Desktop (Windows/macOS), this is a **known limitation** — the Docker VM abstracts networking and iptables rules on the host cannot reach container networks. Production deployments should use Linux with iptables enforcement. | Honest assessment — not "mitigated by documentation" |
| `spec.sandbox.image` allows arbitrary images with security implications                           | High                                                     | High   | M2.0 implements image allowlist in SandboxBoundary per tier. Tier-3 agents restricted to `sera-agent-worker:latest` only. Image validation runs before container creation.                                                                                                                                                                                | Blocked by M2.0                                      |
| Guest harness ignores heartbeat contract                                                          | Medium                                                   | Low    | HeartbeatService already marks non-responsive agents as `unresponsive`. Document heartbeat in BYOH contract. Compliance test verifies.                                                                                                                                                                                                                    | Acceptable                                           |
| Rust binary too large for container (debug symbols, static linking)                               | Low                                                      | Low    | Strip symbols, use `lto = true`, `codegen-units = 1` in release profile. Target <10MB for PoC agent.                                                                                                                                                                                                                                                      | Low risk                                             |
| Skill manifest format proliferation                                                               | Low                                                      | Medium | Keep `sera-skill.json` minimal and discovery-only for v1. Wrapper generators for OpenClaw/LangChain.                                                                                                                                                                                                                                                      | Acceptable                                           |
| Existing skill system complexity causes integration confusion                                     | Medium                                                   | Medium | M6.1 creates a new `ManifestSkillIndex` class distinct from the 4 existing skill components. API endpoint merges results. Clear documentation of which component owns what.                                                                                                                                                                               | Addressed in plan                                    |
| `SandboxSpec` introduction breaks existing YAML parsing                                           | Low                                                      | High   | Zod schema makes `sandbox` fully optional. `AgentManifestLoader` already handles spec-wrapped vs flat format. Golden test against all existing templates.                                                                                                                                                                                                 | Low risk                                             |

---

## Out of Scope (Deferred to v2+)

- **Full sera-runtime-rs** — Porting the complete TypeScript agent-runtime (11,319 LOC: reasoning loop, context compaction, tool loop detection, tool argument repair, Centrifugo publishing, 10+ tool handlers) to Rust. The v1 Rust agent is a minimal BYOH proof-of-concept only.
- **Centrifugo thought-streaming sidecar** for guest harnesses (env vars reserved in v1 contract)
- **Rust migration of sera-core** (Phase 2+ of migration plan)
- **Language-agnostic skill execution protocol** (JSON-RPC/stdin-stdout) — v1 is discovery-only
- **OpenClaw skill import pipeline** (beyond manifest wrapper)
- **Skill `entrypoint` and `runtime` fields** — execution semantics deferred to v2
- **Image generation / media processing epics**
- **sera-tui-rs migration**
- **Write-path migration to Rust** (remains TypeScript)
