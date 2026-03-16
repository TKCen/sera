# 🤖 Jules Scheduled Tasks — SERA Continuous Improvement

> Scheduled task prompts for [jules.google](https://jules.google) to systematically build out the SERA platform.
> Generated from project audit on 2026-03-16.

---

## 📊 Project State Assessment

All 8 core epics have been **scaffolded** — types, services, routes, and unit tests exist for every subsystem. However, most code was AI-generated in rapid succession and needs:

1. **Integration testing** — subsystems don't talk to each other end-to-end yet
2. **Error hardening** — happy-path only, no retry logic, no graceful degradation
3. **UI wiring** — web pages exist but many don't call real API endpoints
4. **Docker stability** — the container build has had recurring issues
5. **Documentation** — no JSDoc, no API docs, sparse README

---

## 📅 Recommended Schedule

| Cadence | Task | Priority |
|---|---|---|
| **Weekly – Monday** | [Task 1] Integration Test Suite | 🔴 Critical |
| **Weekly – Tuesday** | [Task 2] Chat System End-to-End | 🔴 Critical |
| **Weekly – Wednesday** | [Task 3] Dashboard UI Wiring | 🟡 High |
| **Weekly – Thursday** | [Task 4] Memory System Hardening | 🟡 High |
| **Weekly – Friday** | [Task 5] Docker Build Reliability | 🟡 High |
| **Bi-weekly – Week 1 Monday** | [Task 6] API Documentation & OpenAPI Spec | 🟢 Medium |
| **Bi-weekly – Week 1 Wednesday** | [Task 7] Intercom ↔ Centrifugo Integration | 🟢 Medium |
| **Bi-weekly – Week 2 Monday** | [Task 8] Agent Manifest Editor UI | 🟢 Medium |
| **Bi-weekly – Week 2 Wednesday** | [Task 9] Memory Graph Visualization | 🟢 Medium |
| **Monthly – 1st** | [Task 10] Security Audit & Tier Enforcement | 🔵 Ongoing |
| **Monthly – 15th** | [Task 11] Performance & Dead Code Cleanup | 🔵 Ongoing |
| **Monthly – 22nd** | [Task 12] Dependency Updates & Vulnerability Scan | 🔵 Ongoing |

---

## 🔴 Task 1: Integration Test Suite

**Schedule:** Weekly — Monday  
**Estimated duration:** 30–45 min

```
SERA Integration Test Suite Expansion

Repository: homelab (sera/ subdirectory)

Context:
The SERA project has unit tests for individual subsystems (AgentManifestLoader, CircleRegistry,
SandboxManager, TierPolicy, IntercomService, ChannelNamespace, MemoryBlockStore, ProcessManager,
SkillRegistry, StorageProvider) but no integration tests that verify subsystems work together.

Task:
1. Review all existing test files in `core/src/` (look for `*.test.ts` files) to understand
   current coverage.

2. Create an integration test file at `core/src/__tests__/integration.test.ts` that tests:
   a. Agent loading → Orchestrator registers agents from AGENT.yaml manifests in `agents/`
   b. Circle loading → CircleRegistry validates agent references against loaded manifests
   c. Chat flow → POST /api/chat hits the orchestrator, which uses a loaded agent
   d. Memory flow → Creating a memory entry via POST /api/memory/blocks/:type, then
      retrieving it via GET /api/memory/entries/:id
   e. Skills flow → SkillRegistry has builtin skills registered after server boot

3. Use the existing test setup — the project uses Vitest. Check `core/package.json` for
   the test script. Use `supertest` for HTTP assertions (already a dependency).

4. Mock external services (LLM provider, Qdrant, PostgreSQL) — do NOT make real API calls.

5. Run `npm test` in `core/` and ensure all existing + new tests pass.

6. If any existing tests are broken, fix them before adding new ones.

Do NOT modify any production code. Only add/fix test files.
```

---

## 🔴 Task 2: Chat System End-to-End

**Schedule:** Weekly — Tuesday  
**Estimated duration:** 45–60 min

```
SERA Chat System End-to-End Wiring

Repository: homelab (sera/ subdirectory)

Context:
The chat page exists at `web/src/app/chat/page.tsx` and the backend has `POST /api/chat`
and `POST /api/execute` endpoints. The chat currently uses a basic primary agent with no
conversation persistence, no streaming, and no agent selection.

Task:
1. Review the current chat page at `web/src/app/chat/page.tsx` and the backend chat
   endpoint in `core/src/index.ts` (search for `/api/chat`).

2. Improve the backend chat endpoint:
   a. Accept an optional `agentName` parameter to route to a specific agent (not just
      the primary agent). Use `orchestrator.getAgent(agentName)` to resolve.
   b. Add conversation history to the LLM context — pass the full `history` array as
      messages to the agent, not just the latest message.
   c. Add proper error messages when the agent fails (distinguish between "no agent found"
      vs "LLM error" vs "timeout").

3. Improve the frontend chat page:
   a. Add an agent selector dropdown that fetches available agents from `GET /api/agents`.
   b. Pass the selected agent name in the chat request body.
   c. Show a loading/thinking indicator while waiting for a response.
   d. Display the agent's `thought` field (from the response) in a collapsible "thinking"
      panel below each message.
   e. Support markdown rendering in chat messages (use a library like `react-markdown`
      if not already present).

4. Ensure the Next.js API proxy at `web/src/app/api/` correctly forwards to sera-core.

5. Test by running `npm run dev` in both `core/` and `web/` and sending messages.
   Verify the chat works with the architect-prime agent.

Keep the existing UI design system (check `web/src/app/globals.css` and
`web/src/components/` for the design tokens and components).
```

---

## 🟡 Task 3: Dashboard UI Wiring

**Schedule:** Weekly — Wednesday  
**Estimated duration:** 45–60 min

```
SERA Dashboard Pages — Wire to Real API Endpoints

Repository: homelab (sera/ subdirectory)

Context:
The SERA web app has pages for agents, circles, tools, settings, and insights but many
display placeholder data or are not yet connected to the backend API. The backend has
working API routes for all these entities.

Task:
1. Audit each dashboard page to determine which ones are wired vs placeholder:
   - `web/src/app/agents/page.tsx` — should call `GET /api/agents`
   - `web/src/app/agents/[id]/page.tsx` — should call `GET /api/agents/:id`
   - `web/src/app/circles/page.tsx` — should call `GET /api/circles`
   - `web/src/app/tools/page.tsx` — should call `GET /api/skills`
   - `web/src/app/settings/page.tsx` — should call `GET /api/config/llm` and
     `GET /api/providers`

2. For each page that is NOT wired to real data:
   a. Add `fetch()` calls to the appropriate API endpoint.
   b. Use the Next.js API proxy (requests to `/api/*` are proxied to sera-core:3001).
      Check `web/next.config.ts` for the rewrite rules.
   c. Add loading states and error states.
   d. Map the API response to the existing UI components.

3. For the agents detail page (`agents/[id]/page.tsx`):
   a. Display the full agent manifest (identity, model, tools, skills, resources).
   b. Show the agent's memory entries (fetch from `GET /api/memory/blocks`).
   c. Add a "Chat with this agent" button that navigates to `/chat?agent={name}`.

4. Do NOT change the CSS design system or overall layout/sidebar.
   Only modify the data fetching and display logic within each page.

5. Verify by running `npm run dev` in `web/` and navigating to each page.
   Confirm no console errors and that real data loads from the backend.
```

---

## 🟡 Task 4: Memory System Hardening

**Schedule:** Weekly — Thursday  
**Estimated duration:** 30–45 min

```
SERA Memory System Hardening & Edge Cases

Repository: homelab (sera/ subdirectory)

Context:
The memory system has a MemoryBlockStore that reads/writes markdown files with YAML
frontmatter, a MemoryManager that coordinates blocks, a Reflector for auto-compaction,
and a graph API. The implementation is functional but needs hardening.

Task:
1. Review these files:
   - `core/src/memory/blocks/MemoryBlockStore.ts`
   - `core/src/memory/blocks/types.ts`
   - `core/src/memory/manager.ts`
   - `core/src/memory/Reflector.ts`

2. Harden the MemoryBlockStore:
   a. Add file locking or mutex to prevent concurrent writes to the same file.
   b. Handle corrupted/invalid frontmatter gracefully — log a warning and skip
      the file rather than crashing.
   c. Validate that wikilinks (`[[Title]]`) reference existing entries and log
      warnings for broken links.
   d. Add a `repair()` method that scans for orphaned refs and broken wikilinks.

3. Harden the MemoryManager:
   a. Add input validation for all public methods (null checks, type checks).
   b. Add rate limiting awareness — if an agent writes more than 10 entries/minute,
      log a warning.
   c. Ensure the `search()` method handles empty results gracefully.

4. Harden the Reflector:
   a. Add a configurable compaction threshold (currently hardcoded).
   b. Handle LLM failures during summarization — retry once, then skip.
   c. Log compaction metrics (entries compacted, archive entries created).

5. Add edge-case tests to `core/src/memory/blocks/MemoryBlockStore.test.ts`:
   - Test with malformed YAML frontmatter
   - Test with concurrent writes to the same block type
   - Test with very large content (>100KB)
   - Test with special characters in titles

6. Run `npm test` in `core/` and ensure all tests pass.
```

---

## 🟡 Task 5: Docker Build Reliability

**Schedule:** Weekly — Friday  
**Estimated duration:** 30–45 min

```
SERA Docker Build & Runtime Reliability

Repository: homelab (sera/ subdirectory)

Context:
The SERA project uses Docker Compose with 5 services: sera-core, sera-web, centrifugo,
sera-db (pgvector), and qdrant. The sera-core container has had recurring issues with
module resolution (ERR_MODULE_NOT_FOUND) and path resolution (workspace directories).

Task:
1. Review the Docker configuration:
   - `docker-compose.yaml` (root)
   - `core/Dockerfile`
   - `web/Dockerfile`
   - `core/package.json` (check build scripts)
   - `core/tsconfig.json` (check module resolution settings)

2. Verify the sera-core Dockerfile:
   a. Ensure all dependencies are installed in the production stage.
   b. Verify the TypeScript compilation produces correct ESM output.
   c. Confirm the `WORKSPACE_DIR` environment variable is correctly passed and
      used in `core/src/index.ts` for resolving the agents/ and circles/ directories.
   d. Add a health check to the Dockerfile: `HEALTHCHECK CMD curl -f http://localhost:3001/api/health || exit 1`

3. Verify the sera-web Dockerfile:
   a. Ensure the Next.js build completes without errors.
   b. Confirm that the `CORE_API_URL` environment variable is correctly used for
      API proxying.

4. Add a `docker-compose.healthcheck.yaml` override file or add healthcheck
   directives to the main `docker-compose.yaml` for all services:
   - sera-core: `curl http://localhost:3001/api/health`
   - sera-web: `curl http://localhost:3000/api/health`
   - sera-db: `pg_isready`
   - qdrant: `curl http://localhost:6333/readyz`

5. Test by running `docker compose build` and `docker compose up` from the `sera/`
   directory. Verify:
   - All containers start without errors
   - `curl http://localhost:3001/api/health` returns `{"status":"ok"}`
   - The web UI is accessible

6. If any build issues are found, fix them. Document any workarounds in comments.
```

---

## 🟢 Task 6: API Documentation & OpenAPI Spec

**Schedule:** Bi-weekly — Week 1, Monday  
**Estimated duration:** 45–60 min

```
SERA API Documentation & OpenAPI Specification

Repository: homelab (sera/ subdirectory)

Context:
SERA's core API has ~40 endpoints across agents, circles, chat, memory, skills, sandbox,
intercom, config, providers, and vector search. There is a partial API_SCHEMAS.md in
docs/ but no machine-readable spec.

Task:
1. Review all route files to catalog every endpoint:
   - `core/src/index.ts` (inline routes for chat, memory, config, providers, party mode)
   - `core/src/routes/agents.ts`
   - `core/src/routes/circles.ts`
   - `core/src/routes/skills.ts`
   - `core/src/routes/sandbox.ts`
   - `core/src/routes/intercom.ts`
   - `core/src/routes/lsp.ts`

2. Create/update `docs/API_SCHEMAS.md` with a complete reference:
   - Group by domain (Agents, Circles, Chat, Memory, Skills, Sandbox, Intercom, Config)
   - For each endpoint: method, path, request body, response body, error codes
   - Include example request/response JSON

3. Create an OpenAPI 3.1 spec at `docs/openapi.yaml`:
   - Define all endpoints with request/response schemas
   - Use $ref components for reusable types (AgentManifest, MemoryEntry, CircleInfo, etc.)
   - Include server URLs for both local development and Docker deployment

4. Add JSDoc comments to all route handler functions in the route files.
   Use @param and @returns tags with TypeScript types.

5. Verify the OpenAPI spec is valid by running:
   `npx -y @redocly/cli lint docs/openapi.yaml`

Do NOT change any endpoint behavior. Documentation only.
```

---

## 🟢 Task 7: Intercom ↔ Centrifugo Live Integration

**Schedule:** Bi-weekly — Week 1, Wednesday  
**Estimated duration:** 45–60 min

```
SERA Intercom Service — Live Centrifugo Integration

Repository: homelab (sera/ subdirectory)

Context:
The IntercomService exists at `core/src/intercom/IntercomService.ts` with channel namespace
validation and message types. Centrifugo is deployed in Docker Compose. However, the
IntercomService currently does NOT make real HTTP calls to Centrifugo — it needs to be
connected to the live Centrifugo HTTP API.

Task:
1. Review the current implementation:
   - `core/src/intercom/IntercomService.ts`
   - `core/src/intercom/types.ts`
   - `core/src/intercom/ChannelNamespace.ts`
   - `centrifugo/config.json`

2. Connect IntercomService to the real Centrifugo HTTP API:
   a. Use the `CENTRIFUGO_API_URL` env var (defaults to `http://centrifugo:8000/api`).
   b. Read the Centrifugo API key from `centrifugo/config.json` or an env var.
   c. Implement `publish(channel, data)` using Centrifugo's HTTP publish API.
   d. Implement `presence(channel)` to check who is subscribed.
   e. Implement `history(channel)` to retrieve recent messages.

3. Update the Centrifugo config (`centrifugo/config.json`):
   a. Add namespace definitions matching `ChannelNamespace.ts`:
      - `internal:` — for agent thoughts and terminal streams
      - `intercom:` — for agent-to-agent messaging
      - `channel:` — for circle-wide broadcasts
   b. Enable history for the `intercom:` namespace.
   c. Set appropriate TTLs and history sizes.

4. Wire thought streaming into the agent reasoning loop:
   a. In `core/src/agents/BaseAgent.ts`, after each reasoning step, publish
      the thought to `internal:agent:{id}:thoughts` via IntercomService.
   b. This should be non-blocking (fire-and-forget with error logging).

5. Add a web UI component that subscribes to Centrifugo via WebSocket:
   a. In the chat page, connect to Centrifugo using the `centrifuge-js` npm package.
   b. Subscribe to the current agent's thought channel.
   c. Display thoughts in the existing thought panel.

6. Test by running all services via Docker Compose and sending a chat message.
   Verify that thoughts appear in real-time on the web UI.
```

---

## 🟢 Task 8: Agent Manifest Editor UI

**Schedule:** Bi-weekly — Week 2, Monday  
**Estimated duration:** 45–60 min

```
SERA Agent Manifest Editor — Web UI

Repository: homelab (sera/ subdirectory)

Context:
Agents are defined in AGENT.yaml files and loaded by AgentManifestLoader. The agents
detail page exists at `web/src/app/agents/[id]/page.tsx` but there is no editor UI.
The backend has `PUT /api/agents/:id/manifest` for saving manifests. The goal is
letting users modify agents from the dashboard without editing YAML files.

Task:
1. Review:
   - `web/src/app/agents/[id]/page.tsx` — current detail page
   - `core/src/routes/agents.ts` — API endpoints
   - `core/src/agents/manifest/types.ts` — AgentManifest type definition
   - `agents/architect.agent.yaml` — example manifest for field reference

2. Create `web/src/app/agents/[id]/edit/page.tsx` with a form-based editor:
   a. **Identity section**: Editable fields for role, description, communicationStyle,
      and principles (list editor).
   b. **Model section**: Provider dropdown, model name input, temperature slider,
      fallback model configuration.
   c. **Tools section**: Checkbox list for allowed/denied tools. Fetching available
      tools from `GET /api/skills`.
   d. **Subagents section**: List editor for allowed roles with maxInstances.
   e. **Resources section**: Memory and CPU inputs with validation.
   f. **Raw YAML toggle**: A code editor (textarea with monospace font) that shows
      the raw YAML and allows direct editing.

3. Add form validation:
   - Required fields: metadata.name, identity.role, model.provider, model.name
   - Numeric validation for temperature (0-2), resources
   - Security tier must be 1, 2, or 3

4. On save, POST to `PUT /api/agents/:id/manifest` and show success/error toast.
   After save, trigger a reload via `POST /api/agents/reload`.

5. Add an "Edit" button to the agent detail page that navigates to the edit page.

6. Use the existing design system from `web/src/app/globals.css` and match the
   style of other pages (dark theme, glass panels, etc.).

7. Verify by running `npm run dev` in `web/`, navigating to an agent, clicking
   Edit, modifying a field, saving, and confirming the YAML file was updated.
```

---

## 🟢 Task 9: Memory Graph Visualization

**Schedule:** Bi-weekly — Week 2, Wednesday  
**Estimated duration:** 45–60 min

```
SERA Memory Graph Visualization — Interactive Knowledge Map

Repository: homelab (sera/ subdirectory)

Context:
The memory system has a graph API at `GET /api/memory/graph` that returns nodes (memory
entries) and edges (refs + wikilinks). The `insights` page at `web/src/app/insights/page.tsx`
exists but likely has placeholder content. The goal is an interactive graph visualization.

Task:
1. Review:
   - `core/src/memory/manager.ts` — the `getGraph()` method
   - `core/src/memory/blocks/MemoryBlockStore.ts` — how refs and wikilinks are resolved
   - `web/src/app/insights/page.tsx` — current insights page

2. Install a graph visualization library in the web project:
   - Use `@react-sigma/core` + `graphology` (lightweight, React-native), OR
   - Use `react-force-graph-2d` (simpler, force-directed).
   - Choose whichever is simpler to integrate.

3. Create a graph visualization component at `web/src/components/MemoryGraph.tsx`:
   a. Fetch data from `GET /api/memory/graph`.
   b. Render nodes as circles colored by block type (human=blue, persona=purple,
      core=green, archive=gray).
   c. Render edges as lines between connected nodes.
   d. On node click, show a tooltip/panel with the entry title, type, tags, and
      content preview.
   e. On node double-click, navigate to the entry detail view.
   f. Support zoom and pan.

4. Integrate the graph into the insights page:
   a. Replace any placeholder content with the MemoryGraph component.
   b. Add filter controls: filter by block type, filter by tag.
   c. Add a search box that highlights matching nodes.

5. Style the graph to match the existing dark theme design system.

6. Test by:
   a. Creating a few memory entries via `POST /api/memory/blocks/core` with refs
      between them.
   b. Navigating to the insights page and verifying the graph renders correctly.
   c. Clicking nodes and verifying the detail panel appears.
```

---

## 🔵 Task 10: Security Audit & Tier Enforcement

**Schedule:** Monthly — 1st  
**Estimated duration:** 60 min

```
SERA Security Audit — Tier Enforcement & Sandbox Hardening

Repository: homelab (sera/ subdirectory)

Context:
SERA uses a 3-tier security model defined in AGENT.yaml manifests and enforced by
TierPolicy.ts. The SandboxManager manages Docker containers for agent workloads.
The system needs regular auditing to ensure security boundaries hold.

Task:
1. Review the security boundary implementation:
   - `core/src/sandbox/TierPolicy.ts` — tier enforcement rules
   - `core/src/sandbox/SandboxManager.ts` — container management
   - `core/src/sandbox/types.ts` — security types
   - `core/src/agents/manifest/types.ts` — AgentManifest security fields

2. Audit tier enforcement:
   a. Verify Tier 1 agents cannot: access network, mount host paths, exec shell commands.
   b. Verify Tier 2 agents: can access sera_net only, have resource limits applied.
   c. Verify Tier 3 agents: have full capability but are logged to audit trail.
   d. Write test cases for each tier boundary in `core/src/sandbox/TierPolicy.test.ts`.

3. Audit the sandbox routes:
   a. Verify that `POST /api/sandbox/spawn` checks the requesting agent's tier
      before creating a container.
   b. Verify that `POST /api/sandbox/exec` validates the command against the agent's
      allowed tools list.
   c. Check for any routes that bypass security checks.

4. Review Docker socket access:
   a. The Docker socket is mounted in sera-core. Verify that only SandboxManager
      accesses it and no other code path can create containers directly.
   b. Recommend mitigations if direct socket access is too permissive.

5. Check for common vulnerabilities:
   a. Path traversal in file-read/file-write skills.
   b. Injection attacks in sandbox exec commands.
   c. SSRF via web-search skill.

6. Create a security report at `docs/security-audit-{date}.md` with findings,
   risk ratings, and recommended fixes. Implement any critical fixes immediately.
```

---

## 🔵 Task 11: Performance & Dead Code Cleanup

**Schedule:** Monthly — 15th  
**Estimated duration:** 30–45 min

```
SERA Performance Review & Dead Code Cleanup

Repository: homelab (sera/ subdirectory)

Context:
The SERA codebase was rapidly prototyped and has gone through multiple refactors.
There may be unused imports, dead code paths, and performance bottlenecks.

Task:
1. Run a dead code analysis:
   a. Check for unused exports in `core/src/` — files that are never imported.
   b. Check for unused dependencies in `core/package.json` and `web/package.json`
      using `npx depcheck`.
   c. Remove any dead code or unused dependencies found.

2. Review performance hotspots:
   a. The `AgentManifestLoader.loadAllManifests()` is called synchronously at startup.
      If there are many YAML files, this could slow boot time. Add timing logs.
   b. The `MemoryBlockStore` reads all files on every `getAll()` call. Consider
      adding an in-memory cache with file-watcher invalidation.
   c. The `CircleRegistry.loadFromDirectory()` is synchronous. Consider making
      it async.

3. Review the TypeScript compilation:
   a. Check `core/tsconfig.json` for strict mode settings.
   b. Run `npx tsc --noEmit` and fix any type errors.
   c. Enable `exactOptionalPropertyTypes` if not already enabled and fix resulting errors.

4. Clean up console.log statements:
   a. Replace raw `console.log` with a structured logger (or at minimum, prefix
      all logs with `[ComponentName]` for traceability).
   b. Remove any debug-only logging.

5. Run existing tests (`npm test` in `core/`) and ensure nothing is broken
   by the cleanup.
```

---

## 🔵 Task 12: Dependency Updates & Vulnerability Scan

**Schedule:** Monthly — 22nd  
**Estimated duration:** 20–30 min

```
SERA Dependency Updates & Vulnerability Scan

Repository: homelab (sera/ subdirectory)

Context:
SERA depends on several npm packages across core/ and web/. Regular updates prevent
security vulnerabilities and ensure compatibility.

Task:
1. Run vulnerability scans:
   a. `cd core && npm audit` — document any findings.
   b. `cd web && npm audit` — document any findings.

2. Update dependencies:
   a. Run `npm outdated` in both `core/` and `web/` to see available updates.
   b. Update patch and minor versions: `npm update` in both directories.
   c. For major version updates, review the changelog for breaking changes before
      updating. Only update major versions if the breaking changes are manageable.

3. Critical packages to keep current:
   - `express` (core)
   - `openai` (core — LLM provider)
   - `dockerode` (core — sandbox manager)
   - `next` (web)
   - `react` / `react-dom` (web)

4. After updates:
   a. Run `npm run build` in `core/` — ensure TypeScript compilation succeeds.
   b. Run `npm run build` in `web/` — ensure Next.js build succeeds.
   c. Run `npm test` in `core/` — ensure all tests pass.

5. If any tests fail after updates, investigate and fix or pin the problematic
   dependency to the last working version.

6. Commit with message: `chore(deps): monthly dependency update YYYY-MM-DD`
```

---

## 📋 Jules Configuration Notes

### How to set up in Jules

1. Go to [jules.google](https://jules.google) and open the **homelab** repository
2. For each task above, create a scheduled task with:
   - **Title**: The task name (e.g., "Integration Test Suite")
   - **Prompt**: The code block content for that task
   - **Schedule**: As indicated in the table above
   - **Branch**: Each task should create a feature branch (e.g., `jules/integration-tests`)

### Task dependencies

```
Task 1 (Tests) ────── runs independently, foundational
Task 2 (Chat) ─────── runs independently
Task 3 (UI Wiring) ── depends on Task 2 for chat agent selector
Task 4 (Memory) ───── runs independently
Task 5 (Docker) ───── runs independently
Task 6 (API Docs) ─── runs independently (read-only)
Task 7 (Intercom) ─── runs independently
Task 8 (Editor UI) ── depends on Task 3 for agents page wiring
Task 9 (Graph) ────── depends on Task 4 for hardened memory
Task 10 (Security) ── runs independently
Task 11 (Cleanup) ─── runs independently
Task 12 (Deps) ────── runs independently
```

### PR Review strategy

- **Tasks 1, 5, 6, 11, 12**: Auto-merge safe (tests, docs, cleanup)
- **Tasks 2, 3, 4, 7, 8, 9**: Review before merge (functional changes)
- **Task 10**: Always review (security-related)

### Scaling up

Once these stabilize, consider adding:
- **Adapter System** — Telegram/Discord/WhatsApp integration (from master plan Phase 1)
- **LSP Integration** — Symbol-level code tools for agents (master plan Phase 3)
- **Browser Automation** — Playwright-based agent hands (master plan Phase 4)
- **Flow Pipelines** — Event-driven multi-stage tasks (master plan Phase 4)
