# ADR-005: Core Modularity and Component Interfaces

**Status:** Proposed
**Date:** 2026-03-30

## Context

A systematic analysis of `core/src/` reveals significant coupling patterns that make the codebase harder to test, extend, and reason about. This ADR documents the findings and proposes a path toward clean component interfaces.

## Findings

### 1. `agents/` is a circular dependency hub

The `agents/` module participates in **5 circular dependency pairs**:
- `agents ↔ llm`
- `agents ↔ sandbox`
- `agents ↔ services`
- `agents ↔ tools`
- `agents ↔ intercom`

14 other modules depend on agents, and agents depends on 12 modules. This makes it impossible to test, refactor, or replace any single module without touching agents.

**Root cause:** `Orchestrator.ts` (20 imports) is a god object that knows about every other component — LLM routing, sandboxing, intercom, metering, scheduling, skills, context compaction, identity, circles.

### 2. No module-level interfaces

Every consumer imports concrete files directly (`from '../agents/Orchestrator.js'`). No module has a barrel `index.ts` that exports a clean public API. This means:
- All internal files are public surface area
- Refactoring module internals breaks external consumers
- There's no way to know what's "public" vs "private" to a module

### 3. `index.ts` is a 755-line composition root

85 imports, ~30+ service instantiations, manual wiring of all dependencies. No dependency injection container or factory pattern. Adding a new service requires editing this file and threading dependencies through manually.

### 4. Pervasive singleton pattern

57 files use `getInstance()`. Route handlers call `XxxService.getInstance()` inline rather than receiving dependencies via constructor injection. This makes:
- Dependency graphs invisible (not declared in constructors)
- Testing requires mocking global state
- Service initialization order is implicit, not explicit

### 5. Clean modules exist

`audit`, `auth`, `sessions`, `secrets`, `identity`, `storage` have clean boundaries — low fan-out, low fan-in, no circular dependencies. These demonstrate the target pattern.

## Current State Metrics

| Module | Fan-out | Fan-in | Circular deps | Status |
|--------|---------|--------|---------------|--------|
| agents | 12 | 14 | 5 pairs | 🔴 Hub |
| routes | 20 | 1 | 1 pair | 🟡 Expected (composition) |
| sandbox | 5 | 7 | 1 pair | 🟡 Moderate |
| llm | 4 | 6 | 1 pair | 🟡 Moderate |
| skills | 9 | 4 | 0 | 🟡 High fan-out |
| channels | 8 | 2 | 0 | 🟡 High fan-out |
| memory | 5 | 6 | 1 pair | 🟡 Moderate |
| audit | 1 | 13 | 0 | 🟢 Clean |
| auth | 1 | 3 | 0 | 🟢 Clean |
| sessions | 1 | 3 | 0 | 🟢 Clean |
| secrets | 2 | 3 | 0 | 🟢 Clean |

## Recommended Approach

### Phase 1: Module interfaces (barrel exports)

Add `index.ts` to each major module that re-exports only the public API:

```
core/src/agents/index.ts
  export type { AgentManifest } from './manifest/types.js';
  export type { AgentInstance } from './types.js';
  export { Orchestrator } from './Orchestrator.js';
  export { AgentRegistry } from './registry.service.js';
  // Internal: BaseAgent, WorkerAgent, AgentFactory — not exported
```

Lint rule: disallow importing from `../agents/internal-file.js` — must use the barrel.

### Phase 2: Break the `agents/` hub

Split `Orchestrator` into focused services:

| Current Orchestrator responsibility | New component |
|-------------------------------------|---------------|
| Agent creation & lifecycle | `AgentLifecycleService` |
| Container management delegation | Uses `SandboxManager` (no direct dependency) |
| Heartbeat & health monitoring | `AgentHealthMonitor` |
| LLM/metering/skill wiring | Injected via interfaces, not concrete classes |
| Ephemeral TTL enforcement | `EphemeralAgentManager` |

Each new service depends on **interfaces**, not concrete implementations.

### Phase 3: Dependency injection

Replace `index.ts` manual wiring with a simple DI container:

```typescript
// container.ts
const container = new Container();
container.register('orchestrator', Orchestrator, [
  'sandboxManager', 'agentRegistry', 'identityService'
]);
container.register('sandboxManager', SandboxManager, ['docker']);
// ...
```

This makes dependency graphs explicit, testable, and removes the need for singletons.

### Phase 4: Eliminate singletons

Replace `XxxService.getInstance()` calls in routes with injected dependencies:

```typescript
// Before (hidden dependency)
router.get('/data', async (req, res) => {
  const service = ScheduleService.getInstance();
  // ...
});

// After (explicit dependency)
export function createRouter(scheduleService: ScheduleService) {
  router.get('/data', async (req, res) => {
    // scheduleService is injected
  });
}
```

## Priority Order

1. **Module barrel exports** — low risk, immediate clarity improvement
2. **Break Orchestrator god object** — medium risk, biggest modularity win
3. **Dependency injection** — medium risk, enables proper testing
4. **Eliminate singletons** — lower priority, incremental

## Consequences

### Positive
- Each module has a clear public API
- Circular dependencies eliminated
- Tests can mock at module boundaries
- New developers can understand component interfaces from barrel exports
- Refactoring internals doesn't break consumers

### Negative
- Barrel exports add indirection
- DI container adds infrastructure
- Breaking Orchestrator is a large refactor touching many files
- Existing tests need updating for DI patterns

## Appendix: index.ts Dependency Count

The main entry point creates or references these services (non-exhaustive):

```
Orchestrator, SandboxManager, IntercomService, BridgeService,
MemoryManager, SkillRegistry, ToolExecutor, CircleRegistry,
MCPRegistry, MCPServerManager, SessionStore, IdentityService,
MeteringService, MeteringEngine, AgentScheduler, AuthService,
SecretsManager, AgentRegistry, PermissionRequestService,
ProviderRegistry, LlmRouter, ContextCompactionService,
DynamicProviderManager, CircuitBreakerService, KnowledgeGitService,
MemoryCompactionService, EmbeddingService, AuditService,
ScheduleService, NotificationService, PgBossService,
TelegramAdapter, DiscordAdapter, WhatsAppAdapter
```

35+ service instances manually wired in a single file.
