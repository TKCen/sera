# ADR-007: Duplication and Consolidation Plan

**Status:** Proposed
**Date:** 2026-03-30

## Context

A systematic audit across web, core, and agent-runtime reveals significant duplication — legacy systems running alongside replacements, identical types defined in multiple files, and UI features implemented twice. This ADR catalogs every instance and proposes a consolidation plan that preserves all functionality.

## Inventory

### A. Dead Code (safe to delete immediately)

| File/Code | Why dead | Action |
|-----------|----------|--------|
| `web/src/hooks/useMemory.ts` | Zero importers. All 5 exports unused. | **Delete** |
| `web/src/components/MemoryGraphWrapper.tsx` | Zero importers. Legacy Next.js Suspense wrapper. | **Delete** |
| `core/src/llm/LiteLLMClient.ts` | LiteLLM sidecar removed. Only `ContextAssembler` imports `ChatMessage` from it. | **Delete**, update import |
| 8 legacy memory API functions in `web/src/lib/api/memory.ts` | `getMemoryBlocks`, `getMemoryBlock`, `getMemoryEntry`, `updateMemoryEntry`, `deleteMemoryEntry`, `addMemoryRef`, `deleteMemoryRef`, `getMemoryGraph` — none have consumers except via dead `useMemory.ts`. Exception: keep `addMemoryEntry` (used by `AgentDetailMemoryTab`). | **Delete** (except `addMemoryEntry`) |
| `web/src/lib/api/memory.ts: searchMemory()` | Duplicate of `searchMemoryBlocks()` — same endpoint, older return type. | **Delete** |

### B. Duplicate UI Features (consolidate)

#### B1. Provider Management (HIGH priority)

**Current state:** Two separate UIs managing the same provider data:

| Location | Route | What it does |
|----------|-------|-------------|
| `pages/ProvidersPage.tsx` | `/providers` | Model list, template activation, test connection, discover models, delete |
| `pages/SettingsPage.tsx` (Providers tab) | `/settings` | Dynamic provider add form, DynamicProviderCard list, CloudProviderSection |

Both use `useProviders`, `useDynamicProviders`, etc. The `CloudProviderSection` component duplicates ProvidersPage's template activation with a different UI pattern.

**Consolidation plan:**
1. Make `/providers` the canonical provider management page (it's more complete)
2. Add the dynamic provider add form (currently in SettingsPage) to ProvidersPage
3. Keep SettingsPage's "Models" tab (per-model config overrides) — this is distinct from provider lifecycle
4. Remove the "Providers" tab from SettingsPage entirely
5. Remove `/providers` from the sidebar if it's redundant with Settings, OR remove the Settings Providers tab
6. Delete `CloudProviderSection.tsx` (absorbed into unified ProvidersPage)

#### B2. Memory Views (HIGH priority)

**Current state:** Three overlapping views from different implementation generations:

| File | Route | Generation | Status |
|------|-------|-----------|--------|
| `MemoryExplorerPage.tsx` | `/memory` | Current (this session) | Active, full-featured |
| `MemoryDetailPage.tsx` | `/memory/:id` | Legacy (Epic 8) | Has inline BlockCard, own TYPE_COLORS |
| `AgentMemoryGraphPage.tsx` | `/agents/:id/memory-graph` | Legacy (Epic 8) | Separate graph view |
| `AgentDetailMemoryTab.tsx` | (tab) | Legacy (Epic 8) | Agent detail sub-tab |

**Consolidation plan:**
1. `MemoryDetailPage` → **remove**, redirect `/memory/:id` to `/memory` with agent scope pre-selected
2. `AgentMemoryGraphPage` → **remove**, redirect to `/memory` with graph expanded
3. `AgentDetailMemoryTab` → simplify to a summary card that links to `/memory?agent={id}` instead of reimplementing scope filtering
4. All memory UIs use shared `components/memory/*` components
5. Delete the inline `BlockCard` and `TYPE_COLORS` in MemoryDetailPage

#### B3. Circles Routes (MEDIUM priority)

**Current state:** Two routers both mounted at `/api/circles`:
- `circles.ts` — filesystem YAML-based CRUD
- `circles-db.ts` — PostgreSQL-based CRUD

**Consolidation plan:** Remove `circles.ts` (filesystem router). DB-backed circles are the canonical source. Any circle data in YAML files should be imported to DB on first startup.

### C. Duplicate Types (consolidate)

#### C1. ChatMessage (3 definitions)

| File | Nullable content? | Used by |
|------|-------------------|---------|
| `agents/types.ts` | `string` | BaseAgent, WorkerAgent, routes |
| `llm/LiteLLMClient.ts` | `string \| null` | ContextAssembler only |
| `llm/LlmRouter.ts` | `string \| null` | LlmRouter internals |

**Consolidation:** Delete LiteLLMClient (dead). Make `agents/types.ts` the canonical definition. Add `| null` to content if needed for LLM responses. LlmRouter re-exports from agents/types.

#### C2. Web ThoughtEvent (2 definitions)

| File | Fields |
|------|--------|
| `lib/api/types.ts` | `agentDisplayName?` (optional) |
| `lib/centrifugo.ts` | `agentDisplayName` (required) |

**Consolidation:** One definition in `lib/api/types.ts`. Make `agentDisplayName` optional.

#### C3. Message + MessageThought (2 definitions each)

| File | Notes |
|------|-------|
| `app/chat/page.tsx` | Defines both locally |
| `components/ChatThoughtPanel.tsx` | Defines both with extra `toolName?`, `toolArgs?` |

**Consolidation:** Extract to `lib/api/types.ts` (or a `types/chat.ts`). Use the superset (ChatThoughtPanel version).

#### C4. AgentInfo (2 incompatible definitions)

| File | Shape |
|------|-------|
| `components/ChatSidebar.tsx` | `{ id, name, display_name?, status? }` |
| `lib/api/types.ts` | `{ name, displayName?, status?, containerId?, ... }` (no `id`) |

**Consolidation:** Rename the ChatSidebar version to `ChatAgent` or derive via `Pick<AgentInstance, ...> & { id: string }`.

### D. Duplicate Constants

#### D1. TYPE_COLORS (4 copies)

| File | Format |
|------|--------|
| `components/memory/BlockCard.tsx` | Tailwind classes (bg-X/15 text-X) |
| `components/memory/TimelineView.tsx` | Tailwind classes (bg-X) |
| `pages/MemoryDetailPage.tsx` | Tailwind classes (bg-X/20 text-X) |
| `components/MemoryGraph.tsx` | Hex color strings (for canvas) |

**Consolidation:** Extract to `components/memory/constants.ts`:
- `MEMORY_TYPE_TAILWIND` — shared Tailwind class map
- `MEMORY_TYPE_HEX` — hex colors for canvas rendering

### E. Legacy Backend Systems (migrate then remove)

#### E1. MemoryManager + MemoryBlockStore → ScopedMemoryBlockStore

**Current consumers of legacy MemoryManager:**
- `BaseAgent` — context assembly
- `AgentFactory` — creates MemoryManager per agent
- `WorkerAgent` — memory context
- Legacy `/api/memory/blocks` routes
- `registerBuiltinSkills` — passes to skills
- `index.ts` — instantiation

**Migration plan:**
1. Update `BaseAgent`/`WorkerAgent` to use `ScopedMemoryBlockStore` for context assembly
2. Update `AgentFactory` to create scoped store instead of MemoryManager
3. Migrate legacy `/api/memory/blocks` routes to scoped store (or remove — they're superseded by `/api/memory/:agentId/blocks`)
4. Delete `MemoryManager`, `MemoryBlockStore`, and legacy block types (`human`, `persona`, `core`, `archive`)

This is Epic 19 (Memory System Consolidation) — currently deferred to Phase 4. Consider moving to Phase 3 given the duplication burden.

#### E2. OpenAIProvider → LlmRouterProvider

**Current state:** `ProviderFactory.createDefault()` falls back to `OpenAIProvider` when no LlmRouter is provided. All YAML-loaded agents use `LlmRouterProvider`.

**Migration:** Make `LlmRouterProvider` the only path. Pass `LlmRouter` everywhere. Delete `OpenAIProvider.ts`.

### F. Environment Variable Sprawl

`MEMORY_PATH` is read in 5 files with different defaults:
- `manager.ts`: `process.env.MEMORY_PATH ?? '/memory'`
- `MemoryCompactionService.ts`: `process.env.MEMORY_PATH ?? '/memory'`
- `SessionStore.ts`: `path.join(process.cwd(), '..', 'memory')` (!!)
- `SeraMCPServer.ts`: `process.env.MEMORY_PATH ?? '/memory'` (twice)

**Consolidation:** Create `core/src/lib/config.ts` that exports all env-derived config as typed constants. Single source of truth for paths, URLs, and feature flags.

## Implementation Order

### Phase 1: Safe deletes (no functionality loss)

1. Delete `web/src/hooks/useMemory.ts`
2. Delete `web/src/components/MemoryGraphWrapper.tsx`
3. Delete `core/src/llm/LiteLLMClient.ts`, update ContextAssembler import
4. Delete dead memory API functions from `web/src/lib/api/memory.ts`
5. Delete `searchMemory()` (keep `searchMemoryBlocks()`)

### Phase 2: Type consolidation

6. Unify `ChatMessage` to one definition
7. Extract `Message`, `MessageThought`, `ThoughtEvent` to shared types
8. Extract `TYPE_COLORS` to `memory/constants.ts`
9. Rename/derive `AgentInfo` in ChatSidebar

### Phase 3: UI consolidation

10. Remove SettingsPage "Providers" tab, consolidate into ProvidersPage
11. Remove `MemoryDetailPage`, redirect to MemoryExplorer
12. Remove `AgentMemoryGraphPage`, redirect to MemoryExplorer
13. Simplify `AgentDetailMemoryTab` to link to explorer

### Phase 4: Backend consolidation

14. Remove `circles.ts` filesystem router
15. Remove `OpenAIProvider` fallback
16. Centralize env var access in `config.ts`
17. Migrate MemoryManager consumers to ScopedMemoryBlockStore (Epic 19)

## Consequences

### Positive
- ~1,500 lines of dead code removed
- 4 duplicate type definitions eliminated
- 2 duplicate UI pages removed
- Single source of truth for provider management, memory browsing, and type definitions
- Easier onboarding — no confusion about "which version to use"

### Negative
- Old routes (`/memory/:id`, `/agents/:id/memory-graph`) need redirects
- `AgentDetailMemoryTab` loses self-contained functionality (becomes a link)
- MemoryManager migration (Phase 4) is a large refactor touching BaseAgent/WorkerAgent
