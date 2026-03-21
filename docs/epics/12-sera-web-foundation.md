# Epic 12: sera-web Foundation

## Overview

The operator dashboard needs a solid technical foundation before feature work begins. This epic establishes the frontend architecture: framework and build setup, a typed API client layer, real-time Centrifugo subscription hooks, a consistent component system, and the Aurora Cyber design language. Everything built in Epics 13 and 14 stands on this foundation.

## Context

- See `docs/ARCHITECTURE.md` → sera-web (Tech Stack section)
- sera-web is a pure client-side SPA — no SSR, no server-side data fetching
- All data comes from sera-core REST API (TanStack Query) or Centrifugo WebSocket (real-time)
- The browser connects directly to Centrifugo for WebSocket — sera-core only issues tokens
- Current stack: Next.js 16 + Tailwind v4. Migration to Vite + React Router is recommended but optional
- Design system: "Aurora Cyber" — deep blacks, cyan/green accents, glassmorphism, high contrast

## Dependencies

- Epic 04 (LLM Proxy) — `/api/providers` endpoint
- Epic 09 (Real-Time Messaging) — Centrifugo token issuance endpoint

---

## Stories

### Story 12.1: Frontend framework and build setup

**As a** developer
**I want** a clean, fast frontend build setup
**So that** local development iteration is fast and production builds are small and optimised

**Acceptance Criteria:**
- [ ] **Option A (migration):** Vite + React Router v6 + TypeScript replacing Next.js. Build output: static files served by nginx:alpine. Docker image target < 50MB.
- [ ] **Option B (keep):** Next.js with `output: 'export'` (static export, no Node.js runtime in production). Eliminates the Node.js standalone image.
- [ ] Either option: HMR works in dev, `bun run build` produces a deployable artifact
- [ ] `tsconfig.json` strict mode, path alias `@/*` → `src/*`
- [ ] ESLint + Prettier configured and run in CI
- [ ] `docker-compose.dev.yaml` mounts source and runs dev server with HMR

**Technical Notes:**
- If migrating to Vite: `vite.config.ts` `server.proxy` handles `/api/*` → `http://sera-core:3001` for dev
- Production: nginx config proxies `/api/*` to sera-core, serves static files for everything else
- The Next.js API routes (`/api/health`, `/api/core/[...path]`) become: a nginx proxy rule (for `/api/core`) and a static health endpoint

---

### Story 12.2: Typed API client layer

**As a** developer
**I want** a typed HTTP client for sera-core's API
**So that** all API calls are type-safe, consistent, and easy to mock in tests

**Acceptance Criteria:**
- [ ] `src/lib/api/` contains typed clients grouped by domain: `agents`, `circles`, `memory`, `providers`, `schedules`, `audit`, `metering`
- [ ] Each client function returns a typed response — no `any` types
- [ ] Shared base client handles: base URL (`VITE_API_URL` or `NEXT_PUBLIC_API_URL`), auth headers, error parsing
- [ ] API errors surfaced as typed `APIError` objects with `status`, `message`, `code`
- [ ] 401 responses redirect to a login/setup page (or surface a reconnect prompt for local no-auth mode)
- [ ] Types generated from the OpenAPI spec (`docs/openapi.yaml`) or maintained as hand-written TypeScript — one canonical source

---

### Story 12.3: TanStack Query integration

**As a** developer
**I want** TanStack Query (React Query) as the server state management layer
**So that** data fetching, caching, loading states, and refetching are handled consistently across all pages

**Acceptance Criteria:**
- [ ] `QueryClient` configured at app root with sensible defaults: `staleTime: 30s`, `retry: 2`, `refetchOnWindowFocus: true`
- [ ] All API calls wrapped in `useQuery` / `useMutation` hooks — no bare `useEffect + fetch` patterns
- [ ] Custom hooks per domain: `useAgents()`, `useAgent(id)`, `useCircles()`, `useProviders()`, `useSchedules()`, etc.
- [ ] Mutations invalidate relevant queries on success (e.g. creating an agent invalidates the agents list query)
- [ ] Global error boundary displays API errors in a consistent toast/banner pattern
- [ ] Loading states handled by each hook — consumers receive `{ data, isLoading, error }` without boilerplate

---

### Story 12.4: Centrifugo subscription hooks

**As a** developer
**I want** React hooks for subscribing to Centrifugo channels
**So that** real-time updates integrate naturally into React components without manual WebSocket management

**Acceptance Criteria:**
- [ ] `useCentrifugo()` hook provides connection state (`connecting | connected | disconnected | error`)
- [ ] `useChannel(channelName)` hook subscribes to a channel and returns the latest message
- [ ] `useThoughtStream(agentId)` hook returns an array of accumulated thought events for an agent
- [ ] `useTokenStream(agentId)` hook returns accumulated token string for streaming LLM output
- [ ] `useAgentStatus(agentId)` hook returns current agent status, auto-updated via `agent:{agentId}:status` channel
- [ ] Hooks auto-reconnect on disconnect (exponential backoff, max 30s)
- [ ] Centrifugo connection token fetched from `GET /api/centrifugo/config` on init and refreshed before expiry
- [ ] Subscriptions cleaned up on component unmount — no memory leaks

**Technical Notes:**
- Use the `centrifuge` npm package (already in dependencies) for the WebSocket client
- A single shared Centrifugo client instance, managed via React Context

---

### Story 12.5: Design system and component library

**As a** developer
**I want** a consistent component library implementing the Aurora Cyber design language
**So that** all pages share a visual identity and new features can be built without designing from scratch

**Acceptance Criteria:**
- [ ] Design tokens defined in `globals.css`: primary palette (AIU Cyan `#00E5FF`, AIU Green `#00FF00`), surface (Aurora Black `#020402`), glass surface, text hierarchy
- [ ] Base components implemented: `Button` (primary/secondary/ghost/danger variants), `Card` (glassmorphism surface), `Badge` (status colours), `Spinner`, `Skeleton` (loading state), `Toast` / `Notification`
- [ ] Navigation: `Sidebar` with section grouping and active state
- [ ] Data display: `Table` (sortable columns, loading skeleton), `StatCard` (metric + trend), `EmptyState`
- [ ] Feedback: `Alert` (info/warning/error/success), `Dialog`/`Modal`, `Tooltip`
- [ ] All components: TypeScript props, accessible (ARIA labels, keyboard navigation), responsive
- [ ] shadcn/ui used as the base layer where applicable — customised to Aurora Cyber theme
- [ ] Storybook or equivalent component catalogue (optional but recommended)

---

### Story 12.6: Application shell and routing

**As a** user
**I want** a consistent app shell with navigation and the correct routes for all features
**So that** I can navigate between sections without confusion

**Acceptance Criteria:**
- [ ] App shell: fixed sidebar navigation + main content area
- [ ] Routes defined: `/chat`, `/agents`, `/agents/new`, `/agents/:id`, `/agents/:id/edit`, `/circles`, `/circles/:id`, `/insights`, `/schedules`, `/settings`, `/tools`, `/memory/:id`
- [ ] Active route highlighted in sidebar
- [ ] 404 page for unknown routes
- [ ] Browser tab title updates per route
- [ ] Sidebar collapses to icon-only on narrow viewports
- [ ] No route requires a full page load — all navigation is client-side
