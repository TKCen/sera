# SPEC: Migration Path

> **Status:** DRAFT  
> **Source:** PRD §17, §18  
> **Priority:** Ongoing (phases 0–4)  

---

## 1. Overview

SERA is migrating from a **TypeScript monolith** to a **modular Rust workspace**. This is a **clean break** — no backward compatibility with the TS core is required. However, the current system must remain operational during development until the Rust implementation reaches feature parity at the MVS (Minimal Viable SERA) checkpoint.

---

## 2. Migration Strategy

- **Clean break** from the TypeScript core
- **Current system runs alongside** the Rust implementation during development
- **Detailed transition planning** happens once Phase 2 (MVS) is complete
- **No data migration** from TS to Rust is assumed (clean start)

---

## 3. Minimal Viable SERA (MVS)

The MVS target — the point at which the Rust implementation can replace the TS core for basic use:

| Capability | Requirement |
|---|---|
| Agent | One agent with basic tools |
| Memory | File-based memory |
| Channel | Discord integration |
| Session | Session reset / lifecycle |
| Model | Local LM Studio with gemma-4 model |
| Auth | Basic (Tier 1 autonomous or simple JWT) |
| Tools | Memory read/write, shell, session management |

MVS is the **Phase 2 checkpoint** — once reached, the TS system can be decommissioned for basic use cases.

---

## 4. Phase Plan

### Phase 0 — Foundation (Weeks 1–3)

| Deliverable | Crate |
|---|---|
| Rust workspace setup | Root `Cargo.toml` |
| Shared types with Principal model | `sera-types` |
| Configuration loading and validation | `sera-config` |
| Unified error types | `sera-errors` |
| Database abstraction (PostgreSQL + SQLite) | `sera-db` |
| Lane-aware FIFO queue | `sera-queue` |
| OpenTelemetry integration | `sera-telemetry` |
| Secret provider (env + file) | `sera-secrets` |
| Protobuf contracts for all gRPC interfaces | `proto/` |

### Phase 1 — Core Domain (Weeks 4–7)

| Deliverable | Crate |
|---|---|
| Session state machine, transcript, compaction | `sera-session` |
| File-based memory backend with optional git | `sera-memory` |
| AuthN (JWT, API keys), basic RBAC, Principal registry | `sera-auth` |
| Tool registry, schema, built-in tools | `sera-tools` |
| WASM runtime, chainable pipelines, per-instance config | `sera-hooks` |
| Approval routing with configurable enforcement modes | `sera-hitl` |
| Skill pack loader | `sera-skills` |
| Cron scheduler, dreaming workflow | `sera-workflow` |
| Bundled documentation for agent consumption | Docs |

### Phase 2 — Runtime & Gateway (Weeks 8–11)

| Deliverable | Crate |
|---|---|
| Model adapters (OpenAI-compat, Anthropic, Gemini, Ollama) | `sera-models` |
| KV-cache-optimized context pipeline, turn loop | `sera-runtime` |
| HTTP/WS/gRPC server | `sera-gateway` |
| Full loop: gateway → queue → runtime → tool → memory → response | Integration |
| **MVS checkpoint: basic agent working with Discord + LM Studio** | Milestone |

### Phase 3 — Interop & Clients (Weeks 12–15)

| Deliverable | Crate |
|---|---|
| MCP server + client bridge | `sera-mcp` |
| A2A, ACP adapters | `sera-a2a`, `sera-acp` |
| AG-UI streaming (full + minimal thin client stream) | `sera-agui` |
| CLI and SDK | `sera-cli`, `sera-sdk` |
| Discord connector (gRPC adapter) | External |
| First WASM hook examples (with parameterized config) | Examples |
| `sera-web` rebuild (AG-UI compatible) | Frontend |

### Phase 4 — Enterprise & Hardening (Weeks 16+)

| Deliverable | Crate |
|---|---|
| OIDC, SCIM, AuthZen PDP, SSF/CAEP/RISC | `sera-auth` enterprise |
| Vault, AWS SM, Azure KV, GCP SM providers | `sera-secrets` enterprise |
| External agent identity registration | `sera-auth` |
| Multi-node support (queue backend decision) | Infrastructure |
| Circle DAG orchestration | `sera-circles` |
| LCM memory backend | `sera-memory` |
| Dynamic risk-based approval routing | `sera-hitl` |
| TUI | `sera-tui` |
| Comprehensive E2E tests & benchmarks | Testing |
| Documentation and operator guides | Docs |

---

## 5. Resolved Decisions

Decisions already made (from PRD §18) that affect migration scope:

| Decision | Rationale |
|---|---|
| Queue backend for Tier 3 deferred to Phase 4 | In-memory/SQLite (T1) and Postgres (T2) cover early phases |
| Model adapters: trait + gRPC | In-process for standard providers; gRPC for exotic setups |
| File-based memory default | Simple, inspectable, version-controllable |
| Web frontend: clean rebuild | No backward compat constraint |
| Clean break from TS core | Keep current system running alongside during dev |
| Workspace-primary filesystem | File-based is tangible, inspectable, git-compatible |
| Standard WASM toolchains for hooks | Don't reinvent the wheel |
| Both WebSocket and gRPC streaming | WS for web; gRPC for inter-service |
| Pluggable secret provider | Secrets never in config files |
| Principal-centric identity | Agents need first-class identity |
| Configurable HITL enforcement | Private sandboxes can run fully autonomous |
| KV-cache-optimized context | Maximizes prefix cache hits |
| General workflow engine | Dreaming is built-in, not special-cased |

---

## 6. Risk Assessment

| Risk | Mitigation |
|---|---|
| Phase estimates are ambitious | Each phase has a clear checkpoint; scope can be adjusted |
| WASM hook DX may be complex | Start with Rust-only hooks; Python/TS SDKs can follow |
| Multi-node architecture (Phase 4) is underspecified | Trait boundaries defined early; backends pluggable |
| Current system may bit-rot during migration | MVS target prioritizes feature parity for basic use cases |

---

## 7. Cross-References

| Spec | Phase |
|---|---|
| [SPEC-crate-decomposition](SPEC-crate-decomposition.md) | Phase 0 (workspace setup) |
| [SPEC-gateway](SPEC-gateway.md) | Phase 2 |
| [SPEC-runtime](SPEC-runtime.md) | Phase 2 |
| [SPEC-tools](SPEC-tools.md) | Phase 1 |
| [SPEC-hooks](SPEC-hooks.md) | Phase 1 |
| [SPEC-memory](SPEC-memory.md) | Phase 1 |
| [SPEC-workflow-engine](SPEC-workflow-engine.md) | Phase 1 |
| [SPEC-interop](SPEC-interop.md) | Phase 3 |
| [SPEC-identity-authz](SPEC-identity-authz.md) | Phase 1 (basic), Phase 4 (enterprise) |
| [SPEC-hitl-approval](SPEC-hitl-approval.md) | Phase 1 |
| [SPEC-config](SPEC-config.md) | Phase 0 |
| [SPEC-secrets](SPEC-secrets.md) | Phase 0 (basic), Phase 4 (enterprise) |
| [SPEC-circles](SPEC-circles.md) | Phase 4 |
| [SPEC-observability](SPEC-observability.md) | Phase 0 |
| [SPEC-clients](SPEC-clients.md) | Phase 3 |
| [SPEC-thin-clients](SPEC-thin-clients.md) | Phase 3+ |
| [SPEC-deployment](SPEC-deployment.md) | Phase 0 (Tier 1) |
| [SPEC-security](SPEC-security.md) | Phase 0 (foundational) |

---

## 8. Success Criteria

| Metric | Target |
|---|---|
| Single-node throughput | ≥ 100 concurrent sessions, < 50ms gateway routing |
| Local startup time | < 2 seconds (Tier 1, single binary) |
| Hook chain overhead | < 5ms per WASM hook invocation |
| gRPC adapter latency | < 10ms roundtrip for local connectors |
| Bootstrap time | < 5 minutes from `sera init` to first agent conversation |
| Extension authoring | < 1 hour for a WASM hook; < 4 hours for a gRPC connector |
| HITL approval roundtrip | < 500ms from trigger to notification delivery |
| KV cache hit rate | ≥ 80% prefix reuse across turns in same session |

---

## 9. MVS Implementation Status

The Rust workspace at `rust/` has reached the **Minimal Viable SERA (MVS) checkpoint** with all deliverables complete and tested.

### Workspace Structure

The implementation comprises **11 crates**, all compiling and tested:

- `sera-domain` — Core types (Principal, Event, Session, ConfigManifest, Tool, Memory)
- `sera-config` — Configuration loading with K8s YAML support and secret resolution
- `sera-db` — SQLite backend for sessions, transcript, queue, and audit logs
- `sera-runtime` — Turn loop, context pipeline, and tool execution
- `sera-core` — HTTP/gRPC gateway and main CLI binary (`sera`)
- `sera-tools` — 7 MVS tools with sandboxed path safety
- Plus 6 supporting crates: `sera-telemetry`, `sera-auth`, `sera-memory`, `sera-mcp`, `sera-discord`, `sera-observe`

### MVS Crate Mapping

The original 8-crate plan maps directly to the built workspace:

| Plan | Implementation | Scope |
|---|---|---|
| `sera-types` | `sera-domain` | Principal, Event, Session, ConfigManifest, Tool, Memory models |
| `sera-config` | `sera-config::manifest_loader` | K8s YAML loading, secret resolution, validation |
| `sera-errors` | Distributed via `thiserror` | Error types available across all crates |
| `sera-db` | `sera-db::sqlite` | Session storage, transcript compaction, queue, audit trail |
| `sera-memory` | `sera-domain::memory` | File-based markdown with keyword search |
| `sera-tools` | `sera-runtime::tools::mvs_tools` | 7 tools (memory r/w, shell, session mgmt, file ops, http, context) |
| `sera-models` | `sera-runtime::llm_client` | OpenAI-compatible SSE streaming |
| `sera-gateway` | `sera-core` (bin) | HTTP/gRPC server, Discord module, agent lifecycle |

### MVP Capabilities

- **Binary**: `sera` CLI supports `init`, `agent create`, `agent list`, `start` commands
- **Agent execution**: Full loop from gateway → queue → runtime → tool → memory → response
- **Context management**: KV-cache-optimized context pipeline with configurable token limits
- **Persistence**: SQLite for sessions, transcript, and audit logs
- **Discord integration**: gRPC-based connector with full message round-trip
- **Tool system**: 7 sandboxed tools with path safety and resource limits
- **Test coverage**: 645+ tests passing across all crates
- **Release binary**: 6.9MB statically linked single binary

### Development Reference

- **Workspace commands**: See `rust/CLAUDE.md` for build, test, and debug workflows
- **Architecture knowledge**: See `docs/wiki/` for detailed design rationale and implementation notes
- **Configuration**: K8s-style YAML manifests with env-var secret resolution

### Next Phase Targets (Phase 3+)

Once Phase 3 begins:
- MCP server and A2A/ACP adapters
- CLI and SDK clients
- Multi-agent orchestration (circles)
- Enterprise auth (OIDC, Vault providers)

---

## 10. Open Questions

1. **TS → Rust data migration** — Is there any data (memory, sessions, config) to migrate from the TS system? Or is it a clean start?
2. **Parallel operation period** — How long will the TS and Rust systems run side-by-side? Is there a cutover date?
3. **Discord connector migration** — Does the current Discord integration need to be ported exactly, or can it be redesigned?
4. **Week estimates** — Are the week ranges realistic given team size and availability?
