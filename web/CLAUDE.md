# sera-web

Operator dashboard for SERA. **Check `docs/ARCHITECTURE.md` → Tech Stack for the current migration status before writing any code** — the framework target has changed from Next.js.

## Current vs target state

| Concern | Current | Target |
|---|---|---|
| Framework | Next.js | Vite + React Router v7 |
| Server state | — | TanStack Query |
| Components | — | shadcn/ui + Radix UI + Tailwind v4 |
| Auth flow | — | OIDC PKCE (Epic 16 Story 16.5) |

Do not add new Next.js-specific features (SSR, server components, API routes) — the direction is a plain SPA. The `.next/` build cache can be ignored.

## Epic references

Load the relevant epic before implementing a feature area:

| Area | Epic |
|---|---|
| Framework setup, API client, Centrifugo hooks, routing | `docs/epics/12-sera-web-foundation.md` |
| Agent list, chat, thought visualisation, memory graph | `docs/epics/13-sera-web-agent-ux.md` |
| Dashboards, audit log, provider management, health | `docs/epics/14-sera-web-observability.md` |
| Auth login flow, role-gated UI | `docs/epics/16-authentication-and-secrets.md` (Stories 16.5) |

## API communication

- **REST**: generate typed client from `docs/openapi.yaml` — do not write types by hand
- **Real-time**: Centrifugo WebSocket subscription — see `docs/ARCHITECTURE.md` → Real-Time Messaging for channel names, subscription token flow, and message shapes

## Binary paths

```bash
# From the workspace root — npm workspaces routes web commands correctly
npm run dev --workspace=web
npm run build --workspace=web
```

Or directly from the `web/` directory using the local node_modules if needed.

## Docker

- **`.dockerignore` is critical**: Without it, the build context includes `node_modules/` and `.next/` (~180 MB). The `.dockerignore` excludes `node_modules`, `.next`, `.env`, `.git`.
- **Standalone output**: `next.config.ts` sets `output: 'standalone'` — the production Dockerfile copies from `.next/standalone` and `.next/static`.

## Running tests

Tests must be run from the `web/` directory (not workspace root) so that Vite's `@` alias resolves:

```bash
node node_modules/vitest/vitest.mjs run src/__tests__/
```

Type-check:

```bash
node node_modules/typescript/bin/tsc --noEmit -p tsconfig.json
```

## Centrifugo channel names (Epic 13 operator channels)

The public operator-facing channels differ from the internal ones in some hooks:

| Purpose | Channel |
|---|---|
| Agent status | `agent:{agentId}:status` |
| Token streaming (chat) | `tokens:{agentId}` |
| Thought stream | `thoughts:{agentId}` |

The internal hooks (`useThoughtStream`, `useTokenStream`) use `internal:…` prefixed channels for a different purpose — do not confuse them with the above.

## API type shapes — common gotchas

- **`useCircles()` returns `CircleSummary[]`**: flat `{ name, displayName, memberCount }` — not `CircleManifest` which has a `metadata` wrapper. Access `c.name`, not `c.metadata.name`.
- **Button variants**: the Button component uses `danger` for the destructive style, not `destructive`.

## Learnings

- **Multiple lockfiles warning**: Next.js 16 warns about detecting both root and `web/package-lock.json`. Can be silenced with `turbopack.root` in `next.config.ts` if needed — currently harmless.
- **OIDC token storage architecture (Story 16.5)**: The OIDC access token must never reach client storage. The web stores only an opaque `sess_*` session token (from `WebSessionStore`) in `sessionStorage`. `AuthContext` exposes `setSessionAndUser(token, user)` for the callback page to call after `POST /api/auth/oidc/callback` returns `{ sessionToken, user }`. All API calls use `Bearer <sessionToken>` — the OIDC token stays server-side only.
- **`scrollIntoView` not available in jsdom**: Guard with `if (el?.scrollIntoView)` before calling it, or the call will throw in tests.
- **`vi.mock` factory is hoisted — no top-level variables**: Any variable referenced inside a `vi.mock(...)` factory must be defined with `vi.hoisted()` or inlined directly; referencing a `const` defined in the same file causes a ReferenceError at runtime.
- **TanStack Query mutation → invalidation wipes local chat state**: When `createAgentTask` succeeds, invalidating the tasks query causes a refetch that overwrites in-flight session messages. Pattern: use a `historyInitializedForAgent` ref so history only seeds messages once per agent selection; reset the ref on agent switch. See `ChatPage.tsx`.
- **`react-force-graph-2d` requires Suspense**: Wrap the `MemoryGraph` component in `<Suspense>` when lazy-loaded; the canvas initialisation throws during server-side-style rendering without it.
- **`vi.restoreAllMocks()` destroys `vi.fn()` implementations**: In Vitest, `vi.restoreAllMocks()` calls `mockRestore()` on all mocks — including those declared in `vi.mock()` factories — which removes their `mockResolvedValue` implementations and causes subsequent tests to receive `undefined`. Use `vi.clearAllMocks()` in `afterEach` instead to reset call counts without destroying implementations.
- **Recharts is not bundled by default**: Recharts must be added to `package.json` dependencies and `npm install` run in Docker before Vite can build. The shadcn/ui ecosystem references it but doesn't install it automatically.
