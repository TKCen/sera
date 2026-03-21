# Jules Recurring Tasks — SERA Continuous Maintenance

> Prompts for scheduled Jules tasks that run on a regular cadence.
> Each prompt is self-contained — paste directly into Jules as a scheduled task.
> Results arrive as PRs. Review via the `integrate-agent-pr` workflow.

---

## Schedule overview

### Code quality (maintenance)

| Cadence | Task | Purpose |
|---|---|---|
| **Weekly (Mon)** | Type safety sweep | Catch `any` types, unsafe casts, missing return types |
| **Weekly (Wed)** | Dead code & unused exports | Remove code that nothing imports |
| **Weekly (Fri)** | Test coverage gaps | Add tests for uncovered files |
| **Bi-weekly (1st Mon)** | Dependency audit | `bun audit` + outdated check + update |
| **Bi-weekly (2nd Mon)** | TODO/FIXME sweep | Address or triage accumulated tech debt markers |
| **Monthly (1st)** | API docs sync | Ensure `docs/openapi.yaml` matches actual routes |
| **Monthly (15th)** | Console.log cleanup | Replace raw logs with structured logger calls |

### UX, features & refactoring

| Cadence | Task | Purpose |
|---|---|---|
| **Weekly (Tue)** | Accessibility & UX polish | Semantic HTML, ARIA, focus management, skeleton loaders — one page per run |
| **Weekly (Thu)** | Error resilience pass | Error boundaries, toast system, retry buttons — one page per run |
| **Bi-weekly (1st/3rd Wed)** | Small feature enhancements | Backlog of 7 small features, one per run |
| **Bi-weekly (2nd/4th Wed)** | Component extraction (web) | Split oversized pages into focused components |
| **Bi-weekly (2nd/4th Fri)** | Service modularization (core) | Split oversized services into focused modules |
| **Monthly (8th)** | Legacy page cleanup | Migrate pages/ → app/, remove duplicates |

---

## Weekly — Type Safety Sweep

**Schedule:** Every Monday

```
Repository: TKCen/sera
Branch: jules/type-safety-sweep

Read AGENTS.md in the project root for coding conventions.

Task: Find and fix type safety issues across the codebase.

Steps:
1. Run `bun run typecheck` in the project root. If there are existing errors, fix them first.

2. Search for `any` types in core/src/ and web/src/:
   - grep -rn ': any' core/src/ web/src/
   - grep -rn 'as any' core/src/ web/src/
   - grep -rn '<any>' core/src/ web/src/

3. For each `any` found:
   - Replace with the correct specific type
   - If the correct type is unclear, look at how the value is used downstream
   - If it's a third-party library type, check node_modules for the real type
   - If genuinely unavoidable, add a comment: // eslint-disable-next-line @typescript-eslint/no-explicit-any -- <reason>

4. Search for unsafe type assertions:
   - grep -rn 'as unknown as' core/src/ web/src/
   - Replace with proper type narrowing (type guards, instanceof, etc.)

5. Add explicit return types to any exported functions that are missing them.

6. Run the full validation: bun run typecheck && bun run lint && bun run test
   Fix any failures.

Rules:
- Do NOT change runtime behavior — type annotations only
- Do NOT add new dependencies
- If fixing a type would require a large refactor, skip it and add a TODO comment instead
- Maximum 20 files changed per PR to keep reviews manageable
```

---

## Weekly — Dead Code & Unused Exports

**Schedule:** Every Wednesday

```
Repository: TKCen/sera
Branch: jules/dead-code-cleanup

Read AGENTS.md in the project root for coding conventions.

Task: Find and remove dead code, unused exports, and unused imports.

Steps:
1. Check for unused exports in core/src/:
   For each .ts file, check if its exports are imported anywhere else.
   Use grep to verify: grep -rn 'import.*{.*ExportName' core/src/

2. Check for unused exports in web/src/:
   Same approach. Pay attention to React components — they may be used
   in route files or lazy-loaded.

3. Check for unused imports in all .ts and .tsx files:
   The linter should catch most of these. Run: bun run lint
   Fix any unused-import warnings.

4. Check for unused dependencies:
   - In core/: look at package.json dependencies, grep for each package
     name in core/src/. If not imported anywhere, remove it.
   - In web/: same approach for web/package.json and web/src/.
   - Do NOT remove devDependencies (vitest, typescript, eslint, etc.)

5. Look for dead code patterns:
   - Functions that are defined but never called
   - Variables assigned but never read
   - Unreachable code after return/throw statements
   - Commented-out code blocks (remove them — git has history)

6. Run validation: bun run typecheck && bun run lint && bun run test
   If removing an export breaks a test, the test may be testing dead code — remove both.

Rules:
- Do NOT remove code that is referenced in docs/epics/ as planned future work
- Do NOT remove anything from the public API routes (even if currently unused by the frontend)
- Maximum 15 files changed per PR
- If unsure whether something is used, leave it and move on
```

---

## Weekly — Test Coverage Gaps

**Schedule:** Every Friday

```
Repository: TKCen/sera
Branch: jules/test-coverage

Read AGENTS.md in the project root for coding conventions.
Read docs/TESTING.md for the testing strategy.

Task: Identify files with no test coverage and add unit tests.

Steps:
1. List all .ts files in core/src/ that do NOT have a corresponding .test.ts:
   For each core/src/**/*.ts file, check if core/src/**/*.test.ts exists.

2. Prioritize files by importance:
   - Route handlers (core/src/routes/*.ts) — test request/response shapes
   - Service classes — test public methods
   - Utility functions — test edge cases
   - Skip: type-only files, index.ts re-exports, config files

3. For each uncovered file (pick up to 3 per run):
   a. Read the file and understand its public API
   b. Create a .test.ts file next to it (or in __tests__/)
   c. Write tests for:
      - Happy path for each public method
      - Error/edge cases (null input, empty arrays, invalid data)
      - Boundary conditions
   d. Use Vitest (import { describe, it, expect, vi } from 'vitest')
   e. Mock external dependencies (database, HTTP, filesystem)

4. Also check web/src/ for untested components:
   - Focus on hooks (web/src/hooks/) and utility functions (web/src/lib/)
   - Skip pure UI components unless they have complex logic

5. Run: bun run test
   All new and existing tests must pass.

Rules:
- Unit tests only — no database, no Docker, no HTTP calls
- Mock all I/O with vi.mock() or vi.fn()
- Tests should run in < 5ms each
- Do NOT modify production code to make it testable (no test-only exports)
- Maximum 5 new test files per PR
```

---

## Bi-weekly — Dependency Audit

**Schedule:** 1st and 3rd Monday of the month

```
Repository: TKCen/sera
Branch: jules/dependency-audit

Read AGENTS.md in the project root for coding conventions.

Task: Audit and update dependencies for security and freshness.

Steps:
1. Run security audit:
   cd core && bun audit --json > /tmp/core-audit.json
   cd web && bun audit --json > /tmp/web-audit.json

   If there are high or critical vulnerabilities, fix them:
   bun audit fix

2. Check for outdated packages:
   cd core && bun outdated
   cd web && bun outdated

3. Update strategy:
   - PATCH versions: update all (bun update)
   - MINOR versions: update all unless changelog mentions breaking behavior
   - MAJOR versions: do NOT update — just note them in the PR description

4. After updating:
   bun run typecheck    # in project root
   bun run lint         # in project root
   bun run test         # in project root

   If any test fails after an update, revert that specific package:
   bun add <package>@<previous-version>

5. In the PR description, list:
   - Packages updated (name, old version → new version)
   - Vulnerabilities fixed
   - Major versions available but not updated (and why)

Rules:
- NEVER update typescript, vitest, or eslint major versions without checking compatibility
- NEVER remove a dependency — only update
- If bun audit fix would change many packages, do it in a separate commit
- Keep bun.lock changes in the same commit as package.json changes
```

---

## Bi-weekly — TODO/FIXME Sweep

**Schedule:** 2nd and 4th Monday of the month

```
Repository: TKCen/sera
Branch: jules/todo-sweep

Read AGENTS.md in the project root for coding conventions.

Task: Find TODO/FIXME/HACK comments in the codebase and either fix them or create tracking issues.

Steps:
1. Find all TODO/FIXME/HACK comments:
   grep -rn 'TODO\|FIXME\|HACK\|XXX\|WORKAROUND' core/src/ web/src/ --include='*.ts' --include='*.tsx'

2. For each comment, decide:
   a. **Fix it now** — if the fix is small (< 20 lines) and self-contained
   b. **Skip it** — if it references planned future work in docs/epics/
   c. **Remove it** — if the TODO is outdated (the work was already done)

3. For items you fix:
   - Implement the fix
   - Remove the TODO comment
   - Add a test if the fix changes behavior

4. For items that are outdated:
   - Verify the work was done (check git log or the current code)
   - Remove the stale comment

5. Run validation: bun run typecheck && bun run lint && bun run test

Rules:
- Maximum 10 TODOs addressed per PR
- Do NOT fix TODOs that would require architectural changes
- Do NOT create GitHub issues from this task — just fix or skip
- If a TODO says "after epic X" and that epic isn't done, skip it
```

---

## Monthly — API Docs Sync

**Schedule:** 1st of the month

```
Repository: TKCen/sera
Branch: jules/api-docs-sync

Read AGENTS.md in the project root for coding conventions.

Task: Ensure docs/openapi.yaml and docs/API_SCHEMAS.md match the actual API routes.

Steps:
1. Catalog all current API endpoints by reading route files:
   - core/src/routes/agents.ts
   - core/src/routes/tasks.ts
   - core/src/routes/providers.ts
   - core/src/index.ts (inline routes)
   - Any other files in core/src/routes/

   For each endpoint, note: method, path, request body type, response type.

2. Compare against docs/openapi.yaml:
   - Add any endpoints that exist in code but not in the spec
   - Remove any endpoints that are in the spec but no longer in code
   - Update request/response schemas if the types have changed

3. Compare against docs/API_SCHEMAS.md:
   - Update the human-readable docs to match
   - Include example request/response JSON for new endpoints

4. Validate the OpenAPI spec is valid YAML and well-formed.

5. Do NOT change any production code — documentation only.

Rules:
- Match the existing format and style in openapi.yaml
- Use $ref for shared schema components
- Include all error response codes (400, 404, 500 at minimum)
- If an endpoint is complex, add a description field explaining its purpose
```

---

## Monthly — Console.log Cleanup

**Schedule:** 15th of the month

```
Repository: TKCen/sera
Branch: jules/logging-cleanup

Read AGENTS.md in the project root for coding conventions.

Task: Replace raw console.log/warn/error with structured, prefixed logging.

Steps:
1. Find all raw console.log/warn/error calls:
   grep -rn 'console\.\(log\|warn\|error\|debug\|info\)' core/src/ web/src/ --include='*.ts' --include='*.tsx'

2. For core/src/:
   - Check if there's an existing logger utility. If so, use it.
   - If not, each console.log should at minimum be prefixed with the
     component name: console.log('[ComponentName]', ...)
   - Remove debug-only logging that shouldn't be in production
   - Keep error logging but ensure it includes context (what failed, what input caused it)

3. For web/src/:
   - Remove console.log statements that were for development debugging
   - Keep console.error for actual error handling
   - Prefix remaining logs with component name

4. Patterns to fix:
   - console.log(variable) → remove or add context
   - console.log('here') or console.log('test') → remove
   - console.error(err) → console.error('[ComponentName] description:', err)
   - console.log(JSON.stringify(x)) → remove (debugging artifact)

5. Run validation: bun run typecheck && bun run lint && bun run test

Rules:
- Do NOT introduce a new logging library unless one already exists
- Do NOT remove console.error calls in catch blocks — improve them
- Maximum 15 files changed per PR
```

---

---

# UX, Features & Refactoring

These tasks improve user experience, add small enhancements, and modularize oversized files.
They are higher-touch than the maintenance tasks above — review PRs carefully.

---

## Weekly — Accessibility & UX Polish Pass

**Schedule:** Every Tuesday

```
Repository: TKCen/sera
Branch: jules/ux-polish

Read AGENTS.md in the project root for coding conventions.
Read web/CLAUDE.md for frontend conventions.

Task: Improve accessibility and UX polish across the web dashboard — one page per run.

Context:
Most pages already have loading spinners, error banners, and empty states.
The real gaps are: zero aria attributes, no semantic HTML, no focus management,
no error boundaries, and alert() used instead of toast notifications.
Existing UI primitives: web/src/components/ui/ (alert, badge, button, card, dialog,
input, skeleton, spinner, tooltip). EmptyState component exists at
web/src/components/EmptyState.tsx.

Pick ONE page per run from this rotation:
1. web/src/app/chat/page.tsx
2. web/src/app/agents/page.tsx
3. web/src/app/agents/[id]/page.tsx
4. web/src/app/agents/create/page.tsx
5. web/src/app/circles/page.tsx
6. web/src/app/schedules/page.tsx
7. web/src/app/settings/page.tsx
8. web/src/app/insights/page.tsx
9. web/src/app/tools/page.tsx

For the chosen page, check and fix ALL of the following:

1. **Semantic HTML**:
   - Replace <div> wrappers with <main>, <nav>, <section>, <article>, <form> where appropriate
   - Use <button> instead of <div onClick>
   - Use <ul>/<li> for lists of items

2. **ARIA attributes**:
   - Add aria-label to all icon-only buttons (e.g., delete, edit, close buttons)
   - Add aria-live="polite" to regions that update dynamically (message lists, status areas)
   - Add role="status" to loading indicators
   - Add aria-describedby for form fields with help text

3. **Focus management**:
   - Ensure all interactive elements are reachable via Tab key
   - In modals/dialogs: trap focus, restore focus on close
   - After form submission: move focus to the result or error message

4. **Replace alert() with inline feedback**:
   - If any alert() calls exist, replace with the Alert component or a toast
   - Error messages should appear inline near the triggering action

5. **Skeleton loaders**: Where pages show a plain spinner, upgrade to skeleton placeholders
   that match the eventual layout (cards → skeleton cards, lists → skeleton rows).
   Use the existing Skeleton component from web/src/components/ui/skeleton.tsx.

6. Run validation: bun run typecheck && bun run lint && bun run test

Rules:
- Fix ONE page per run — do not touch other pages
- Use existing UI components from web/src/components/ui/
- Match the existing dark theme (sera-* CSS variables in globals.css)
- Maximum 5 files changed per PR
- Do NOT change business logic — accessibility and UX improvements only
```

---

## Weekly — Error Resilience Pass

**Schedule:** Every Thursday

```
Repository: TKCen/sera
Branch: jules/error-resilience

Read AGENTS.md in the project root for coding conventions.
Read web/CLAUDE.md for frontend conventions.

Task: Add error boundaries and improve error resilience in the web dashboard.

Context:
The app has no React Error Boundaries. Unhandled errors crash the entire page.
Some pages use alert() for errors instead of inline feedback. The chat page has
a loading state during test chat that could fail silently. No toast notification
system exists yet.

Steps:

1. **Add a global Error Boundary** (if not done in a previous run):
   Create web/src/components/ErrorBoundary.tsx:
   - Catch React rendering errors
   - Show a fallback UI: "Something went wrong" + error message + "Reload" button
   - Log the error to console.error with component stack

   Wrap the main layout in web/src/app/layout.tsx with the ErrorBoundary.

2. **Add page-level error boundaries**:
   For each page that fetches data, wrap the main content in an ErrorBoundary
   with a page-specific fallback message.
   Pick ONE page per run from the rotation:
   - web/src/app/chat/page.tsx (priority — most complex)
   - web/src/app/agents/page.tsx
   - web/src/app/settings/page.tsx
   - web/src/app/insights/page.tsx (memory graph can crash on bad data)

3. **Add a toast notification system** (if not done in a previous run):
   - Install sonner (< 5KB gzipped): bun add sonner
   - Add <Toaster /> to web/src/app/layout.tsx
   - Create a thin wrapper: web/src/lib/toast.ts that exports toast.success(),
     toast.error(), etc.

4. **Replace alert() calls with toasts**:
   Search for: alert( in web/src/
   Replace each with the appropriate toast call.

5. **Add retry buttons to failed fetches**:
   For TanStack Query hooks, ensure the error state includes a "Retry" button
   that calls refetch().

6. Run validation: bun run typecheck && bun run lint && bun run test

Rules:
- ONE page per run (steps 2-4), but the global ErrorBoundary and toast setup (steps 1, 3)
  should be done in the first run
- Use sonner for toasts — do not build a custom toast system
- Maximum 6 files changed per PR
- Do NOT change business logic
```

---

## Bi-weekly — Small Feature Enhancements

**Schedule:** 1st and 3rd Wednesday

```
Repository: TKCen/sera
Branch: jules/feature-enhancement

Read AGENTS.md in the project root for coding conventions.

Task: Pick ONE small feature enhancement from this backlog and implement it.
Cycle through items across runs. Skip items that are already implemented.

Enhancement backlog (pick the first unimplemented one):

1. **Confirmation dialog for destructive actions**
   Files: any page with delete buttons
   The dialog component exists at web/src/components/ui/dialog.tsx.
   Add a confirmation dialog before:
   - Deleting an agent
   - Removing a circle member
   - Deleting a schedule
   Pattern: "Are you sure you want to delete {name}? This cannot be undone."

2. **Search/filter on agents list page**
   File: web/src/app/agents/page.tsx
   Add a search input that filters agents by name, role, or template.
   Client-side filtering is fine — the list is small.
   Use the existing Input component from web/src/components/ui/input.tsx.

3. **Copy-to-clipboard for agent IDs and config values**
   Files: agent detail page (web/src/app/agents/[id]/page.tsx), settings page
   Add a copy button next to IDs, API keys, and config values.
   Use navigator.clipboard.writeText(). Show a brief "Copied!" tooltip
   using the existing Tooltip from web/src/components/ui/tooltip.tsx.

4. **Pagination for list pages**
   Files: agents page, schedules page, circles page
   Currently all data is fetched at once. Add client-side pagination:
   - Show 20 items per page
   - Add Previous/Next buttons at the bottom
   - Show "Page X of Y" indicator
   This is a UI-only change — the API already returns all results.

5. **Keyboard shortcut: Cmd/Ctrl+K for global search**
   File: web/src/app/layout.tsx or web/src/components/AppShell.tsx
   Add a command palette dialog triggered by Cmd+K / Ctrl+K.
   Use the existing Dialog component. Search agents by name + navigate to detail.
   Also search pages (Settings, Circles, Schedules) by name.

6. **Agent detail: memory tab empty state**
   File: web/src/app/agents/[id]/page.tsx
   The agent detail page has loading/error states but the memory tab shows nothing
   when the agent has no memories. Add the EmptyState component
   (web/src/components/EmptyState.tsx) with a message and optional "Create memory" action.

7. **Breadcrumb navigation**
   Files: agent detail, agent edit, circle detail, circle edit, memory detail
   Add breadcrumb navigation at the top of nested pages:
   Agents > Agent Name > Edit
   Use semantic <nav aria-label="breadcrumb"> + <ol> markup.

For the chosen enhancement:
1. Implement the feature
2. Add it to the appropriate page(s)
3. Run validation: bun run typecheck && bun run lint && bun run test

Rules:
- ONE enhancement per PR
- Use existing UI components where possible
- If adding a new dependency, it must be < 20KB gzipped
- Maximum 8 files changed per PR
- Include a brief description of what was added in the PR body
```

---

## Bi-weekly — Component Extraction (Web)

**Schedule:** 2nd and 4th Wednesday

```
Repository: TKCen/sera
Branch: jules/component-extraction

Read AGENTS.md in the project root for coding conventions.
Read web/CLAUDE.md for frontend conventions.

Task: Extract reusable components from oversized page files.

The following pages are too large and contain inline components that should be extracted:

| File | Lines | Extract candidates |
|---|---|---|
| web/src/app/chat/page.tsx | ~856 | MessageList, ThoughtPanel, SessionSidebar, ChatInputBar (has 7+ useState calls) |
| web/src/app/agents/[id]/edit/page.tsx | ~781 | IdentitySection, ModelSection, ToolsSection, ResourcesSection |
| web/src/components/AgentForm.tsx | ~624 | ManifestEditor, YamlToggle, ValidationPanel |
| web/src/app/agents/[id]/page.tsx | ~558 | OverviewTab, ToolsTab, IntercomTab, MemoryTab |
| web/src/app/settings/page.tsx | ~522 | ProviderCard, ModelsTable, GeneralSettingsTab |
| web/src/app/agents/create/page.tsx | ~477 | TemplateSelector, CreateAgentForm, YamlPreview |

Pick ONE page per run. For that page:

1. Identify self-contained UI sections (50+ lines) that could be separate components.

2. For each extraction:
   a. Create a new file in web/src/components/ with a descriptive name
      (e.g., ChatMessageBubble.tsx, AgentIdentityForm.tsx)
   b. Move the JSX and its associated state/handlers to the new component
   c. Define a clear props interface (typed, no `any`)
   d. Import and use the new component in the original page
   e. The page should read like an outline after extraction

3. Keep state ownership correct:
   - Lift state up only if multiple children need it
   - Pass callbacks for mutations (onSave, onDelete, onChange)
   - Do NOT use context for component-local state

4. Run validation: bun run typecheck && bun run lint && bun run test

Rules:
- ONE page per run
- Extract 2-4 components per run (don't over-split)
- Each component should be independently understandable
- Do NOT change any behavior — pure refactor, identical UI
- Maximum 6 files changed per PR (1 page + extracted components)
```

---

## Bi-weekly — Service Layer Modularization (Core)

**Schedule:** 2nd and 4th Friday

```
Repository: TKCen/sera
Branch: jules/core-modularization

Read AGENTS.md in the project root for coding conventions.
Read core/CLAUDE.md for backend conventions.

Task: Modularize oversized files in core/src/ by extracting focused services
and eliminating cross-cutting code duplication.

Context:
The route layer is already thin and well-separated from services. index.ts is a clean
bootstrap file. No circular dependencies exist. The real opportunities are:
- Oversized service classes with multiple responsibilities
- Cross-cutting patterns duplicated dozens of times

**Priority 1 — Cross-cutting extractions (do these first, one per run):**

a. **apiErrorHandler middleware** — 291 try-catch blocks across 37 route files all
   do the same thing: `try { ... } catch (err) { res.status(500).json({ error }) }`.
   Create core/src/middleware/asyncHandler.ts that wraps route handlers:
   ```typescript
   const asyncHandler = (fn: RequestHandler) => (req, res, next) =>
     Promise.resolve(fn(req, res, next)).catch(next);
   ```
   Add an Express error handler in index.ts. Then convert route handlers to use it.
   Do ONE route file per run (start with the largest).

b. **PlatformPath utility** — Windows path normalization is copy-pasted 3+ times
   in SandboxManager.ts and elsewhere: `/^[a-zA-Z]:/.test()` → lowercase + backslash fix.
   Create core/src/lib/PlatformPath.ts and replace all duplicates.

c. **Singleton base pattern** — 12+ services use the same static getInstance() +
   lazy init + private constructor pattern. Create an abstract Singleton<T> base or
   a factory helper to eliminate the boilerplate.

**Priority 2 — Service splits (after cross-cutting is done):**

| File | Lines | Extract into |
|---|---|---|
| core/src/agents/Orchestrator.ts | ~730 | HeartbeatService (~100 lines: checkStaleInstances, interval mgmt), CleanupService (~80 lines: runCleanupJob, teardown), DiskQuotaService (~60 lines: runDiskQuotaCheck), LifecyclePublisher (~50 lines: publishLifecycleEvent to Centrifugo). Core keeps: startInstance, stopInstance, agent registration. |
| core/src/sandbox/SandboxManager.ts | ~500 | BindMountBuilder (~60 lines: mount logic), ContainerSecurityMapper (~80 lines: capability/tier → Docker labels/CapAdd). Core keeps: Docker API calls, container lifecycle. |
| core/src/llm/LlmRouter.ts | ~524 | OpenAITranslator (~100 lines: ChatCompletionRequest → pi-mono), StreamingAdapter (~100 lines: SSE piping). Core keeps: provider dispatch, error handling. |
| core/src/routes/llmProxy.ts | ~300 | Extract request translation into the LLM translators above. Route should only parse request + call LlmRouter + pipe response. |

Pick ONE extraction per run. For that extraction:

1. Read the file and identify the methods to extract (listed above).
2. Create a new file in the same directory with a focused name.
3. Move methods + private helpers. Keep the original as the public interface.
4. Update all imports across the codebase (grep for the moved exports).
5. Run validation: bun run typecheck && bun run lint && bun run test

Rules:
- ONE extraction per run
- Do NOT change behavior — pure structural refactor
- Do NOT rename public APIs — only move internal implementation
- If existing tests reference internal methods that moved, update the imports
- Maximum 8 files changed per PR
```

---

## Monthly — Legacy Page Cleanup

**Schedule:** 8th of the month

```
Repository: TKCen/sera
Branch: jules/legacy-cleanup

Read AGENTS.md in the project root for coding conventions.
Read web/CLAUDE.md for frontend conventions.

Task: Clean up the legacy pages/ directory and consolidate into the app/ directory.

Context:
The web app has BOTH web/src/pages/*.tsx (legacy React Router style) and
web/src/app/*/page.tsx (Next.js App Router). Many pages exist in both locations.
The app/ directory is the canonical location — pages/ is legacy.

Steps:
1. List all files in web/src/pages/:
   ls web/src/pages/

2. For each page in pages/, check if an equivalent exists in app/:
   - If YES: compare the two. If app/ is more complete, the pages/ version can be removed.
     If pages/ has features that app/ is missing, port them to app/ first.
   - If NO: migrate the page to app/ (create the proper directory + page.tsx structure).

3. Pick up to 3 pages per run to migrate or remove.

4. After removing/migrating:
   - Update any imports that referenced the old pages/ files
   - Check web/src/app/layout.tsx and any router config for stale references
   - Grep for the old file name across web/src/ to catch missed references

5. Run validation: bun run typecheck && bun run lint && bun run test

Rules:
- Maximum 3 pages per run
- When migrating, keep the app/ conventions (page.tsx in a directory, not standalone files)
- When porting features from pages/ → app/, do NOT change the feature — just move it
- If a pages/ file is imported by other components, update those imports
- Maximum 10 files changed per PR
- Do NOT delete pages/ directory until ALL pages are migrated (a later run will handle that)
```

---

## Setting up in Jules

For each task above:

1. Go to [jules.google](https://jules.google) → Scheduled Tasks
2. Create a new task with:
   - **Repository:** TKCen/sera
   - **Prompt:** The code block content for that task
   - **Schedule:** As indicated (weekly/bi-weekly/monthly)
3. Jules will create a PR for each run
4. Review PRs using the `integrate-agent-pr` workflow in `.agents/workflows/`

### PR review guidelines for recurring tasks

**Code quality (quick review):**
| Task | Review depth |
|---|---|
| Type safety sweep | Quick — changes are type-only |
| Dead code cleanup | Medium — verify nothing important was removed |
| Test coverage | Medium — check test quality, not just existence |
| Dependency audit | Quick — check for major version bumps |
| TODO/FIXME sweep | Medium — verify fixes are correct |
| API docs sync | Quick — docs only |
| Console.log cleanup | Quick — logging changes only |

**UX, features & refactoring (careful review):**
| Task | Review depth |
|---|---|
| Accessibility & UX polish | Medium — verify ARIA, semantic HTML, skeleton loaders |
| Error resilience | Medium — verify error boundaries catch properly, toasts fire |
| Feature enhancements | Careful — new functionality, check for regressions |
| Component extraction | Medium — verify no behavior change, clean prop interfaces |
| Service modularization | Careful — verify no behavior change, imports updated everywhere |
| Legacy page cleanup | Medium — verify no features lost in migration |
