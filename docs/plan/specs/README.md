# SERA Specification Index

> **Derived from:** [plan.md](../plan.md) (PRD v0.3)  
> **Date:** 2026-04-09  
> **Last Updated:** 2026-04-09 (Research synthesis applied: OpenSwarm v2/v3 + Anthropic + OpenClaw + Agent Stack)  

---

## Core Domain Specs

| # | Spec | Scope | Phase | Status |
|---|---|---|---|---|
| 1 | [SPEC-gateway](SPEC-gateway.md) | Gateway, event model, dedupe/debounce, session state machine, lane-aware queue with modes, session scoping, connectors | 2 | DRAFT |
| 2 | [SPEC-runtime](SPEC-runtime.md) | Agent runtime, turn loop, KV-cache context pipeline, structured generation, persona architecture, multi-model routing, dynamic parameters, harness patterns, skills system | 2 | DRAFT |
| 3 | [SPEC-tools](SPEC-tools.md) | Tool trait, registry, progressive disclosure, profiles, execution, credential injection, sandbox lifecycle | 1 | DRAFT |
| 4 | [SPEC-hooks](SPEC-hooks.md) | WASM hook system, chains, hook points, parameterization, HookChain manifests | 1 | DRAFT |
| 5 | [SPEC-memory](SPEC-memory.md) | Memory trait, file-based backend, git, LCM, embedding-based search, hybrid search, recall signal tracking, compaction | 1 | DRAFT |
| 6 | [SPEC-workflow-engine](SPEC-workflow-engine.md) | Triggered workflows, cron, dreaming, workflow sessions, Beads task graph | 1 | DRAFT |
| 7 | [SPEC-interop](SPEC-interop.md) | MCP, A2A, ACP, AG-UI protocols | 3 | DRAFT |

## Identity, Security & Authorization

| # | Spec | Scope | Phase | Status |
|---|---|---|---|---|
| 8 | [SPEC-identity-authz](SPEC-identity-authz.md) | Principal model, AuthN, AuthZ, RBAC, SSF | 1/4 | DRAFT |
| 9 | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Approval system, escalation chains, agent review, speculative execution | 1 | DRAFT |
| 10 | [SPEC-secrets](SPEC-secrets.md) | Secret management, providers, credential injection, side-routed entry | 0/4 | DRAFT |
| 11 | [SPEC-security](SPEC-security.md) | Trust boundaries, attack surface, data protection, PII tokenization | 0+ | DRAFT |

## Configuration & Operations

| # | Spec | Scope | Phase | Status |
|---|---|---|---|---|
| 12 | [SPEC-config](SPEC-config.md) | Composable K8s-style manifests (apiVersion/kind/metadata/spec), directory discovery, schema registry, layering, hot-reload | 0 | DRAFT |
| 13 | [SPEC-observability](SPEC-observability.md) | OpenTelemetry, audit log, run evidence / proof bundles, cost attribution, eval hooks | 0 | DRAFT |
| 14 | [SPEC-deployment](SPEC-deployment.md) | Deployment tiers (local → enterprise) | 0/2/4 | DRAFT |
| 15 | [SPEC-versioning](SPEC-versioning.md) | Interface versioning: config schemas, gRPC contracts, Rust traits, Hook WIT, interop protocols, CLI/API | 0 | DRAFT |

## Client Interfaces

| # | Spec | Scope | Phase | Status |
|---|---|---|---|---|
| 15 | [SPEC-clients](SPEC-clients.md) | CLI, TUI, Web SPA, SDK | 3 | DRAFT |
| 16 | [SPEC-thin-clients](SPEC-thin-clients.md) | HMIs, embedded displays, minimal AG-UI stream | 3+ | DRAFT |

## Architecture & Project

| # | Spec | Scope | Phase | Status |
|---|---|---|---|---|
| 17 | [SPEC-circles](SPEC-circles.md) | Multi-agent DAG coordination, inter-agent channels | 4 | DRAFT |
| 18 | [SPEC-crate-decomposition](SPEC-crate-decomposition.md) | Rust workspace structure, crate catalog, proto layout | 0 | DRAFT |
| 19 | [SPEC-migration](SPEC-migration.md) | Phase plan, MVS target, resolved decisions | 0–4 | DRAFT |

---

## Enhancements Applied

### OpenSwarm v2.0/v3.0 Enhancements

| Enhancement | Category | Spec | Section |
|---|---|---|---|
| Structured / Constrained Generation | Must-Add | [SPEC-runtime](SPEC-runtime.md) | §5.1 |
| Sandbox Lifecycle Management (SandboxProvider trait) | Must-Add | [SPEC-tools](SPEC-tools.md) | §6a |
| Embedding-Based Memory Search | Must-Add | [SPEC-memory](SPEC-memory.md) | §2a |
| Persona Architecture (immutable + mutable + introspection) | Should-Add | [SPEC-runtime](SPEC-runtime.md) | §4.3 |
| Multi-Model Task Routing | Should-Add | [SPEC-runtime](SPEC-runtime.md) | §5.2 |
| Dynamic Model Parameters & Sampler Profiles | Should-Add | [SPEC-runtime](SPEC-runtime.md) | §5.3 |
| Inter-Agent Communication Channels | Can-Defer | [SPEC-circles](SPEC-circles.md) | §5a |
| Speculative Execution During HITL Wait | Can-Defer | [SPEC-hitl-approval](SPEC-hitl-approval.md) | §7a |

### Composable Config & Versioning  

| Enhancement | Category | Spec | Section |
|---|---|---|
| Composable K8s-style config manifests | Must-Add | [SPEC-config](SPEC-config.md) | §2 |
| Config schema versioning (apiVersion) | Must-Add | [SPEC-config](SPEC-config.md) | §2.1 |
| Config version history + rollback | Should-Add | [SPEC-config](SPEC-config.md) | §8 |
| Interface versioning (cross-cutting) | Must-Add | [SPEC-versioning](SPEC-versioning.md) | All |
| gRPC contract versioning (proto packages) | Must-Add | [SPEC-versioning](SPEC-versioning.md) | §4 |
| Rust trait evolution strategy | Must-Add | [SPEC-versioning](SPEC-versioning.md) | §5 |
| Hook WIT versioning | Must-Add | [SPEC-versioning](SPEC-versioning.md) | §6 |
| Capability manifest | Should-Add | [SPEC-versioning](SPEC-versioning.md) | §9 |
| Deprecation policy | Should-Add | [SPEC-versioning](SPEC-versioning.md) | §10 |

### Research Synthesis Enhancements (Anthropic + OpenClaw + Agent Stack)

| Enhancement | Category | Spec | Section |
|---|---|---|---|
| Recall Signal Tracking (Dreaming Support) | Must-Add | [SPEC-memory](SPEC-memory.md) | §2b |
| Inbound Debounce / Dedupe Configuration | Must-Add | [SPEC-gateway](SPEC-gateway.md) | §4.1, §4.2 |
| Queue Mode Definitions (collect/followup/steer/interrupt) | Must-Add | [SPEC-gateway](SPEC-gateway.md) | §5.2 |
| Session Key Scoping Strategies | Must-Add | [SPEC-gateway](SPEC-gateway.md) | §6.3 |
| Workflow Session Isolation | Must-Add | [SPEC-workflow-engine](SPEC-workflow-engine.md) | §4 |
| Progressive Tool Disclosure (search_tools) | Should-Add | [SPEC-tools](SPEC-tools.md) | §4.1 |
| Harness Patterns (cost bounds, heartbeats, checkpoints) | Should-Add | [SPEC-runtime](SPEC-runtime.md) | §5.5 |
| Skills Formal Definition | Should-Add | [SPEC-runtime](SPEC-runtime.md) | §13 |
| Idempotency & Retry Boundaries | Should-Add | [SPEC-runtime](SPEC-runtime.md) | §6.1 |
| Run Evidence / Proof Bundle | Should-Add | [SPEC-observability](SPEC-observability.md) | §3.1 |
| Cost Attribution (per-agent, per-session, per-model) | Should-Add | [SPEC-observability](SPEC-observability.md) | §3.1 |
| PII Tokenization Pattern | Should-Add | [SPEC-security](SPEC-security.md) | §4.6 |
| Beads Task Graph Reclassification | Should-Add | [SPEC-workflow-engine](SPEC-workflow-engine.md) | §6.1 |
| Hybrid Search (vector + keyword) | Should-Add | [SPEC-memory](SPEC-memory.md) | §2a |
| Evaluation Framework Hooks | Can-Defer | [SPEC-observability](SPEC-observability.md) | §3.2 |

---

## Resolved Research Items

| Item | Spec | Resolution |
|---|---|---|
| ~~Memory search implementation~~ | [SPEC-memory](SPEC-memory.md) | §2a: Embedding-based semantic search with fallback to keyword |
| ~~Sandbox execution mechanism~~ | [SPEC-tools](SPEC-tools.md) | §6a: Pluggable SandboxProvider trait (Docker, WASM, MicroVM, External) |
| ~~Queue modes~~ | [SPEC-gateway](SPEC-gateway.md) | §5.2: Full definitions — collect, followup, steer, steer-backlog, interrupt |
| ~~Session key scoping~~ | [SPEC-gateway](SPEC-gateway.md) | §6.3: 5 strategies (main, per-channel, per-channel-peer, per-account-channel-peer, per-thread) |
| ~~Workflow session isolation~~ | [SPEC-workflow-engine](SPEC-workflow-engine.md) | §4: Dedicated `workflow:{agent_id}:{name}` sessions |
| ~~Beads task graph integration~~ | [SPEC-workflow-engine](SPEC-workflow-engine.md) | §6.1: Reclassified as workflow/tool integration, not memory backend |
| ~~Skill system definition~~ | [SPEC-runtime](SPEC-runtime.md) | §13: Formal SkillDef with storage, lifecycle, and configuration |
| ~~Cost attribution~~ | [SPEC-observability](SPEC-observability.md) | §3.1: Per-agent, per-session, per-principal, per-model attribution |
| ~~Protobuf versioning~~ | [SPEC-crate-decomposition](SPEC-crate-decomposition.md) | §5: Package-based versioning via [SPEC-versioning](SPEC-versioning.md) §4 |
| ~~Hook versioning~~ | [SPEC-hooks](SPEC-hooks.md) | Resolved: WIT package versioning via [SPEC-versioning](SPEC-versioning.md) §6 |
| ~~Protocol versioning~~ | [SPEC-interop](SPEC-interop.md) | Resolved: Protocol compatibility registry via [SPEC-versioning](SPEC-versioning.md) §7 |
| ~~Config file format~~ | [SPEC-config](SPEC-config.md) | Resolved: Composable YAML manifests with `---` separators |
| ~~Multi-file config~~ | [SPEC-config](SPEC-config.md) | Resolved: Directory-based recursive discovery (§2.3) |
| ~~Config versioning~~ | [SPEC-config](SPEC-config.md) | Resolved: Version history with rollback (§8) |

---

## Items Requiring Further Research

| Item | Spec | Section |
|---|---|---|
| Session state machine extensibility config format | [SPEC-gateway](SPEC-gateway.md) | §6 |
| Tool profiles — concrete profile definitions | [SPEC-tools](SPEC-tools.md) | §5 |
| `sera-web` UI framework choice | [SPEC-clients](SPEC-clients.md) | §5.2 |
| AG-UI thin client minimum event set | [SPEC-thin-clients](SPEC-thin-clients.md) | §3 |
| Task classification for model routing | [SPEC-runtime](SPEC-runtime.md) | §5.2 |
| Structured output provider support matrix | [SPEC-runtime](SPEC-runtime.md) | §5.1 |
| Embedding model deployment strategy | [SPEC-memory](SPEC-memory.md) | §2a |
| Sandbox image management | [SPEC-tools](SPEC-tools.md) | §6a |
| Progressive disclosure cache invalidation | [SPEC-tools](SPEC-tools.md) | §4.1 |
| Evidence retention policy | [SPEC-observability](SPEC-observability.md) | §3.1 |
| Git conflict resolution for multi-agent workspaces | [SPEC-memory](SPEC-memory.md) | §5.3 |
| Webhook authentication method | [SPEC-gateway](SPEC-gateway.md) | §9 |
| Config migration across SERA upgrades | [SPEC-config](SPEC-config.md) | §10 |
| Auto-migration CLI tool | [SPEC-versioning](SPEC-versioning.md) | §12 |
| Proto backward compatibility CI | [SPEC-versioning](SPEC-versioning.md) | §12 |

---

## Research Sources Integrated

| Source | Category | Specs Impacted |
|---|---|---|
| OpenSwarm v2.0/v3.0 | Cognitive architecture | runtime, tools, memory, circles, hitl-approval |
| Anthropic: Managed Agents | Agent subprocess model | runtime, tools |
| Anthropic: Harness Design | Long-running reliability | runtime |
| Anthropic: Code Execution with MCP | Progressive disclosure, PII | tools, security |
| OpenClaw 1–6 | Control plane, concurrency, memory, security, tools, reliability | gateway, memory, security, observability |
| OpenClaw Dreaming | Memory consolidation | memory, workflow-engine |
| Agent Stack Part 1 | 10-layer systems map | observability, gateway |
| Karpathy LLM-Wiki | Compilation over retrieval | memory, runtime (skills) |
| Beads | Deterministic task DAG | workflow-engine |
