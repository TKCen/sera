# `docs/architecture/` — visual architecture artifacts

## `workflow-map.html` + `workflow-flows.json`

Single-page, data-driven explorer for SERA package/component workflows.

### Open it

Browsers block `fetch()` from `file://` URLs, so use one of these:

```bash
# from repo root
python3 -m http.server -d docs/architecture 8000
# then open http://localhost:8000/workflow-map.html
```

Or open `workflow-map.html` directly and use the **Load JSON…** button to pick `workflow-flows.json` from disk.

### What it shows

- Every SERA Rust crate, gateway internal, runtime internal, subsystem, storage layer, external service, and legacy package, laid out in seven categorical columns.
- A scrollable list of named flows on the left (chat turn, HITL approval, BYOH dispatch, evolution proposal apply, mail-to-workflow, OpenAI/Anthropic compat, A2A peer exchange, plugin call, skill execution, memory write, kill switch arm, Discord turn, MCP tool call, and more).
- When you click a flow:
  - Involved components are highlighted; everything else is dimmed.
  - Numbered curved edges are drawn for each step, in order, coloured by kind (`request` / `response` / `internal` / `async` / `persist` / `external`).
  - The right-hand panel lists the steps with their HTTP route or function-level label, plus any extra notes (timeout behaviour, security caveats, ops gotchas).
- Click an edge number or step to highlight it in the diagram.

### Status conventions

- **active** — handler present in the active Rust gateway/runtime.
- **partial** — module exists but is a scaffold (e.g. `sera-cache`, `sera-secrets`, `sera-eval`) or is one of two parallel implementations (e.g. hindsight backend).
- **speculative** — flow is not implemented (e.g. *invite new user*). Kept in the JSON so the diagram doesn't pretend it ships, and so the gap is obvious.
- **legacy** — lives under `legacy/`. Reference only.

### JSON schema (informal)

```jsonc
{
  "categories": [{ "id": "gateway", "label": "Gateway", "col": 1 }],
  "components": [
    {
      "id": "sera-gateway",                      // stable id used by flow steps
      "name": "sera-gateway",                    // display name on the box
      "summary": "Main axum API server…",        // tooltip + subtitle text
      "category": "gateway",                     // drives column placement
      "kind": "rust-bin",                        // free-form (rust-lib, ts, go, external, …)
      "status": "active",                        // active | partial | speculative | legacy
      "position": { "col": 1, "row": 0 }         // row within the category column
    }
  ],
  "flows": [
    {
      "id": "chat-turn",
      "name": "Chat turn (HTTP)",
      "summary": "User sends a message via POST /api/chat…",
      "status": "active",
      "tags": ["foundational", "http"],
      "steps": [
        {
          "from": "client-http",
          "to": "sera-gateway",
          "label": "POST /api/chat {message, agent?, stream?}",
          "note": "Bearer auth required.",
          "kind": "request"                       // request | response | internal | async | persist | external
        }
      ]
    }
  ]
}
```

### Adding a flow

1. Pick an existing source — a route in `rust/crates/sera-gateway/src/routes/`, a runtime sequence in `rust/crates/sera-runtime/src/turn.rs`, a session-log entry under `rust/.omc/wiki/`, etc.
2. Add steps to `workflow-flows.json` referencing existing `component.id`s. Create a new component if the flow truly touches one not yet listed.
3. Mark `status` honestly. If only part of the path is implemented today, use `partial` or `speculative`.
4. Reload the page — no build step.

### Verification

```bash
python3 -c "import json; json.load(open('docs/architecture/workflow-flows.json'))"
```
