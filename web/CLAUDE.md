# web/ — SERA Operator Console

Thin React SPA against the rust gateway. Fresh build, NOT a port of `legacy/web/`.

## Stack

- React 19 + Vite 6 + TypeScript (strict)
- Tailwind v4 (PostCSS plugin, `@import 'tailwindcss'`)
- TanStack Query 5 for server state
- react-router 7 (data router)
- `@microsoft/fetch-event-source` for AG-UI SSE (supports `Authorization` header — `EventSource` does not)
- shadcn/ui-style primitives via Radix; lucide-react icons; sonner toasts
- Vitest + jsdom for unit tests; Playwright at top-level `e2e/` for E2E

## Commands

All from `web/`:

```bash
bun install
bun run dev          # vite on :5173, proxies /api → :3001
bun run build        # tsc -b && vite build
bun run typecheck
bun run lint
bun run test
```

## Run locally (the two paths)

**Dev server (HMR, fastest iteration):**

```bash
cd web
bun install        # first time
bun run dev        # http://localhost:5173 — proxies /api → http://localhost:3001
```

The gateway must be running separately (e.g. `cargo run -p sera-gateway --bin sera` from `rust/`, or `docker compose -f docker-compose.rust.yaml up sera-gateway`).

**Full stack via docker compose:**

```bash
docker compose -f docker-compose.rust.yaml up --build
```

Brings up postgres + centrifugo + sera-gateway + sera-web. Browse to <http://localhost:5173> for the operator console; the gateway listens on `:3001`.

Use the dev-default API key `sera_bootstrap_dev_123` (or whatever `SERA_BOOTSTRAP_API_KEY` is set to in your env) to sign in.

## Docker

- **Multistage build:** `oven/bun:1-alpine` runs `bun install --frozen-lockfile && bun run build`, then nginx serves the static `dist/`.
- **`bun.lock` is committed** (root `.gitignore` was unstuck during W.1) and must stay in sync with `package.json` so `--frozen-lockfile` doesn't fail in CI.
- **`.dockerignore` is critical** — without it the build context includes `node_modules/` (~hundreds of MB) and the build is slow.
- **nginx proxies `/api`** to `sera-gateway:3001` on the compose network. The `/api/chat` location turns `proxy_buffering off` and bumps `proxy_read_timeout` to 1h so SSE frames flow.
- **SPA routing** — every unknown path falls through to `index.html` so client-side routing works on hard refresh.

## Auth (v1)

API-key bearer token in `localStorage` under `sera.auth.token`. The gateway dev key is `sera_bootstrap_dev_123`. `apiFetch` (`src/lib/api.ts`) attaches the header on every request. OIDC device-flow is deferred.

## Gateway endpoints in scope (v1)

`/api/health`, `/api/auth/me`, `/api/chat`, `/api/agents`, `/api/agents/{id}`, `/api/sessions`, `/api/sessions/{id}/transcript`, `/api/hooks`. Everything else (memory, audit, schedules, admin) is a Sprint 2+ concern.

## Conventions

- Pages live under `src/views/`. Each page is a single component named `<Name>View`.
- Hooks for query/mutation logic under `src/hooks/use<Name>.ts`.
- Shared primitives under `src/components/`. Layout shell + nav in `Layout.tsx`.
- API client is `apiFetch<T>` only — never raw `fetch` in views.
- No Centrifugo dependency until the gateway exposes it as a public surface.

## Out of scope

Anything in `legacy/web/` that needs an endpoint not in the list above. File a bead, don't 404-stub.
