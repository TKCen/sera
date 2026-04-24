# SPEC — Chat TUI (Wave J)

> **Status:** design spec. Implementation lives behind Wave J beads.
> **Parent bead:** sera-dux5 (retargeted from operator-dashboard to chat-first)
> **Inspirations:** Claude Code (Ink/TS, alt-screen, chat-dominant), hermes-agent (Ink/React/TS)
> **Anti-inspiration:** claw-code (rustyline REPL — wrong category)

## Goal

Rust ratatui chat TUI that gives a Claude-Code-class interactive experience against the sera gateway. Conversation is the surface; tool calls, subagents, approvals, markdown all render inline. One binary (`sera-tui`), one job: chat with a SERA agent.

## Non-goals (v1)

- Operator dashboard features (HITL queue list, evolve proposals list) — accessible via modal shortcuts only; not dominant UI
- Multi-pane rotation
- Mouse selection / mouse scroll (stretch)
- Web-side parity (sera-web is a separate surface)
- Running agents directly (no embedded LLM) — always via gateway

## Decisions (ratified)

| # | Decision |
|---|----------|
| D1 | **Layout**: chat-dominant. Conversation fills screen; composer at bottom; 1-line status; agents & HITL are modal overlays (Ctrl+A, Ctrl+H) |
| D2 | **Tool calls**: inline collapsible blocks (`⏺ Write(path) ▸`). Space toggles. Default collapsed |
| D3 | **Subagents**: stacked drill-in. Enter on `⏺ Task(…) ▸` pushes child thread onto stack; Esc pops. Breadcrumb in status bar |
| D4 | **Markdown**: progressive render with `pulldown-cmark` + `syntect` for code blocks |
| D5 | **Approvals**: inline block in transcript (`⚠ Approval required: [a]pprove [r]eject [e]scalate`) — not modal |
| D6 | **Interrupt**: ESC cancels current turn (requires `POST /api/chat/cancel` gateway route). Ctrl+C exits TUI |
| D7 | **Composer**: `/` autocomplete popup, `@` file fuzzy completion, Alt+Enter newline, Enter submit, Up/Down history (persisted to `~/.sera/chat-history.jsonl`) |
| D8 | **Binary**: redesign `sera-tui`. Retire 4-pane dashboard identity |

## Layout

```
┌──────────────────────────────────────────────────────────────────────┐
│  sera chat · agent=sera · session=sw-8f3a1c · streaming · 2.1k tok  │ status (1 line)
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  user › write a haiku about rust                                     │
│                                                                      │
│  sera                                                                │
│  Sure, here's one:                                                   │
│                                                                      │
│      Borrow checker sighs                                            │
│      Lifetimes entwine softly                                        │
│      Segfault never comes                                            │
│                                                                      │
│  user › now save it to poem.md                                       │
│                                                                      │
│  sera                                                                │
│  ⏺ Write(poem.md) ▸                                                  │  ← collapsed tool
│                                                                      │
│  Done. Saved as poem.md.                                             │
│                                                                      │
│  user › have the researcher find 3 rust crates for poetry            │
│                                                                      │
│  sera                                                                │
│  ⏺ Task(researcher) ▸                                                │  ← collapsed subagent
│                                                                      │
│  Based on the researcher's findings, the top crates are: …           │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│ › type a message (Alt+Enter=newline, /=cmd, @=file)                 │  composer (3–5 lines)
├──────────────────────────────────────────────────────────────────────┤
│ tab: focus  esc: cancel turn  ^c: exit  ^a: agents  ^h: hitl  ^p: …│ key-hint footer (1 line, collapsible)
└──────────────────────────────────────────────────────────────────────┘
```

Expanded tool call (Space on a collapsed block):

```
  ⏺ Write(poem.md) ▾
     args:
       content: "Borrow checker sighs\n…"
     result:
       ok — 62 bytes
```

Expanded subagent (Enter on `⏺ Task(researcher) ▸` drills in; Esc pops):

```
  sera › researcher          ← breadcrumb in status bar
  ┌────────────────────────
  │ user › find 3 rust crates for poetry
  │
  │ researcher
  │ ⏺ web_search(query="rust poetry crates") ▸
  │
  │ I found …
  └────────────────────────
```

## Event model

Client subscribes to one SSE stream per active turn. Incoming event types (from gateway):

| Event | Renders as |
|-------|-----------|
| `text_delta` | Appended to current assistant message; markdown buffered until boundary, then styled flush |
| `tool_call_begin` | New collapsed block; `⏺ {tool}({summary}) ▸` |
| `tool_call_end` | Attach result to matching begin block |
| `task_begin` (subagent) | Collapsed `⏺ Task({agent}) ▸` block; recurse |
| `task_delta` | Routed to child thread state; if current drill-in is this child, live-render |
| `task_end` | Mark task block complete |
| `approval_required` | Inline approval block; pauses stream until user acts |
| `usage` | Update status bar (tokens, cost) |
| `turn_completed` | Mark turn done; release composer; render final turn separator |
| `error` | Inline error block with retry suggestion |

Outgoing (client → gateway):

| Action | Endpoint |
|--------|----------|
| Submit message | `POST /api/chat {agent, message, stream: true}` |
| Cancel turn | `POST /api/chat/cancel {session_id}` (new route — gateway bead) |
| Approve/reject/escalate | `POST /api/hitl/requests/{id}/{action}` (sera-z6ql, already live) |
| List agents | `GET /api/agents` |
| List sessions | `GET /api/sessions?agent_id={id}` |
| New session | implicit — first `POST /api/chat` creates one |
| Load session transcript | `GET /api/sessions/{id}/transcript` |

## State model

```
struct App {
    // Core
    active_agent: Option<Agent>,
    session_stack: Vec<ThreadView>,   // parent at [0], current drill-in at top
    current_turn: Option<TurnState>,

    // Composer
    composer: TextArea,
    composer_history: RingBuffer<String>,
    composer_draft_key: Option<SessionKey>,  // autosave

    // Modals
    agents_modal: Option<AgentPickerModal>,
    hitl_queue_modal: Option<HitlQueueModal>,
    session_picker_modal: Option<SessionPickerModal>,
    model_picker_modal: Option<ModelPickerModal>,
    help_modal: bool,

    // Status
    connection: ConnectionState,
    usage: UsageSnapshot,       // tokens, cost per turn

    // Config
    keybindings: TuiKeybindings,
    theme: Theme,
}

struct ThreadView {
    session_id: SessionId,
    agent_id: String,
    blocks: Vec<Block>,                   // user, assistant, tool, task, approval, error
    scroll: ScrollState,
    streaming: Option<StreamCursor>,      // which block is currently live-updating
}

enum Block {
    UserMessage { text: String },
    AssistantMessage { markdown: ParsedMarkdown, streaming_ranges: Vec<Range<usize>> },
    ToolCall { tool: String, summary: String, args: Value, result: Option<ToolResult>, expanded: bool },
    Task { agent: String, summary: String, child_thread: ThreadView, expanded: bool },
    Approval { request_id: String, tool: String, reason: String, status: ApprovalStatus },
    Error { message: String, retryable: bool },
    TurnSeparator,
}
```

## Commands

Slash commands (`/` in composer opens autocomplete popup):

| Command | Action |
|---------|--------|
| `/help` | Open help modal |
| `/quit` | Exit TUI |
| `/new` or `/clear` | Start fresh session for active agent |
| `/agent <name>` | Switch active agent |
| `/model <name>` | Set/override agent's model |
| `/approve <id>`, `/reject <id>`, `/escalate <id>` | HITL action by id |
| `/retry` | Re-send last user message |
| `/export` | Write transcript to `./sera-transcript-{session}.md` |
| `/debug` | Toggle debug overlay (raw SSE events) |

Keybindings (all configurable in `~/.sera/tui.toml`):

| Key | Action |
|-----|--------|
| `Enter` | Submit composer |
| `Alt+Enter` | Newline in composer |
| `ESC` | If turn streaming → cancel. Else if drilled into subagent → pop back. Else close modal. |
| `Ctrl+C` | Exit TUI |
| `Ctrl+A` | Open agent picker modal |
| `Ctrl+H` | Open HITL queue modal |
| `Ctrl+P` | Open session picker modal |
| `Ctrl+M` | Open model picker modal |
| `Space` | When cursor is on a collapsed tool/task block, toggle expansion |
| `Enter` | When cursor is on a `⏺ Task` block, drill into subagent thread |
| `Up/Down` | Scroll transcript (composer unfocused) OR cycle composer history (composer focused + empty) |
| `Tab` | Toggle focus between composer and transcript |
| `?` | Show keybinding overlay |

## Configuration

`~/.sera/tui.toml`:

```toml
[api]
url = "http://localhost:42540"
api_key = ""

[theme]
palette = "dark"   # dark | light | custom
user_color = "cyan"
assistant_color = "default"
tool_color = "yellow"
task_color = "magenta"
error_color = "red"

[composer]
history_size = 500
submit_binding = "Enter"
newline_binding = "Alt+Enter"

[keybindings]
# override specific bindings here

[subagents]
# max drill-in depth (UX; gateway enforces its own limits)
max_depth = 5
```

## Testing

- **Unit tests** — every widget + reducer action. Target ≥150 unit tests by v1.
- **Integration tests** — in-process gateway via `sera-e2e-harness::InProcessGateway`; exercise full send→receive→render cycles.
- **Snapshot tests** — serialize rendered buffer to text; compare against golden files for each layout state (empty, streaming, collapsed tool, expanded tool, approval block, drilled-in subagent).
- **Property tests** — composer history never drops messages; drill-in stack never orphans a session; tool/task blocks always have matching begin/end.

## Staging

### J.0 — MVP (ship first, end-to-end usable)

| Bead | Feature | Est. LOC |
|------|---------|----------|
| J.0.1 | Layout pivot: chat-dominant; retire 4-pane rotation; agents pane → Ctrl+A modal | 300 |
| J.0.2 | Block-based transcript (Vec<Block> replaces string append); render assistant/user/tool blocks; markdown stubbed as plain text | 250 |
| J.0.3 | Collapsible tool blocks (Space toggles) | 150 |
| J.0.4 | ESC cancels turn → `POST /api/chat/cancel` (**requires gateway bead K.1 first**) | 100 |
| J.0.5 | `/` autocomplete popup; `@` file completer | 200 |
| J.0.6 | Non-blocking startup (H.0.1 — already a known bug) | 80 |
| J.0.7 | Disconnected skeleton + retry (H.0.2) | 120 |
| J.0.8 | Composer history persistence | 100 |

**Gateway prerequisite:** K.1 `POST /api/chat/cancel` route + CancellationToken hookup.

### J.1 — rich rendering

| Bead | Feature | Est. LOC |
|------|---------|----------|
| J.1.1 | pulldown-cmark integration — progressive markdown render | 300 |
| J.1.2 | syntect integration — code block syntax highlighting | 200 |
| J.1.3 | Usage/cost in status bar (**requires sera-xoie** — already filed) | 80 |
| J.1.4 | Inline HITL approval block (promote G.2.2 modal → inline) | 150 |
| J.1.5 | Help modal with keybinding overlay | 80 |

### J.2 — subagent drill-in

| Bead | Feature | Est. LOC |
|------|---------|----------|
| J.2.1 | Task block widget + child ThreadView state | 200 |
| J.2.2 | Drill-in stack (Enter push, Esc pop); breadcrumb in status bar | 200 |
| J.2.3 | Subagent live-streaming into child thread | 150 |
| J.2.4 | Gateway: route subagent SSE events with `parent_task_id` correlation | 150 (gateway bead) |

### J.3 — polish

- Model picker modal (J.3.1)
- Debug overlay (J.3.2)
- `/export` transcript writer (J.3.3)
- Config file loader `~/.sera/tui.toml` (J.3.4 — H.1.3 rename)
- Theme customization (J.3.5)

## Risks

- **Markdown streaming flicker**: progressive render must commit style only at block boundaries (end of line for lists, closing fence for code) to avoid flashing. Mitigation: buffer + flush on boundary (hermes-agent + Claude Code both do this).
- **Subagent event correlation**: gateway needs to tag SSE events with `parent_task_id` so child thread state knows where they belong. This is new plumbing. Currently subagent handoff scaffold exists but SSE correlation isn't implemented. Wave B.3 (A2A real handler) is a prerequisite.
- **ESC overloading**: ESC handles cancel-turn + pop-drill-in + close-modal. Dispatch precedence must be documented and tested.
- **Terminal variance**: Alt+Enter newline doesn't work on all terminals (iTerm needs explicit config; some Windows terminals). Fallback: `Ctrl+J`.
- **Tool argument size**: a `⏺ Write(path)` with 10KB of content can't render inline when expanded. Mitigation: truncate with `… (2.1KB)` hint + `/export` to see full.

## Open questions (resolve during implementation)

1. Should the chat TUI have an inline mode (`--inline` to skip alt-screen) for CI / scripting? → defer to J.3.
2. How do we surface `/api/chat`'s `200 OK` non-stream path? → Currently only stream=true is assumed; add `--no-stream` flag later for debugging.
3. Do we want live typing indicators (dot animation) during stream latency? → Nice to have; add under J.3.
4. Should we support attaching files (`@path` completed and content included in message)? → Yes, but content inlining is a gateway concern; TUI just expands `@path` to `<file:path>content</file>` at submit time. Defer to J.3.
