# Epic 19: Memory System Consolidation

## Overview

SERA currently has two coexisting memory systems that were never designed to live together permanently. Epic 8 introduced the canonical scoped memory model (personal/circle/global, `ScopedMemoryBlockStore`, Qdrant with named namespaces). But a Letta-inspired system from an earlier iteration (human/persona/core/archive blocks, `MemoryBlockStore`, `MemoryManager`) still exists and is actively used by the in-process reasoning path.

This epic retires the old system cleanly, migrates the in-process agent path to the Epic 8 model, and removes the resulting dead code.

## Context

### Why two systems exist

The Letta-style system was implemented early (pre-Epic 8) as a placeholder memory model. When Epic 8 introduced the canonical three-scope architecture, the old system was left in place because:

1. It was wired into `BaseAgent` and `WorkerAgent` via `MemoryManager.assembleContext()` — the in-process reasoning path
2. The new `ContextAssembler` RAG was wired only into the LLM proxy route (`/v1/llm/...`) — used by containerised agents calling back through sera-core
3. The memory graph UI (Epic 13) was built against the old store's graph/refs data model

The result is that the two execution paths read from different memory stores:

| Path | Memory read by |
|---|---|
| `BaseAgent` / `WorkerAgent` (in-process) | Old `MemoryManager.assembleContext()` — Letta blocks |
| Containerised agent → LLM proxy | New `ContextAssembler` RAG — scoped Qdrant namespaces |

### What the old system owns today

| Component | File | Status after this epic |
|---|---|---|
| `MemoryBlockStore` | `memory/blocks/MemoryBlockStore.ts` | Delete |
| `MemoryManager` | `memory/manager.ts` | Delete |
| `Reflector` | `memory/Reflector.ts` | Delete (see Story 19.3) |
| Letta block types | `memory/blocks/types.ts` | Delete |
| Old `MemoryBlockStore.test.ts` | `memory/blocks/MemoryBlockStore.test.ts` | Delete |
| Old `Reflector.test.ts` | `memory/Reflector.test.ts` | Delete |
| Legacy `/api/memory/blocks/*` routes | `routes/memory.ts` | Remove legacy section |
| Legacy `/api/memory/graph` route | `routes/memory.ts` | Remove or migrate to scoped store |
| `_memoryManager` param in `registerBuiltinSkills` | `skills/builtins/index.ts` | Remove parameter |

### What must be preserved

- `ScopedMemoryBlockStore` and all of `memory/blocks/scoped-types.ts` — these are the canonical store
- `KnowledgeGitService` — circle/global git-backed knowledge
- `MemoryCompactionService` — archival job
- `ContextAssembler` RAG — the new retrieval path
- All existing personal memory files on disk — must be migrated or left readable

## Dependencies

- Epic 08 (Memory & RAG) — must be complete (provides the target system)
- Epic 05 (Agent Runtime) — `BaseAgent`/`WorkerAgent` are being modified
- Epic 13 (sera-web Agent UX) — memory graph UI needs updating for the new data model

---

## Stories

### Story 19.1: Migrate BaseAgent and WorkerAgent to scoped memory

**As** sera-core
**I want** the in-process agent reasoning path to read from `ScopedMemoryBlockStore` via `ContextAssembler`
**So that** all agents — containerised and in-process — use the same memory model

**Acceptance Criteria:**
- [ ] `BaseAgent` no longer holds a `MemoryManager` reference; it injects a `ContextAssembler` instead (or delegates context assembly to the same `ContextAssembler` instance used by the LLM proxy)
- [ ] `WorkerAgent.process()` calls `contextAssembler.assemble(agentId, messages)` instead of `memoryManager.assembleContext(input)`
- [ ] `AgentFactory` no longer instantiates `MemoryManager`; passes `ContextAssembler` instead
- [ ] Context assembly result is identical for in-process and containerised paths — both inject `<memory>` XML blocks from scoped Qdrant namespaces
- [ ] Existing in-process agent tests updated; no regressions

---

### Story 19.2: Remove MemoryManager and MemoryBlockStore

**As** a developer
**I want** the old Letta-style memory classes deleted
**So that** there is one memory model and no confusion about which store an agent reads

**Acceptance Criteria:**
- [ ] `MemoryManager`, `MemoryBlockStore`, and Letta block types (`human`/`persona`/`core`/`archive`) are deleted
- [ ] `memory/blocks/types.ts` deleted; all imports updated to `memory/blocks/scoped-types.ts`
- [ ] `memory/manager.ts` deleted
- [ ] `memory/blocks/MemoryBlockStore.ts` deleted
- [ ] `MemoryBlockStore.test.ts` deleted
- [ ] `registerBuiltinSkills` signature updated to remove the `_memoryManager` parameter
- [ ] No TypeScript errors (`tsc --noEmit` passes)

---

### Story 19.3: Migrate Reflector to MemoryCompactionService

**As** sera-core
**I want** the Letta-style Reflector compaction replaced by the Epic 8 `MemoryCompactionService`
**So that** compaction uses the canonical block format and importance-based archival

**Background:** `Reflector` summarises old `core`-type entries via LLM and moves originals to `archive`. `MemoryCompactionService` archives blocks older than N days with importance ≤ 2. These serve the same goal (keep active memory focused) but with different strategies.

**Acceptance Criteria:**
- [ ] `Reflector.ts` and `Reflector.test.ts` deleted
- [ ] `MemoryCompactionService` extended to support an optional LLM-summarisation pass: when `MEMORY_COMPACT_WITH_LLM=true`, archived blocks are summarised into a new `memory`-type block before removal from active index — preserving the Reflector's "distillation" behaviour
- [ ] Compaction job registered in `startServer()` remains as-is (daily pg-boss schedule)
- [ ] `POST /api/memory/:agentId/compact` continues to work as the manual trigger

---

### Story 19.4: Update memory routes and UI data model

**As** an operator
**I want** the `/api/memory/*` routes to serve only scoped (Epic 8) data
**So that** the UI memory graph reflects the real memory model

**Acceptance Criteria:**
- [ ] Legacy `/api/memory/blocks`, `/api/memory/blocks/:type`, `/api/memory/entries/:id`, `/api/memory/graph` routes removed from `routes/memory.ts`
- [ ] `/api/memory/:agentId/blocks` (already implemented in Epic 8) is the canonical list endpoint
- [ ] A replacement graph endpoint `GET /api/memory/:agentId/graph` is added — returns nodes and edges derived from `ScopedMemoryBlockStore` using `tags` as implicit links (blocks sharing tags are connected) and explicit `refs`-style links via a `relatedIds` optional frontmatter field
- [ ] Epic 13 memory graph UI updated to consume the new endpoint shape (coordinate with Epic 13 implementation)
- [ ] `GET /api/memory/:agentId/stats` (already implemented) unchanged

---

## Definition of done

- [ ] All four stories implemented (19.1–19.4)
- [ ] `tsc --noEmit` passes with zero errors
- [ ] No references to `MemoryManager`, `MemoryBlockStore`, `Reflector`, or Letta block types remain outside of `legacy-archive` paths and migration code
- [ ] All existing agent tests pass
- [ ] Memory graph UI (Epic 13) verified against the new graph endpoint
