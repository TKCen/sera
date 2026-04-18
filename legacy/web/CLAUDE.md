# sera-web

Operator dashboard for SERA — a pure client-side SPA.

## Tech stack

| Concern      | Choice                               |
| ------------ | ------------------------------------ |
| Build tool   | Vite v6                              |
| Routing      | React Router v7                      |
| UI framework | React 19                             |
| Server state | TanStack Query v5                    |
| Components   | shadcn/ui + Radix UI                 |
| Styling      | Tailwind CSS v4 (Aurora Cyber theme) |
| Real-time    | Centrifugo JS client (WebSocket)     |
| Auth flow    | OIDC PKCE (Epic 16 Story 16.5)       |

## Epic references

Load the relevant epic before implementing a feature area:

| Area                                                   | Epic                                                         |
| ------------------------------------------------------ | ------------------------------------------------------------ |
| Framework setup, API client, Centrifugo hooks, routing | `docs/epics/12-sera-web-foundation.md`                       |
| Agent list, chat, thought visualisation, memory graph  | `docs/epics/13-sera-web-agent-ux.md`                         |
| Dashboards, audit log, provider management, health     | `docs/epics/14-sera-web-observability.md`                    |
| Auth login flow, role-gated UI                         | `docs/epics/16-authentication-and-secrets.md` (Stories 16.5) |

## API communication

- **REST**: generate typed client from `docs/openapi.yaml` — do not write types by hand
- **Real-time**: Centrifugo WebSocket subscription — see `docs/ARCHITECTURE.md` → Real-Time Messaging for channel names, subscription token flow, and message shapes

## Binary paths

```bash
# From the workspace root — bun workspaces routes web commands correctly
bun run --filter sera-web dev
bun run --filter sera-web build
```

Or directly from the `web/` directory using `bun run`.

## Docker

- **`.dockerignore` is critical**: Without it, the build context includes `node_modules/` (~180 MB). The `.dockerignore` excludes `node_modules`, `.env`, `.git`.
- **Dockerfile uses `oven/bun`**: The builder stage uses `oven/bun:1-alpine` for fast installs. The production runner is nginx serving static files.

## Running tests

Tests must be run from the `web/` directory (not workspace root) so that Vite's `@` alias resolves:

```bash
bunx vitest run src/__tests__/
```

Type-check:

```bash
bunx tsc --noEmit -p tsconfig.json
```

## Centrifugo channel names (Epic 13 operator channels)

The public operator-facing channels differ from the internal ones in some hooks:

| Purpose                | Channel                  |
| ---------------------- | ------------------------ |
| Agent status           | `agent:{agentId}:status` |
| Token streaming (chat) | `tokens:{agentId}`       |
| Thought stream         | `thoughts:{agentId}`     |

The internal hooks (`useThoughtStream`, `useTokenStream`) use `internal:…` prefixed channels for a different purpose — do not confuse them with the above.

## API type shapes — common gotchas

- **`useCircles()` returns `CircleSummary[]`**: flat `{ name, displayName, memberCount }` — not `CircleManifest` which has a `metadata` wrapper. Access `c.name`, not `c.metadata.name`.
- **Button variants**: the Button component uses `danger` for the destructive style, not `destructive`.

## Learnings

- **Next.js fully removed**: Migration to Vite + React Router v7 is complete. The `src/app/` directory and `next.config.ts` have been deleted. All routing is in `src/main.tsx`.
- **OIDC token storage architecture (Story 16.5)**: The OIDC access token must never reach client storage. The web stores only an opaque `sess_*` session token (from `WebSessionStore`) in `sessionStorage`. `AuthContext` exposes `setSessionAndUser(token, user)` for the callback page to call after `POST /api/auth/oidc/callback` returns `{ sessionToken, user }`. All API calls use `Bearer <sessionToken>` — the OIDC token stays server-side only.
- **`scrollIntoView` not available in jsdom**: Guard with `if (el?.scrollIntoView)` before calling it, or the call will throw in tests.
- **`vi.mock` factory is hoisted — no top-level variables**: Any variable referenced inside a `vi.mock(...)` factory must be defined with `vi.hoisted()` or inlined directly; referencing a `const` defined in the same file causes a ReferenceError at runtime.
- **TanStack Query mutation → invalidation wipes local chat state**: When `createAgentTask` succeeds, invalidating the tasks query causes a refetch that overwrites in-flight session messages. Pattern: use a `historyInitializedForAgent` ref so history only seeds messages once per agent selection; reset the ref on agent switch. See `ChatPage.tsx`.
- **`react-force-graph-2d` requires Suspense**: Wrap the `MemoryGraph` component in `<Suspense>` when lazy-loaded; the canvas initialisation throws during server-side-style rendering without it.
- **`vi.restoreAllMocks()` destroys `vi.fn()` implementations**: In Vitest, `vi.restoreAllMocks()` calls `mockRestore()` on all mocks — including those declared in `vi.mock()` factories — which removes their `mockResolvedValue` implementations and causes subsequent tests to receive `undefined`. Use `vi.clearAllMocks()` in `afterEach` instead to reset call counts without destroying implementations.
- **Recharts is not bundled by default**: Recharts must be added to `package.json` dependencies and `bun add` run in Docker before Vite can build. The shadcn/ui ecosystem references it but doesn't install it automatically.
- **Vite HMR does not fire inside Docker on Windows**: Inotify events from host file-system changes are not propagated through Docker Desktop volume mounts to the container's Vite process. `touch` inside the container also doesn't help. The only reliable way to pick up source changes is `docker restart sera-web` (Vite re-reads all files on startup).
- **Absolute overlay links swallow button clicks**: A pattern like `<Link className="absolute inset-0" />` at the end of a card makes the whole card clickable but sits above sibling elements in the z-stack. Any interactive element inside the card (buttons, links) must be inside a wrapper with `relative z-10` to escape the overlay's stacking context, otherwise clicks pass through to the overlay link instead.
- **web/bun.lock must match standalone Docker context**: The Dockerfile builds with `context: ./web`, not the workspace root. The lockfile must be regenerated with `MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd)/web:/app" -w /app oven/bun:1-alpine bun install` from the repo root. A workspace-generated lockfile resolves fewer packages and causes `--frozen-lockfile` to fail in Docker.
- **Never bind-mount host web/ into Docker for `bun install`**: This replaces platform-specific binaries (esbuild) with Linux versions, breaking the host. If contaminated: `rm -rf node_modules && bun install`.
- **sera-web healthcheck reports unhealthy but UI works**: The `wget` command in the healthcheck can't connect to `localhost` inside the container, even though Vite is listening on `0.0.0.0:5173`. `node -e "fetch(...)"` works. See #364.
