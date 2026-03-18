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
- [ ] Nodes: memory blocks, sized by importance, coloured by type
- [ ] Edges: `requires` relationships between blocks (if stored), and co-occurrence in the same retrieval context
- [ ] Node hover: shows block type, tags, truncated content
- [ ] Node click: navigates to `/memory/:id` for full block view
- [ ] Filter by block type, circle scope, date range
- [ ] Graph rendered using `react-force-graph-2d` (already in dependencies)
- [ ] Empty state: clear message when agent has no memory blocks yet
