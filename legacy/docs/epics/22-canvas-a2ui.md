# Epic 22: Canvas / Agent-Driven UI (A2UI)

## Overview

Agents can push dynamic, interactive UI components into the sera-web dashboard using the **A2UI** (Agent-to-UI) message format. This enables agents to visualize complex data, present decision trees, render rich previews of artifacts, and collect structured input — going beyond plain chat text. The canvas is a panel in sera-web that renders agent-pushed components in real time.

## Context

- A2UI is a flat, JSON-based component format designed to be LLM-friendly (easy for models to emit)
- Components are pushed via Centrifugo channels — the canvas subscribes to the agent's UI channel
- The format supports incremental updates (append, replace, clear) for streaming UIs
- Reference format: OpenClaw A2UI v0.8 (JSONL with component types: Column, Row, Card, Text, Image, TextField)
- Canvas is an optional overlay — agents work without it, but can produce richer output with it
- Security: A2UI components are sandboxed (no arbitrary JS execution, no external resource loading)

## Dependencies

- Epic 09 (Real-Time Messaging) — Centrifugo for streaming A2UI messages
- Epic 12/13 (Dashboard) — sera-web hosting the canvas panel

---

## Stories

### Story 22.1: A2UI message format and schema

**As** sera-core
**I want** a well-defined A2UI message format
**So that** agents can emit structured UI components and the dashboard can render them safely

**Acceptance Criteria:**
- [ ] A2UI TypeScript types in `core/src/a2ui/types.ts`:
  ```typescript
  interface A2UIMessage {
    action: 'push' | 'replace' | 'clear' | 'snapshot'
    components: A2UIComponent[]
    targetPanel?: string       // default: 'main'
  }

  type A2UIComponent =
    | { type: 'text'; content: string; variant?: 'heading' | 'body' | 'code' | 'caption' }
    | { type: 'image'; src: string; alt?: string }           // src must be data: URI or /api/ path
    | { type: 'card'; title: string; children: A2UIComponent[] }
    | { type: 'row'; children: A2UIComponent[] }
    | { type: 'column'; children: A2UIComponent[] }
    | { type: 'textField'; id: string; label: string; placeholder?: string }
    | { type: 'button'; id: string; label: string; action: string }
    | { type: 'table'; headers: string[]; rows: string[][] }
    | { type: 'progress'; label: string; value: number; max: number }
    | { type: 'divider' }
  ```
- [ ] JSON Schema in `schemas/a2ui-message.json` for validation
- [ ] Zod schema in `core/src/a2ui/schema.ts` for runtime validation

### Story 22.2: Agent canvas tools

**As** an agent
**I want** built-in tools to push UI components to the operator's canvas
**So that** I can present rich visualizations and interactive elements

**Acceptance Criteria:**
- [ ] `canvas.push` tool — append components to the canvas panel
- [ ] `canvas.replace` tool — replace all canvas content
- [ ] `canvas.clear` tool — clear the canvas
- [ ] `canvas.snapshot` tool — capture current canvas state for memory/audit
- [ ] Tools registered in agent tool inventory, available to all agents
- [ ] Tool output published to Centrifugo channel: `a2ui:{agentInstanceId}`

### Story 22.3: Canvas panel in sera-web

**As** an operator viewing the dashboard
**I want** a canvas panel that renders agent-pushed UI components
**So that** I can see rich visualizations alongside the chat

**Acceptance Criteria:**
- [ ] `CanvasPanel` React component — renders A2UI component tree
- [ ] Subscribes to Centrifugo `a2ui:{agentInstanceId}` channel
- [ ] Supports incremental updates (push appends, replace overwrites, clear resets)
- [ ] Panel toggleable from chat view — slides in from right
- [ ] Responsive layout — stacks below chat on mobile
- [ ] Components are sandboxed: no `dangerouslySetInnerHTML`, no external images, no script execution

### Story 22.4: Interactive components and feedback

**As** an operator
**I want** to interact with canvas components (click buttons, fill text fields)
**So that** agents can collect structured input and act on my decisions

**Acceptance Criteria:**
- [ ] Button clicks publish `a2ui:action` event to Centrifugo with `{ componentId, action }`
- [ ] TextField submissions publish `a2ui:input` event with `{ componentId, value }`
- [ ] Agent receives these events as tool responses in its conversation context
- [ ] Debounced input — text fields don't fire on every keystroke

### Story 22.5: Component catalog and rendering

**As** sera-web
**I want** a typed component catalog mapping A2UI types to React components
**So that** new component types can be added without modifying the renderer

**Acceptance Criteria:**
- [ ] `ComponentCatalog` registry — maps `type` string to React component
- [ ] Built-in renderers for all Story 22.1 component types
- [ ] Unknown types render a fallback "unsupported component" placeholder
- [ ] Components are styled with sera-web design tokens (dark theme compatible)
- [ ] Catalog is extensible (plugin system can register custom renderers in future)

---

## DB Schema

```sql
-- Story 22.2: Canvas snapshots for audit/memory
CREATE TABLE canvas_snapshots (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  agent_instance_id uuid NOT NULL REFERENCES agent_instances(id),
  operator_id     uuid REFERENCES operators(id),
  components      jsonb NOT NULL,              -- A2UI component tree at snapshot time
  created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_canvas_snapshots_agent ON canvas_snapshots(agent_instance_id, created_at DESC);
```
