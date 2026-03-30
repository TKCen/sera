# ADR-006: Web Frontend and Agent-Runtime Modularity

**Status:** Proposed
**Date:** 2026-03-30

## Web Frontend (`web/src/`)

### Findings

**Strengths:**
- **Clean dependency direction**: pages/app → components → ui. No reverse imports (components never import from pages). This is correct React architecture.
- **TanStack Query over global state**: Only 27 context-hook call sites. Most data flows through TanStack Query, which is cacheable and testable.
- **Domain-scoped hooks**: 21 hook files, most under 100 lines. `useAgents` (228 lines, 21 exports) is the largest but follows standard TanStack Query patterns.

**Issues:**

#### 1. Dual routing structure (pages/ + app/)

13 files in `pages/` and 8 route files in `app/`. This is an incomplete migration from the old `pages/` directory to the Next.js-inspired `app/` directory. Some routes live in one place, some in the other.

**Impact:** Developers don't know where to put new pages. Import paths are inconsistent.

**Recommendation:** Complete the migration — move all pages to `app/` or consolidate back to `pages/`. Pick one convention and enforce it.

#### 2. Seven page files exceed 500 lines

| File | Lines |
|------|-------|
| SettingsPage | 871 |
| ChatPage | 620 |
| ToolsPage | 558 |
| AgentForm | 554 |
| SchedulesPage | 544 |
| ChannelsPage | 535 |
| ProvidersPage | 525 |

**Impact:** Hard to navigate, test, and maintain. State management, API calls, and rendering mixed in single files.

**Recommendation:** Extract sub-components (tab panels, form sections, list/detail views). The Memory Explorer page (MemoryExplorerPage.tsx at 170 lines with 6 child components) demonstrates the target pattern.

#### 3. Components bypass hooks and call API directly

28 files import directly from `lib/api/`. The hooks layer provides TanStack Query abstraction, but many components bypass it.

**Impact:** Inconsistent caching behavior, duplicate request logic, harder to test.

**Recommendation:** All data fetching should go through hooks. Components should never import from `lib/api/` directly — only hooks should.

#### 4. Type centralization bottleneck

`lib/api/types.ts` is imported by 15 files. As it grows, any change triggers wide recompilation.

**Recommendation:** Split into domain-specific type files: `types/agents.ts`, `types/memory.ts`, `types/providers.ts`.

### Web Summary

| Aspect | Status | Notes |
|--------|--------|-------|
| Dependency direction | 🟢 Clean | pages → components → ui |
| State management | 🟢 Good | TanStack Query, minimal context |
| Hook organization | 🟢 Good | Domain-scoped, well-sized |
| Page sizes | 🔴 Problem | 7 files > 500 lines |
| Routing structure | 🟡 Inconsistent | Dual pages/ + app/ |
| API coupling | 🟡 Moderate | 28 files bypass hooks layer |

---

## Agent-Runtime (`core/agent-runtime/src/`)

### Findings

**Strengths:**
- **Small, focused codebase**: 12 files, ~2,500 lines total. Each file has a clear purpose.
- **Clean dependency graph**: No circular dependencies. DAG flows: index → {loop, chatServer, heartbeat} → {tools, contextManager, centrifugo, llmClient} → {logger, json}.
- **Well-defined interfaces**: 16 shared types across 5 files. `llmClient.ts` and `centrifugo.ts` are the type hubs.
- **Minimal external dependencies**: 6 npm packages + 4 Node builtins. Small attack surface.

**Issues:**

#### 1. `tools.ts` is a god file (681 lines, 24 methods)

`RuntimeToolExecutor` handles:
- Tool definition catalog (BUILTIN_TOOLS array)
- Tool execution dispatch (switch statement)
- 5 file operation handlers (read, write, list, delete)
- Shell execution with tier gating
- Subagent spawning via HTTP
- Ephemeral tool running via HTTP
- File proxy support (session/persistent grants)
- Binary file detection and MIME guessing
- Path security (traversal prevention)
- Output truncation

**Impact:** Adding a new tool requires editing this single 681-line file. The class violates single-responsibility.

**Recommendation:** Split into:
- `tools/definitions.ts` — tool schema definitions (or fetched from core per ADR-001)
- `tools/executor.ts` — dispatch logic and the `executeTool()` switch
- `tools/file-handlers.ts` — file-read, file-write, file-list, file-delete
- `tools/shell-handler.ts` — shell-exec with tier gating
- `tools/proxy.ts` — subagent spawn, run-tool, core API proxy
- `tools/security.ts` — path traversal prevention, output truncation

#### 2. `index.ts` is a 361-line orchestrator

The entrypoint handles:
- Manifest loading
- Service initialization (LLMClient, tools, centrifugo, heartbeat, chat server)
- Task polling loop
- Stdin task ingestion
- Signal handling (SIGTERM)
- Centrifugo subscription for circle messages
- Shutdown coordination

**Impact:** Difficult to test in isolation. Startup sequence is implicit.

**Recommendation:** Extract a `RuntimeBootstrap` class that explicitly declares initialization order and dependencies. The entrypoint becomes a thin `bootstrap.run()` call.

#### 3. Synchronous HTTP via `spawnSync('curl')`

`tools.ts` uses `spawnSync('curl', ...)` for HTTP calls to sera-core (subagent spawning, tool running, file proxying). This is a blocking operation that:
- Freezes the entire Bun event loop during the HTTP call
- Can't be cancelled or timed out gracefully
- Doesn't support streaming responses
- Adds process spawn overhead

**Root cause:** `executeTool()` is synchronous (returns `ChatMessage` directly). The ReasoningLoop calls it synchronously in the tool iteration loop.

**Recommendation:** Make `executeTool()` async. The loop already `await`s other operations — tool execution should be async too. Replace `spawnSync('curl')` with `fetch()` (Bun has native fetch). This is a prerequisite for ADR-001 (dynamic tool catalog) since the invoke endpoint will be HTTP.

#### 4. No test coverage beyond contextManager

Only `contextManager.test.ts` exists. No tests for:
- Tool execution (file operations, shell, proxy)
- ReasoningLoop (iteration logic, tool call handling)
- ChatServer (HTTP endpoint behavior)
- LLMClient (request/response handling)

**Impact:** Refactoring is risky without tests.

**Recommendation:** Add integration tests for the ReasoningLoop (mock LLMClient + tools) and unit tests for tool handlers. The synchronous curl calls make tools hard to test — another reason to make them async.

### Agent-Runtime Summary

| Aspect | Status | Notes |
|--------|--------|-------|
| File organization | 🟢 Good | 12 files, clear purposes |
| Dependency graph | 🟢 Clean | No circular deps |
| Type interfaces | 🟢 Good | 16 types, well-scoped |
| External deps | 🟢 Minimal | 6 packages |
| tools.ts size | 🔴 Problem | 681 lines, 24 methods, god file |
| Sync HTTP calls | 🔴 Problem | spawnSync('curl') blocks event loop |
| Test coverage | 🔴 Problem | 1 test file for 11 source files |
| index.ts complexity | 🟡 Moderate | 361 lines, implicit startup order |

## Priority Actions

### Immediate (before next feature work)

1. **Make `executeTool()` async** — prerequisite for ADR-001 tool catalog
2. **Split `tools.ts`** — extract file handlers, shell handler, proxy into separate files
3. **Complete pages/ → app/ migration** — pick one convention

### Short-term

4. **Add agent-runtime tests** — at minimum for ReasoningLoop and tool dispatch
5. **Extract large page sub-components** — SettingsPage, ChatPage, ToolsPage
6. **Enforce hooks-only API access** — lint rule: no `lib/api/` imports in components

### Medium-term

7. **Extract RuntimeBootstrap** — explicit initialization order
8. **Split web types** — domain-specific type files
9. **Replace spawnSync('curl')** with native fetch
