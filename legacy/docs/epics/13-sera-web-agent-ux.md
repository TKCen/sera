# Epic 13: sera-web Agent UX

## Overview

The primary operator experience: managing agents, conversing with them, and watching them think in real time. This covers the agent management pages (list, detail, create, edit), the chat interface with streaming, and the thought stream visualisation that makes SERA's "real-time symbiosis" tangible. This is the most user-visible part of the system.

## Context

- See `docs/ARCHITECTURE.md` → Real-Time Messaging (thought channels, token channels)
- Agent status arrives via Centrifugo `agent:{agentId}:status` channel
- LLM output streams via `tokens:{agentId}` channel
- Thought steps (observe/plan/act/reflect) stream via `thoughts:{agentId}:{name}` channel
- All data mutations go through TanStack Query mutations, which invalidate relevant caches

## Dependencies

- Epic 12 (sera-web Foundation) — API client, hooks, component library
- Epic 02 (Agent Manifest) — agent registry API
- Epic 03 (Docker Sandbox) — agent lifecycle API
- Epic 09 (Real-Time Messaging) — Centrifugo channels

---

## Stories

### Story 13.1: Agent list page

**As an** operator
**I want** to see all agents with their current status at a glance
**So that** I can quickly assess the health of my agent fleet

**Acceptance Criteria:**
- [ ] `/agents` page lists all registered agents as cards or rows
- [ ] Each entry shows: icon, display name, circle, tier badge, current status (colour-coded: green=running, grey=stopped, red=error, amber=unresponsive)
- [ ] Status updates in real time via `agent:{agentId}:status` Centrifugo channel — no page refresh needed
- [ ] Filter by: circle, status, tier
- [ ] Search by agent name
- [ ] Empty state with clear "Create your first agent" call to action
- [ ] Quick actions per agent: Start, Stop, View (without navigating away from list)
- [ ] "Create Agent" button navigates to `/agents/new`

---

### Story 13.2: Agent detail page

**As an** operator
**I want** a detailed view of a single agent including its current state, configuration, and resource usage
**So that** I can understand what the agent is doing and how it's configured

**Acceptance Criteria:**
- [ ] `/agents/:id` shows: agent identity header (name, icon, circle, tier, status), manifest summary, active container info (if running), resource usage (current tokens used vs budget), active subagents, current worktree (if any)
- [ ] "Manifest" tab: read-only formatted view of the AGENT.yaml content
- [ ] "Logs" tab: live container log stream via `GET /api/agents/:id/logs?follow=true` — auto-scrolls, pauseable
- [ ] "Memory" tab: lists agent's memory blocks with type, tags, timestamp — links to `/memory/:id`
- [ ] "Schedules" tab: lists agent's schedules with next run time and last run status
- [ ] Start / Stop / Restart buttons with confirmation dialog for Stop/Restart
- [ ] Status indicator in the page header updates in real time

---

### Story 13.3: Create and edit agent forms

**As an** operator
**I want** a form to create and edit agents visually
**So that** I don't have to write AGENT.yaml by hand for common configurations

**Acceptance Criteria:**
- [ ] `/agents/new` and `/agents/:id/edit` share a form component
- [ ] Form fields: display name, icon (emoji picker), circle (dropdown from circles API), tier (1/2/3 with description), role (textarea), description (textarea), model provider (dropdown), model name (text, validated against providers API), temperature (slider 0–1)
- [ ] Tools section: multi-select of available tools (fetched from `/api/tools`), with allowed/denied toggle per tool
- [ ] Skills section: multi-select of available skills (fetched from `/api/skills`)
- [ ] Resources section: token budget fields (hourly/daily), CPU/memory inputs
- [ ] "Advanced" section: raw YAML editor that syncs with the form fields bidirectionally
- [ ] Validation: required fields marked, invalid values highlighted before submit
- [ ] On submit: `POST /api/agents` or `PUT /api/agents/:id` → success navigates to agent detail page
- [ ] "Validate Manifest" button sends manifest to validation endpoint before save

---

### Story 13.4: Chat interface

**As an** operator
**I want** to converse directly with an agent in a chat UI
**So that** I can interact with agents naturally, give them tasks, and see their responses

**Acceptance Criteria:**
- [ ] `/chat` (or `/agents/:id/chat`) shows a conversation thread
- [ ] Agent selector at the top to switch between agents
- [ ] Messages rendered with Markdown support (using `react-markdown` with `remark-gfm`)
- [ ] User messages right-aligned, agent responses left-aligned with agent icon
- [ ] Agent responses stream token-by-token via `tokens:{agentId}` Centrifugo channel
- [ ] Streaming indicator (blinking cursor) while response is generating
- [ ] Code blocks in responses: syntax highlighted, copy button
- [ ] Message input: multi-line textarea, Shift+Enter for newline, Enter to send
- [ ] Chat history loaded from `GET /api/chat/sessions/:id/messages`
- [ ] Session list in a side panel for accessing previous conversations
- [ ] "New Chat" button starts a fresh session

---

### Story 13.5: Thought stream visualisation

**As an** operator
**I want** to watch an agent's internal reasoning steps in real time
**So that** I can understand how the agent approached a problem without waiting for the final answer

**Acceptance Criteria:**
- [ ] Thought stream panel available alongside chat or on agent detail page
- [ ] Thought steps displayed as a timeline: each step shows type (observe/plan/act/reflect), content, and relative timestamp
- [ ] Step types visually distinct: different icon and accent colour per type
  - `observe`: cyan — analysing the problem
  - `plan`: green — deciding approach
  - `act`: amber — executing a tool
  - `reflect`: purple — evaluating result
- [ ] `act` steps show the tool name and sanitised arguments
- [ ] Stream auto-scrolls to the latest thought; user can scroll up to review without interrupting the scroll lock
- [ ] Steps accumulate during the agent's run; cleared on next run start
- [ ] Collapsed/expanded toggle — collapsed shows only `act` steps (key events), expanded shows all

---

### Story 13.6: Memory graph visualisation

**As an** operator
**I want** to see a visual graph of an agent's memory blocks and their relationships
**So that** I can understand what the agent knows and how its knowledge is connected

**Acceptance Criteria:**
- [ ] `/insights` or `/agents/:id/memory-graph` renders a force-directed graph of memory blocks
- [ ] Data source: `GET /api/memory/:agentId/graph` — returns `{ nodes, edges }` (see Epic 19 Story 19.4 for the canonical response schema)
- [ ] Nodes: memory blocks, sized by importance, coloured by type (`episodic`/`semantic`/`procedural`/`summary`)
- [ ] Edges: tag-link (blocks sharing a tag) and explicit-ref (blocks with `relatedIds`) — edge type shown as label or line style
- [ ] Node hover: shows block type, tags, truncated content
- [ ] Node click: navigates to `/memory/:id` for full block view
- [ ] Filter by block type, circle scope, date range, tag
- [ ] Graph rendered using `react-force-graph-2d` (already in dependencies)
- [ ] Empty state: clear message when agent has no memory blocks yet

**Technical Notes:**
- **Coordination with Epic 19:** The graph endpoint data model is defined in Story 19.4. If Epic 13 is implemented before Epic 19, build against the same `{ nodes, edges }` shape — both the legacy and scoped endpoints can serve this format. Do not build against the old `requires`/co-occurrence model — it will be removed.

---

### Story 13.7: Permission request approval UI

**As an** operator
**I want** to see pending permission requests from agents and approve or deny them from the dashboard
**So that** the human-in-the-loop flow (Story 3.9) has a proper UI and I don't have to rely on the API alone

**Acceptance Criteria:**
- [ ] Notification badge on the sidebar when pending requests exist (count from `GET /api/permission-requests`)
- [ ] Badge updates in real time via `system.permission-requests` Centrifugo channel
- [ ] `/permissions` page (or modal triggered from sidebar badge) lists all pending requests
- [ ] Each request shows: agent name + icon, requested dimension (`filesystem` / `network` / `exec.commands`), requested value (path, host, or command pattern), reason (if provided), requested at (relative time), timeout countdown
- [ ] Approve button with grant type selector: `one-time` / `session` / `persistent`
- [ ] Deny button with optional reason text
- [ ] Decision calls `POST /api/permission-requests/:requestId/decision`
- [ ] Approved/denied request animates out of the list
- [ ] Toast confirmation: "Granted [dimension] access to [agentName]" or "Denied..."
- [ ] History tab: recent decisions (last 50) for audit reference
- [ ] If the request originated from a chat session, a "View conversation" link opens the relevant chat

**Technical Notes:**
- This is architecturally similar to a notification inbox — pending items arrive via Centrifugo, operator acts, items resolve
- The timeout countdown should be visible so the operator knows urgency (default: 5 min auto-deny)

---

### Story 13.8: Capability grants viewer

**As an** operator
**I want** to see and manage all capability grants for an agent
**So that** I can review what runtime permissions were granted and revoke them if needed

**Acceptance Criteria:**
- [ ] New "Grants" tab on the agent detail page (`/agents/:id`)
- [ ] Lists all active grants: dimension, value, grant type (one-time / session / persistent), granted by, granted at, expires at
- [ ] Session grants shown with a "session" badge — auto-removed on stop
- [ ] Persistent grants show a "Revoke" button → `DELETE /api/agents/:id/grants/:grantId` with confirmation
- [ ] If pending secret rotations exist, show a warning banner with "Restart to apply" action
- [ ] Empty state: "No runtime grants — this agent is running with its base capabilities"

---

### Story 13.9: Circle management UI

**As an** operator
**I want** to create, edit, and manage circles from the dashboard
**So that** I can organise agents into groups without editing YAML files

**Acceptance Criteria:**
- [ ] `/circles` page enhanced: "Create Circle" button opens a creation dialog
- [ ] Creation dialog: name (slug), display name, description, constitution (textarea with markdown preview)
- [ ] `/circles/:id` detail page shows: member agents (cards with status), constitution text, shared knowledge stats, active sessions
- [ ] "Edit" button → inline editing of display name, description, constitution
- [ ] "Add Member" → dropdown of available agents not in this circle → `POST /api/circles/:id/members`
- [ ] "Remove Member" → confirmation → removes agent from circle
- [ ] "Party Mode" button → opens dialog: prompt input, optional round count, start → `POST /api/circles/:id/party` → navigates to party session view
- [ ] Party session view: rounds displayed as a threaded conversation, each agent's contribution as a separate card with agent icon and name, synthesis (if any) highlighted at the end
- [ ] Delete circle with confirmation ("This will remove all members from this circle")

---

### Story 13.10: Secret entry modal

**As an** operator
**I want** a secure modal dialog for entering secrets when an agent requests one
**So that** secret values never flow through the agent's LLM context

**Acceptance Criteria:**
- [ ] When `system.secret-entry-requests` Centrifugo event arrives: modal overlays the current page (non-dismissable without action)
- [ ] Modal shows: requesting agent name + icon, secret name, description, and a password-type `<input>` field
- [ ] "Store" button → `POST /api/secrets` directly from the browser (not via agent/chat) → resolves the agent's pending tool call
- [ ] "Cancel" button → resolves the tool call with `{ stored: false, reason: 'cancelled' }`
- [ ] Input field has a "show/hide" toggle for verification before storing
- [ ] If multiple requests arrive concurrently: queue them as stacked modals (one at a time)
- [ ] After storing: toast confirmation "Secret '{name}' stored successfully"
- [ ] The secret value **never** appears in: any Centrifugo channel, the chat message history, browser localStorage, or URL parameters
- [ ] Keyboard: Enter submits, Escape cancels

**Technical Notes:**
- The modal must be rendered at the app shell level (not inside a chat component) — it can appear during any page
- Listen for the Centrifugo event in a global hook (e.g. `useSecretEntryRequests()` in `main.tsx` or `AppShell.tsx`)

---

### Story 13.11: Delegation management UI

**As an** operator
**I want** to view and manage delegation tokens and service identities for agents
**So that** I can control what external credentials agents can use

**Acceptance Criteria:**

**Delegation tokens (on agent detail page, new "Delegation" tab):**
- [ ] Lists inbound delegations for this agent: credential name, scope, grant type (one-time/session/persistent), granted by, expires at, status (active/expired/revoked)
- [ ] "Issue Delegation" button → dialog: select credential (from `GET /api/secrets` metadata), set scope, set grant type, set expiry → `POST /api/delegation/issue`
- [ ] "Revoke" button per delegation → `DELETE /api/delegation/:id` with cascade option
- [ ] Child delegations shown as expandable tree (agent → subagent chain)

**Service identities (on agent detail page, within "Delegation" tab):**
- [ ] Lists service identities: service name, credential type, created at, last rotated
- [ ] "Create Identity" button → dialog: service name, initial credential → `POST /api/agents/:id/service-identities`
- [ ] "Rotate" button → `POST /api/agents/:id/service-identities/:id/rotate`
- [ ] "Revoke" button → `DELETE /api/agents/:id/service-identities/:id`

---

### Story 13.12: Centrifugo connection indicator

**As an** operator
**I want** to see the real-time WebSocket connection status in the UI header
**So that** I know whether live updates (thoughts, status changes, token streaming) are working

**Acceptance Criteria:**
- [ ] Connection indicator in the sidebar header or app shell toolbar
- [ ] States: `connected` (green dot), `connecting` (amber pulse), `disconnected` (red dot + "Reconnecting..." text)
- [ ] State sourced from `useCentrifugo()` hook — reflects actual WebSocket transport state
- [ ] Click on disconnected indicator → manual reconnect attempt
- [ ] The existing "Core: Online/Offline" health check in the sidebar is separate — this indicator is specifically for the Centrifugo WebSocket
