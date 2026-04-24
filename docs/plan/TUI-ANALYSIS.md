# TUI Analysis — claw-code vs sera-tui

> **Important preface:** This analysis was run when the local working tree was 28 commits behind `origin/main`. The read-only architect's "zero-hit grep" (no composer, no post_chat, no active_agent_id) reflected that stale state, not current main. All Wave G wiring (G.0.1 / G.0.2 / G.0.3 / G.1.1 / G.1.2 / G.2.1 / G.2.2 / G.2.3) is on `origin/main` as of 2026-04-24. The architectural observations (especially H.0.1 non-blocking startup and H.0.3 disconnected skeleton) remain valid regardless.

## TL;DR

**claw-code is not a ratatui TUI** — it is a `rustyline`-based line-oriented REPL with a blocking synchronous turn loop. Its composer is dead during streaming, its "disconnect handling" is an `ApiError` variant that aborts the turn, and its philosophy explicitly rejects terminal multiplexer UX ("the real interface is Discord"). It is not a useful reference for a chat-first ratatui TUI.

**sera-tui is architecturally ahead** on pane-based layout, HITL inline approval, configurable keybindings, and SSE streaming. Its two real gaps are **non-blocking startup** (`main.rs:105` awaits `refresh_all` before the first `terminal.draw`) and **disconnected-state skeleton rendering**. Both are surgical fixes, not architectural failures.

**Recommendation: incremental.** Keep the reducer + Action + AppCommand + GatewayClient scaffolding. Ship Wave H.0 (non-blocking startup + disconnected skeleton + layout polish) before any parity feature work.

---

## 1. claw-code architecture (what it actually is)

- **Stack:** `rustyline` 15 (readline) + `crossterm` 0.28 (colors only, no event capture) + `pulldown-cmark` 0.13 + `syntect` 5 + `tokio` (used only inside `block_on` for stream consumption). **No `ratatui`.**
- **Event loop:** classic blocking REPL — `loop { read_line(); run_turn(); print; }` at `rust/crates/rusty-claude-cli/src/main.rs:3579-3624`.
- **Composer:** `LineEditor` at `rust/crates/rusty-claude-cli/src/input.rs:101-198` wraps `rustyline::Editor`. `readline()` at `input.rs:149` is fully blocking. Two custom keybindings only (Ctrl-J and Shift-Enter → `Cmd::Newline`).
- **"Disconnected state":** not a concept. `ApiError` at `rust/crates/api/src/error.rs:20-73` covers transport/retry/SSE/credential failures. On network error mid-turn: `consume_stream` returns `Err`, spinner flips to `❌ Request failed`, control returns to the prompt. No reconnect loop, no offline banner, no heartbeat, no liveness probe. One narrow special case: single-shot re-send on `POST_TOOL_STALL_TIMEOUT` at `main.rs:7537-7556` for model stalls after a tool result.
- **Philosophy:** `PHILOSOPHY.md` states "The important interface here is not tmux, Vim, SSH, or a terminal multiplexer." The terminal REPL is a demo artifact of a coordination loop, not the product.

**What's worth borrowing:** `api/src/error.rs` error taxonomy (`is_retryable`, `safe_failure_class`, context-window / body-size / OAuth-expired classifications). Not the UX.

## 2. sera-tui architecture

- **Stack:** `ratatui` + `crossterm` + `tui-textarea` + `tokio` + `reqwest`.
- **Panes:** 4 rotating — Agents / Session / HITL / Evolve. Session now splits transcript + 5-line composer. Modal overlays for help, session picker, HITL approval.
- **State model:** reducer pattern. `App::dispatch(Action)` is pure apart from pushing to `pending: Vec<AppCommand>`; `Runtime::execute` drains commands async.
- **Event loop:** single tokio task at `src/main.rs`, `loop { draw; drain SSE; poll(tick); dispatch; execute; }`.
- **Chat surface (present on main as of 2026-04-24):** composer → Ctrl+Enter drains `pending_sends` / `pending_slash` → `Action::SubmitComposer` → `AppCommand::SendChat` → `GatewayClient::post_chat(agent, message)` → SSE parsed via `parse_sse_stream` → piped into `SessionView::apply_event`.
- **Modals:** help, session picker (Ctrl+P), inline HITL approval.
- **Slash commands:** `/new`, `/clear`, `/agent <name>`, `/help`, `/quit`.
- **Status bar:** bottom 1-line bar with `agent=… · session=… · conn=…`.
- **Bracketed paste:** `EnableBracketedPaste` on startup; pastes > 5 lines collapse to `[N-line paste]` placeholder; full text sent on submit.
- **Test coverage:** 111+ unit + integration tests.
- **Critical remaining bug:** `src/main.rs:105` awaits `Runtime::refresh_all(&mut app).await` *before* the first `terminal.draw`. With an unroutable gateway, three sequential 10s HTTP timeouts = up to 30s of blank terminal.

## 3. Gap matrix

| Capability | claw-code | sera-tui (as of 2026-04-24) |
|---|---|---|
| Non-blocking first frame | Yes (prompt prints before any network) | **No** — see H.0.1 |
| Disconnected skeleton | N/A | Badge exists; never renders when startup hangs |
| Composer (multi-line, paste, history) | rustyline: multi-line, history, no draft save | Multi-line + bracketed-paste + collapse; no history/draft yet |
| Slash commands + help overlay | `/help` etc. tab-complete | `/new /clear /agent /help /quit` + modal |
| Agent / session pickers | N/A (single session) | Agent list + session picker modal (Ctrl+P) |
| HITL inline approval | N/A | Inline modal (a/r/e) + side pane |
| Connection retry UI | N/A | SSE has backoff; HTTP startup has no retry |
| Config file | `.claw.json` + `.claw/settings.json` | env + flag only (despite `long_about`'s claim of `~/.sera/tui.toml`) |
| Keybinding configurability | rustyline hardcoded | Fully configurable `TuiKeybindings` |
| Mouse | No | `EnableMouseCapture` on, no handlers |
| Alternate-screen | Inline | Alternate screen |
| Streaming render | Markdown flush on boundary | Full repaint per tick (ratatui diffs) |
| Test coverage | Unit + integration | 111+ unit + integration |

## 4. Root-cause hypotheses for reported bugs

### Bug 1 — disconnected state blocks first render

`src/main.rs:105` calls `Runtime::refresh_all(&mut app).await` before entering the draw loop at `src/main.rs:107-108`. `refresh_all` sequentially awaits `list_agents`, `list_hitl`, `list_evolve_proposals`. Each uses `reqwest` with a 10-second timeout. Unroutable gateway = up to 30s blank screen.

**Fix (H.0.1):** delete the inline await. Push `AppCommand::RefreshAll` into `app.pending` before the loop so the first iteration draws the skeleton and executes HTTP out-of-band. Route results via the existing mpsc bridge used for SSE.

### Bug 2 — "still no way to chat"

**Confirmed root cause:** the user was running from a local tree 28 commits behind `origin/main`. Pre-Wave-G sera-tui had no composer at all. Post-pull, the composer + slash commands + post_chat + SSE are all present. Most likely now runnable end-to-end.

**Secondary concerns (if post-pull still shows issues):**
- Gateway `/api/agents` must return at least one agent for the selector to have anything to pick. `sera-local` seeds an agent named `sera`.
- Ctrl+Enter: some terminals don't distinguish `Ctrl+Enter` from plain `Enter`. Add `Alt+Enter` as a documented alternate binding.
- `SERA_API_URL` must point at the running gateway (`:42540` per `sera-local`, NOT `:8080`).

## 5. Wave H plan

### H.0 — critical (release-blocking)

- **H.0.1 non-blocking startup** — delete `main.rs:105` inline await; route `RefreshAll` through the command queue; spawn HTTP work into `tokio::spawn` + mpsc. ~80 LOC.
- **H.0.2 disconnected skeleton + retry** — centered "Connecting to `<url>`…" paragraph with `r` to retry; exponential backoff (1s → 30s). ~120 LOC.
- **H.0.3 alt binding for Ctrl+Enter** — add `Alt+Enter` as an alternate submit binding to handle terminals that don't emit Ctrl+Enter distinctly. ~20 LOC.

### H.1 — parity / usability

- **H.1.1** composer history (Up/Down in empty composer recalls last 50 per session, persisted to `~/.sera/tui-history.jsonl`)
- **H.1.2** draft autosave (per-session draft persisted on every keystroke, restored on session reopen)
- **H.1.3** config file loader (`~/.sera/tui.toml` as promised by `config.rs` long_about)
- **H.1.4** inline (non-alt-screen) `--inline` mode flag
- **H.1.5** tool call collapse/expand in transcript (`space` key)

### H.2 — stretch polish

- Mouse click/scroll handlers, differential render profiling, contextual help panel, theme switching.

## 6. Decision: incremental

**For:** 111+ tests encode correct behavior; reducer/Action/AppCommand separation is exemplary; keybinding discipline matches project CLAUDE.md rule. The two reported bugs are one misplaced `await` plus a stale checkout, not architectural failure. claw-code is not a valid rewrite target (rustyline REPL, not ratatui).

**Against:** sera-tui was born as an operator dashboard; the current 4-equal-panes layout may feel wrong for chat-first UX. Consensus synthesis: keep all scaffolding; swap only the **layout** from 4-equal-panes-rotating to agents-sidebar + chat-main + status-footer. Treat the view structs as embeddable widgets. ~200 LOC in `ui.rs` + `main.rs`; preserves all tests. Reassess after H.0.1 + H.0.2 land.

## References

- `/home/entity/projects/claw-code/rust/crates/rusty-claude-cli/Cargo.toml`
- `/home/entity/projects/claw-code/rust/crates/rusty-claude-cli/src/input.rs:101-198`
- `/home/entity/projects/claw-code/rust/crates/rusty-claude-cli/src/main.rs:3579-3624`
- `/home/entity/projects/claw-code/rust/crates/rusty-claude-cli/src/main.rs:7537-7608`
- `/home/entity/projects/claw-code/rust/crates/api/src/error.rs:20-242`
- `/home/entity/projects/claw-code/PHILOSOPHY.md`
- `/home/entity/projects/sera/rust/crates/sera-tui/src/main.rs:95-136`
- `/home/entity/projects/sera/rust/crates/sera-tui/src/app/mod.rs` (post-Wave-G: composer, post_chat, active_agent_id, Action::SelectAgent, Action::ExecuteSlash, HITL modal, session picker)
- `/home/entity/projects/sera/rust/crates/sera-tui/src/views/session.rs` (post-Wave-G: composer, handle_paste, pending_sends, pending_slash)
- `/home/entity/projects/sera/rust/crates/sera-tui/src/client.rs:643-700` (post_chat, parse_sse_stream)
