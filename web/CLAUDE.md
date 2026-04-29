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
