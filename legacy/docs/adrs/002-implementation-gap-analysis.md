# ADR-002: Implementation Gap Analysis vs Canonical Epics

**Status:** Accepted
**Date:** 2026-03-30

## Summary

Systematic comparison of the 24 epics against actual implementation. Phase 1 (MVP) is substantially complete. Phase 2 (Usable) is 80% done. Phase 3 (Ecosystem) is ~60%. Phase 4 (Consolidation) is not started.

## Phase Completion Overview

| Phase | Epics | Status |
|-------|-------|--------|
| Phase 1: MVP | 01-04 | 98% complete |
| Phase 2: Usable | 05-14, 20 | 80% complete |
| Phase 3: Ecosystem | 15-18 | 60% complete |
| Phase 4: Consolidation | 19, 21-24 | 0% — deferred |

## Critical Gaps (blocking agent usefulness)

### 1. Agent-runtime tool catalog not dynamic (Epic 7, Story 7.6)

**Spec:** `GET /v1/llm/tools` returns filtered tool list per agent.
**Actual:** Hardcoded 7-tool array in agent-runtime. Agents cannot use knowledge-store, web-search, web-fetch, knowledge-query, schedule-task.
**Impact:** Agents are severely limited — can only read/write files and run shell commands.
**ADR:** [ADR-001](001-tool-execution-architecture.md)
**Issue:** #462, #482

### 2. Remote tool invocation not generalized (Epic 3, Story 3.10)

**Spec:** `POST /v1/tools/proxy` handles all delegated tool execution.
**Actual:** Proxy only handles filesystem grants. No generalized skill invocation.
**Impact:** API-backed tools unreachable from agent containers.
**ADR:** [ADR-001](001-tool-execution-architecture.md)

### 3. Scope validation incomplete (Epic 17, Stories 17.5-17.9)

**Spec:** Full scope validation — agents can only access resources in their circle, delegation chains tracked.
**Actual:** Delegation tokens exist but scope checks not enforced. An agent with delegation can access any resource, not just the scoped ones.
**Impact:** Security model is permissive — trust is implicit rather than validated.
**ADR:** [ADR-003](003-scope-validation-framework.md)

### 4. Permission grants are session-only (Epic 3, Story 3.10)

**Spec:** Persistent grants stored in PostgreSQL, survive container restarts.
**Actual:** `PermissionRequestService` stores grants in-memory. Lost on restart.
**Impact:** Dynamic filesystem access granted by operators must be re-approved after every restart.
**ADR:** [ADR-004](004-permission-grant-persistence.md)

## Non-Critical Gaps (quality/completeness)

| Gap | Epic | Phase | Priority |
|-----|------|-------|----------|
| Recursion depth guard for subagents | 3.11 | 2 | Low |
| Disk quota enforcement on containers | 3.12 | 2 | Low |
| Task result storage in PostgreSQL | 5.8 | 2 | Medium |
| Prompt injection defense (comprehensive) | 5.10 | 2 | Medium |
| sera-core MCP scope checks | 7.7 | 3 | Medium |
| SERA MCP Extension Protocol v1 | 7.8 | 3 | Medium |
| Circle constitutions (governance rules) | 10.7 | 3 | Low |
| Alert rule engine UI | 18.8 | 3 | Medium |
| Channel topology visualization | 18.9 | 3 | Low |
| Plugin SDK (@sera/mcp-sdk) | 15 | 3 | Medium |
| Audit bulk export | 11.6 | 2 | Low |

## Deferred Work (Phase 4)

These are intentionally deferred and not considered gaps:
- Epic 19: Legacy Letta memory migration
- Epic 21: ACP / IDE Bridge
- Epic 22: Canvas / A2UI
- Epic 23: Voice Interface
- Epic 24: A2A Federation Protocol

## Implementation Strengths

The following areas exceed or match the canonical design:
- **Docker sandbox isolation** — tier enforcement, network isolation, egress ACLs
- **LLM proxy governance** — metering, circuit breakers, budget enforcement
- **Audit trail** — Merkle hash-chain with tamper detection
- **MCP tool bridge** — dynamic server registration, container isolation
- **Channel adapters** — Discord, Slack, Telegram, WhatsApp, Email, Webhook
- **Memory & RAG** — scoped blocks, vector search, git-backed circles
- **Real-time streaming** — Centrifugo thought/token streaming
- **Authentication** — OIDC + API key + JWT with role-based access
