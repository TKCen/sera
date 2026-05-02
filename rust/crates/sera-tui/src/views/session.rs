//! Session view — block-based transcript + composer.
//!
//! **J.0.2 (sera-s3gb)**: the flat `Vec<TranscriptEntry>` + `Vec<String>`
//! tool log of waves G/J.0.1 was replaced by a typed [`Block`] enum (see
//! [`crate::views::blocks`]).  Markdown is stubbed as plain text in this
//! bead; J.1.1 (sera-7fpx) wires `pulldown-cmark` parsing and J.1.2
//! (sera-jie3) adds `syntect` for code blocks.  Tool/task/approval/error
//! variants are shaped here so the leaf beads (J.0.3, J.1.4, J.0.4) only
//! need to fill in the SSE-event handlers.

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block as TuiBlock, Borders, List, ListItem};
use ratatui::Frame;
use tui_textarea::TextArea;

use super::agent_list::make_block;
use super::blocks::{ApprovalStatus, Block, ThreadView, ToolResult};
use crate::client::{ConnectionState, SessionSummary, StreamEvent, TranscriptEntry};
use crate::highlight;
use crate::keybindings::{display_first, TuiKeybindings};

/// Which pane inside the Session view holds keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerFocus {
    Composer,
    Transcript,
}

/// Viewer state for a single session.  Owns:
/// * metadata (agent id, session id, state)
/// * a typed [`Block`] transcript seeded by GET, appended by SSE
/// * scroll bookkeeping — auto-scrolls to tail unless the user has paused
/// * a multi-line composer for drafting outgoing messages
pub struct SessionView {
    pub session: Option<SessionSummary>,
    /// Block-based transcript — the canonical store as of J.0.2.
    pub blocks: Vec<Block>,
    pub scroll_offset: u16,
    pub auto_scroll: bool,
    pub conn: ConnectionState,
    pub composer: TextArea<'static>,
    pub focus: ComposerFocus,
    /// Messages drained from the composer via `submit_composer`.
    pub pending_sends: Vec<String>,
    /// Slash-command strings drained from the composer via `submit_composer`.
    pub pending_slash: Vec<String>,
    /// Full text of a long paste that was collapsed to a placeholder token
    /// in the textarea.
    pub composer_full_text: Option<String>,
}

impl SessionView {
    pub fn new() -> Self {
        let mut composer = TextArea::default();
        composer.set_placeholder_text("Type a message…");
        Self {
            session: None,
            blocks: Vec::new(),
            scroll_offset: 0,
            auto_scroll: true,
            conn: ConnectionState::Disconnected,
            composer,
            focus: ComposerFocus::Composer,
            pending_sends: Vec::new(),
            pending_slash: Vec::new(),
            composer_full_text: None,
        }
    }

    pub fn set_session(&mut self, session: SessionSummary) {
        self.session = Some(session);
        self.blocks.clear();
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    /// Hydrate from a wire-format transcript fetch.  Each [`TranscriptEntry`]
    /// becomes a [`Block`] of the matching role:
    /// * `"user"` → [`Block::UserMessage`]
    /// * `"tool"` → [`Block::ToolCall`] (completed, collapsed, no args)
    /// * anything else (including `"assistant"`, `"system"`) →
    ///   [`Block::AssistantMessage`] (matches the previous reducer's default
    ///   for unknown roles)
    pub fn set_transcript(&mut self, entries: Vec<TranscriptEntry>) {
        self.blocks = entries
            .into_iter()
            .map(|entry| match entry.role.as_str() {
                "user" => Block::UserMessage { text: entry.text },
                "tool" => Block::ToolCall {
                    tool: String::new(),
                    summary: entry.text,
                    args: serde_json::Value::Null,
                    result: None,
                    expanded: false,
                },
                _ => Block::AssistantMessage {
                    text: entry.text,
                    streaming: false,
                },
            })
            .collect();
    }

    /// Apply a [`StreamEvent`] to the view state.  Returns true if the
    /// update is "display-worthy" (i.e. the app should rerender).
    ///
    /// Tool events are folded into [`Block::ToolCall`] entries inline
    /// rather than the legacy side `tool_log` pane:
    ///
    /// * `tool_start` — push a new collapsed `ToolCall` block; the `summary`
    ///   is the delta payload (often the args preview).  J.0.3 (sera-c6mb)
    ///   wires Space-toggle on top of this shape.
    /// * `tool` (intermediate update) — append the delta to the summary of
    ///   the most recent unresolved `ToolCall`; does **not** push a new block.
    /// * `tool_end` — attach a [`ToolResult`] to the most recent matching
    ///   `ToolCall` block.
    pub fn apply_event(&mut self, ev: StreamEvent) -> bool {
        let et = ev.event_type.to_ascii_lowercase();

        // J.2.1: subagent_start SSE frame → push a collapsed Task block.
        // `tool` carries the agent id; `delta` is the initial summary;
        // `session_id` is repurposed as the task_id for child-event routing.
        if et == "subagent_start" {
            let task_id = if !ev.session_id.is_empty() {
                ev.session_id.clone()
            } else {
                ev.tool.clone()
            };
            self.push_task(task_id, ev.tool.clone(), ev.delta.clone());
            return true;
        }

        // J.1.4: approval_required SSE frame → push an inline Approval block.
        // The request_id comes in `session_id` (repurposed) or the `delta`
        // field; tool name is in `tool`; reason/summary is in `delta`.
        if et == "approval_required" {
            let request_id = if !ev.session_id.is_empty() {
                ev.session_id.clone()
            } else {
                ev.delta.clone()
            };
            self.push_approval(request_id, ev.tool.clone(), ev.delta.clone());
            return true;
        }

        let is_tool_event = et == "tool" || et == "tool_start" || et == "tool_end" || !ev.tool.is_empty();
        if is_tool_event {
            if et == "tool_end" {
                self.attach_tool_result(&ev.tool, &ev.delta);
            } else if et == "tool_start" {
                self.push_tool_call(&ev.tool, &ev.delta);
            } else {
                // Intermediate `tool` update: append delta to the most recent
                // unresolved ToolCall rather than creating a duplicate block.
                self.update_tool_call(&ev.tool, &ev.delta);
            }
            return true;
        }
        if ev.delta.is_empty() {
            return false;
        }
        let role = if ev.role.is_empty() {
            "assistant"
        } else {
            ev.role.as_str()
        };
        match role {
            "user" => {
                // User-role deltas are rare from the SSE stream (the
                // composer pushes the user block directly), but the wire
                // format allows replays — preserve the legacy behaviour
                // of merging consecutive same-role entries.
                match self.blocks.last_mut() {
                    Some(Block::UserMessage { text }) => text.push_str(&ev.delta),
                    _ => self.blocks.push(Block::UserMessage {
                        text: ev.delta.clone(),
                    }),
                }
            }
            _ => {
                Block::extend_assistant_text(&mut self.blocks, &ev.delta);
            }
        }
        true
    }

    /// Mark all in-flight streaming on the trailing assistant block as
    /// finished.  Called by the runtime when an SSE stream ends cleanly
    /// or a `turn_completed` frame arrives.  Wired by J.0.4 (sera-zate)
    /// on ESC-cancel and by the runtime on stream completion.
    #[allow(dead_code)]
    pub fn finalize_turn(&mut self) {
        Block::finalize_streaming(&mut self.blocks);
    }

    fn push_tool_call(&mut self, tool: &str, delta: &str) {
        // Treat the delta as the inline summary — most autonomous gateway
        // tool_start frames put the args preview in `delta`.  When delta
        // looks like JSON, store it in `args` as well.
        let args = if delta.trim_start().starts_with('{') {
            serde_json::from_str(delta).unwrap_or(serde_json::Value::Null)
        } else {
            serde_json::Value::Null
        };
        self.blocks.push(Block::ToolCall {
            tool: tool.to_owned(),
            summary: delta.to_owned(),
            args,
            result: None,
            expanded: false,
        });
    }

    /// Append `delta` to the summary of the most recent unresolved `ToolCall`
    /// whose tool name matches (or any if `tool` is empty).  Used for
    /// intermediate `tool` stream frames that refine the args preview without
    /// starting a new invocation.
    fn update_tool_call(&mut self, tool: &str, delta: &str) {
        for block in self.blocks.iter_mut().rev() {
            if let Block::ToolCall {
                tool: t,
                summary,
                result,
                ..
            } = block
                && result.is_none()
                && (tool.is_empty() || t == tool)
            {
                summary.push_str(delta);
                return;
            }
        }
        // No open call found — treat it like a tool_start so the frame is
        // not silently dropped.
        self.push_tool_call(tool, delta);
    }

    fn attach_tool_result(&mut self, tool: &str, delta: &str) {
        // Walk back to the most recent unresolved ToolCall whose tool
        // matches.  When `tool` is empty we attach to the most recent
        // unresolved one regardless.
        for block in self.blocks.iter_mut().rev() {
            if let Block::ToolCall {
                tool: t,
                result,
                ..
            } = block
                && result.is_none()
                && (tool.is_empty() || t == tool)
            {
                *result = Some(ToolResult {
                    ok: true,
                    output: delta.to_owned(),
                });
                return;
            }
        }
        // No matching call found — push a synthetic completed block so
        // the operator still sees the result.  Should be rare.
        self.blocks.push(Block::ToolCall {
            tool: tool.to_owned(),
            summary: String::new(),
            args: serde_json::Value::Null,
            result: Some(ToolResult {
                ok: true,
                output: delta.to_owned(),
            }),
            expanded: false,
        });
    }

    /// Push a user message directly (used when the composer submits — the
    /// gateway echoes the user message back in the SSE stream, but
    /// rendering it locally first keeps the UI responsive).
    #[allow(dead_code)] // J.0.4 / leaves wire this up
    pub fn push_user_message(&mut self, text: String) {
        self.blocks.push(Block::UserMessage { text });
    }

    /// Push an inline error block.  Wired by J.0.7 (disconnected-state)
    /// and the gateway error event path.
    #[allow(dead_code)] // leaves wire this up
    pub fn push_error(&mut self, message: String, retryable: bool) {
        self.blocks.push(Block::Error { message, retryable });
    }

    /// Push an inline approval block.  Called by `apply_event` on
    /// `approval_required` SSE frames (J.1.4).
    pub fn push_approval(&mut self, request_id: String, tool: String, reason: String) {
        self.blocks.push(Block::Approval {
            request_id,
            tool,
            reason,
            status: ApprovalStatus::Pending,
        });
    }

    /// Update the status of an inline approval block by request id.
    /// Called by the app after the HTTP action completes (J.1.4).
    pub fn update_approval_status(&mut self, request_id: &str, status: ApprovalStatus) {
        Block::update_approval_status(&mut self.blocks, request_id, status);
    }

    /// Return the `request_id` of the first `Pending` inline Approval block,
    /// if any.  Used by the app reducer to route approve/reject/escalate keys
    /// to the inline block when one is waiting (J.1.4).
    pub fn first_pending_approval_id(&self) -> Option<&str> {
        self.blocks.iter().find_map(|b| {
            if let Block::Approval {
                request_id,
                status: ApprovalStatus::Pending,
                ..
            } = b
            {
                Some(request_id.as_str())
            } else {
                None
            }
        })
    }

    /// Push a turn separator.  Renderers use this to draw a thin rule
    /// between turns.
    #[allow(dead_code)] // J.0.4 will wire this on turn_completed
    pub fn push_turn_separator(&mut self) {
        self.blocks.push(Block::TurnSeparator);
    }

    /// Push a collapsed Task block for a subagent invocation.
    ///
    /// `task_id` is the stable routing key that matches `parent_task_id` on
    /// child SSE events (sera-pmil).  J.2.3 (sera-mfj3) streams into
    /// `child_thread`; J.2.2 (sera-9vkz) wires Enter/Esc drill-in.
    pub fn push_task(&mut self, task_id: String, agent: String, summary: String) {
        self.blocks.push(Block::Task {
            task_id,
            agent,
            summary,
            child_thread: ThreadView::default(),
            expanded: false,
        });
    }

    pub fn set_connection(&mut self, state: ConnectionState) {
        self.conn = state;
    }

    /// User scrolled up — pause auto-scroll so the view stays put when
    /// new events land.
    pub fn scroll_up(&mut self) {
        self.auto_scroll = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn page_up(&mut self) {
        self.auto_scroll = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(10);
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    pub fn page_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(10);
    }

    /// Jump to tail and re-arm auto-scroll (bound to the `end` key).
    pub fn jump_to_end(&mut self) {
        self.auto_scroll = true;
        self.scroll_offset = 0;
    }

    /// Toggle keyboard focus between the composer and the transcript.
    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            ComposerFocus::Composer => ComposerFocus::Transcript,
            ComposerFocus::Transcript => ComposerFocus::Composer,
        };
    }

    /// Returns true when the composer currently holds focus.
    pub fn composer_focused(&self) -> bool {
        self.focus == ComposerFocus::Composer
    }

    /// Forward a raw key event to the composer textarea.
    pub fn input_to_composer(&mut self, event: KeyEvent) {
        self.composer.input(event);
    }

    /// Drain the composer buffer and return the accumulated text.
    /// If a long paste was stashed in `composer_full_text`, that is returned
    /// verbatim.  Resets both the textarea and the stash to empty.
    pub fn take_composer_text(&mut self) -> String {
        let text = if let Some(full) = self.composer_full_text.take() {
            full
        } else {
            self.composer.lines().join("\n")
        };
        let mut fresh = TextArea::default();
        fresh.set_placeholder_text("Type a message…");
        self.composer = fresh;
        text
    }

    /// Handle a bracketed-paste event.  Pastes of ≤ 5 lines are inserted
    /// verbatim; longer pastes insert a `[N-line paste]` placeholder while
    /// the full content is preserved in `composer_full_text`.
    pub fn handle_paste(&mut self, content: String) {
        const COLLAPSE_THRESHOLD: usize = 5;
        let line_count = content.lines().count();
        if line_count <= COLLAPSE_THRESHOLD {
            for line in content.lines() {
                for ch in line.chars() {
                    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
                    self.composer
                        .input(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
                }
            }
        } else {
            self.composer_full_text = Some(content);
            let placeholder = format!("[{line_count}-line paste]");
            for ch in placeholder.chars() {
                use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
                self.composer
                    .input(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
            }
        }
    }


    /// Replace the trigger token with the selected autocomplete item.
    ///
    /// Slash mode: the  prefix is replaced with the full command + space.
    /// At-file mode:  is replaced with the chosen path.
    pub fn insert_autocomplete(
        &mut self,
        mode: &crate::autocomplete::PopupMode,
        filter: &str,
        insert: &str,
    ) {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use crate::autocomplete::PopupMode;
        let current = self.composer.lines().join("
");
        let new_text = match mode {
            PopupMode::Slash => {
                // Replace leading /+filter with the full command token + space.
                let trigger_len = 1 + filter.len();
                format!("{} {}", insert, current.get(trigger_len..).unwrap_or("").trim_start())
            }
            PopupMode::AtFile => {
                if let Some(at_pos) = current.rfind('@') {
                    let before = &current[..at_pos];
                    let after_at = &current[at_pos + 1..];
                    let rest = after_at
                        .find(char::is_whitespace)
                        .map(|i| &after_at[i..])
                        .unwrap_or("");
                    format!("{before}@{insert}{rest}")
                } else {
                    current
                }
            }
        };
        let mut fresh = TextArea::default();
        fresh.set_placeholder_text("Type a messageâ¦");
        for ch in new_text.trim_end().chars() {
            fresh.input(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        self.composer = fresh;
    }
    /// Submit the current composer buffer: drain text → route to either
    /// `pending_slash` (if the text starts with `/`) or `pending_sends`.
    /// No-op when the buffer is blank.
    pub fn submit_composer(&mut self) {
        let text = self.take_composer_text();
        if text.trim().is_empty() {
            return;
        }
        if text.trim_start().starts_with('/') {
            self.pending_slash.push(text);
        } else {
            tracing::info!(message = %text, "composer submit queued");
            self.pending_sends.push(text);
        }
    }

    /// **J.0.2 (sera-s3gb)**: render the chat canvas.  Composer is owned
    /// by the root layout; this method renders the block-based transcript
    /// only — the legacy bottom tool-log pane is gone (tool calls are
    /// inline blocks now).
    pub fn render_chat(&self, frame: &mut Frame, area: Rect, focused: bool, kb: &TuiKeybindings) {
        self.render_transcript(frame, area, focused, kb);
    }

    /// **J.0.1**: render only the composer into `area`.
    pub fn render_composer_only(&self, frame: &mut Frame, area: Rect) {
        self.render_composer(frame, area);
    }

    fn render_transcript(&self, frame: &mut Frame, area: Rect, focused: bool, kb: &TuiKeybindings) {
        let transcript_focused = focused && self.focus == ComposerFocus::Transcript;
        let items: Vec<ListItem<'_>> = if self.blocks.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "(no transcript yet — waiting for first event)",
                Style::default().fg(Color::DarkGray),
            )))]
        } else {
            self.blocks
                .iter()
                .flat_map(|block| block_to_list_items(block, kb))
                .collect()
        };

        // When auto-scroll is on, we implicitly want the tail visible.
        let visible = area.height.saturating_sub(2) as usize;
        let shown: Vec<ListItem<'_>> = if self.auto_scroll && items.len() > visible {
            items.into_iter().rev().take(visible).rev().collect()
        } else if !self.auto_scroll && self.scroll_offset > 0 {
            let skip = (self.scroll_offset as usize).min(items.len());
            items.into_iter().skip(skip).collect()
        } else {
            items
        };

        let list = List::new(shown).block(make_block("Transcript", transcript_focused));
        frame.render_widget(list, area);
    }

    fn render_composer(&self, frame: &mut Frame, area: Rect) {
        let composer_focused = self.focus == ComposerFocus::Composer;
        let border_style = if composer_focused {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = TuiBlock::default()
            .title("Composer — Ctrl+Enter to send")
            .borders(Borders::ALL)
            .border_style(border_style);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(&self.composer, inner);
    }
}

/// Render one [`Block`] into a sequence of [`ListItem`]s.  Kept free
/// (rather than a method) so leaf beads can wrap their own variants
/// without re-borrowing `&self`.
fn block_to_list_items(block: &Block, kb: &TuiKeybindings) -> Vec<ListItem<'static>> {
    match block {
        Block::TurnSeparator => vec![
            ListItem::new(Line::from(Span::styled(
                "─".repeat(60),
                Style::default().fg(Color::DarkGray),
            ))),
        ],
        // J.1.4: Approval blocks get rich inline rendering with action hints.
        Block::Approval {
            tool,
            reason,
            status,
            ..
        } => render_approval_block(tool, reason, *status, kb),
        // J.2.1: Task blocks get a dedicated renderer so the folded card is
        // styled distinctly (Magenta) and can be extended by J.2.2/J.2.3.
        Block::Task {
            agent,
            summary,
            expanded,
            child_thread,
            ..
        } => render_task_block(agent, summary, *expanded, child_thread),
        Block::ToolCall { .. } | Block::Error { .. } => {
            let header_color = match block {
                Block::ToolCall { .. } => Color::Yellow,
                Block::Error { .. } => Color::Red,
                _ => unreachable!(),
            };
            let mut items = Vec::new();
            for line in block.plain_text().lines() {
                items.push(ListItem::new(Line::from(Span::styled(
                    line.to_owned(),
                    Style::default().fg(header_color),
                ))));
            }
            items.push(ListItem::new(Line::from("")));
            items
        }
        Block::UserMessage { text } | Block::AssistantMessage { text, .. } => {
            let role = block.role_label();
            let role_color = match role {
                "user" => Color::Cyan,
                "assistant" => Color::Green,
                _ => Color::White,
            };
            let header = Line::from(vec![Span::styled(
                format!("[{role}]"),
                Style::default()
                    .fg(role_color)
                    .add_modifier(Modifier::BOLD),
            )]);
            let mut items = vec![ListItem::new(header)];
            // J.1.2 (sera-jie3): assistant messages may contain fenced
            // code blocks; route through `highlight::render_message` so
            // syntect colours each token.  User messages keep the
            // verbatim path — they're operator input and don't carry
            // styled markup.
            match block {
                Block::AssistantMessage { .. } => {
                    for line in highlight::render_message(text) {
                        items.push(ListItem::new(line));
                    }
                }
                _ => {
                    for body_line in text.lines() {
                        items.push(ListItem::new(Line::from(Span::raw(body_line.to_owned()))));
                    }
                }
            }
            items.push(ListItem::new(Line::from("")));
            items
        }
    }
}

/// Render an inline [`Block::Approval`] as styled list items.
///
/// Pending blocks show a warning header and a coloured action-hint line:
/// ```text
/// ⚠ Approval required: Write — write to /tmp/foo
///   [a]pprove  [r]eject  [e]scalate
/// ```
/// Resolved blocks show a single status line with matching colour.
fn render_approval_block(tool: &str, reason: &str, status: ApprovalStatus, kb: &TuiKeybindings) -> Vec<ListItem<'static>> {
    match status {
        ApprovalStatus::Pending => {
            let header = ListItem::new(Line::from(vec![
                Span::styled(
                    "⚠ Approval required: ".to_owned(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    tool.to_owned(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" — {reason}"),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
            let hint = ListItem::new(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("[{}]pprove", display_first(&kb.approve)),
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("[{}]eject", display_first(&kb.reject)),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("[{}]scalate", display_first(&kb.escalate)),
                    Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                ),
            ]));
            vec![header, hint, ListItem::new(Line::from(""))]
        }
        ApprovalStatus::Approved => vec![
            ListItem::new(Line::from(Span::styled(
                format!("✓ Approved: {tool}"),
                Style::default().fg(Color::Green),
            ))),
            ListItem::new(Line::from("")),
        ],
        ApprovalStatus::Rejected => vec![
            ListItem::new(Line::from(Span::styled(
                format!("✗ Rejected: {tool}"),
                Style::default().fg(Color::Red),
            ))),
            ListItem::new(Line::from("")),
        ],
        ApprovalStatus::Escalated => vec![
            ListItem::new(Line::from(Span::styled(
                format!("↑ Escalated: {tool}"),
                Style::default().fg(Color::Magenta),
            ))),
            ListItem::new(Line::from("")),
        ],
    }
}

/// Render a collapsed or expanded [`Block::Task`] as styled list items.
///
/// Collapsed (J.2.1):
/// ```text
/// ⏺ Task(agent-name: initial summary) ▸
/// ```
/// Expanded (future — J.2.2 sets `expanded=true`):
/// ```text
/// ⏺ Task(agent-name: initial summary) ▾
///   (child thread blocks rendered inline — wired by J.2.2)
/// ```
fn render_task_block(
    agent: &str,
    summary: &str,
    expanded: bool,
    _child_thread: &ThreadView,
) -> Vec<ListItem<'static>> {
    let arrow = if expanded { "▾" } else { "▸" };
    let label = format!("⏺ Task({agent}: {summary}) {arrow}");
    let item = ListItem::new(Line::from(Span::styled(
        label,
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    )));
    vec![item, ListItem::new(Line::from(""))]
}

impl Default for SessionView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sess(id: &str) -> SessionSummary {
        SessionSummary {
            id: id.to_owned(),
            agent_id: "agent-1".to_owned(),
            created_at: "2026-04-18T00:00:00Z".to_owned(),
            state: "active".to_owned(),
        }
    }

    #[test]
    fn apply_event_appends_delta_to_last_assistant_block() {
        let mut v = SessionView::new();
        v.set_session(sess("s1"));
        v.apply_event(StreamEvent {
            event_type: "message".into(),
            session_id: "s1".into(),
            role: "assistant".into(),
            delta: "hello ".into(),
            tool: String::new(),
            parent_task_id: None,
        });
        v.apply_event(StreamEvent {
            event_type: "message".into(),
            session_id: "s1".into(),
            role: "assistant".into(),
            delta: "world".into(),
            tool: String::new(),
            parent_task_id: None,
        });
        assert_eq!(v.blocks.len(), 1);
        match &v.blocks[0] {
            Block::AssistantMessage { text, streaming } => {
                assert_eq!(text, "hello world");
                assert!(streaming);
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        }
    }

    #[test]
    fn apply_event_pushes_new_block_on_role_change() {
        let mut v = SessionView::new();
        v.apply_event(StreamEvent {
            event_type: "message".into(),
            session_id: String::new(),
            role: "user".into(),
            delta: "ping".into(),
            tool: String::new(),
            parent_task_id: None,
        });
        v.apply_event(StreamEvent {
            event_type: "message".into(),
            session_id: String::new(),
            role: "assistant".into(),
            delta: "pong".into(),
            tool: String::new(),
            parent_task_id: None,
        });
        assert_eq!(v.blocks.len(), 2);
        assert!(matches!(&v.blocks[0], Block::UserMessage { text } if text == "ping"));
        assert!(matches!(
            &v.blocks[1],
            Block::AssistantMessage { text, .. } if text == "pong"
        ));
    }

    #[test]
    fn apply_event_tool_start_pushes_collapsed_tool_block() {
        let mut v = SessionView::new();
        v.apply_event(StreamEvent {
            event_type: "tool_start".into(),
            session_id: String::new(),
            role: String::new(),
            delta: "args".into(),
            tool: "bash".into(),
            parent_task_id: None,
        });
        assert_eq!(v.blocks.len(), 1);
        match &v.blocks[0] {
            Block::ToolCall {
                tool,
                expanded,
                result,
                ..
            } => {
                assert_eq!(tool, "bash");
                assert!(!*expanded, "tool blocks must be collapsed by default");
                assert!(result.is_none(), "result populated only on tool_end");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn apply_event_tool_end_attaches_result_to_matching_call() {
        let mut v = SessionView::new();
        v.apply_event(StreamEvent {
            event_type: "tool_start".into(),
            session_id: String::new(),
            role: String::new(),
            delta: "args".into(),
            tool: "bash".into(),
            parent_task_id: None,
        });
        v.apply_event(StreamEvent {
            event_type: "tool_end".into(),
            session_id: String::new(),
            role: String::new(),
            delta: "exit 0".into(),
            tool: "bash".into(),
            parent_task_id: None,
        });
        assert_eq!(v.blocks.len(), 1, "tool_end must NOT push a new block");
        match &v.blocks[0] {
            Block::ToolCall { result, .. } => {
                let r = result.as_ref().expect("result attached");
                assert!(r.ok);
                assert_eq!(r.output, "exit 0");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn apply_event_intermediate_tool_update_does_not_push_new_block() {
        // tool_start → tool (×2) → tool_end must leave exactly one ToolCall block.
        let mut v = SessionView::new();
        v.apply_event(StreamEvent {
            event_type: "tool_start".into(),
            session_id: String::new(),
            role: String::new(),
            delta: "{\"path\":".into(),
            tool: "Write".into(),
            parent_task_id: None,
        });
        v.apply_event(StreamEvent {
            event_type: "tool".into(),
            session_id: String::new(),
            role: String::new(),
            delta: "\"poem.md\"}".into(),
            tool: "Write".into(),
            parent_task_id: None,
        });
        // Block count must still be 1, not 2.
        assert_eq!(v.blocks.len(), 1, "intermediate tool frame must NOT push a new block");
        match &v.blocks[0] {
            Block::ToolCall { tool, summary, result, .. } => {
                assert_eq!(tool, "Write");
                // Delta from tool_start and intermediate tool are both in the summary.
                assert!(summary.contains("{\"path\":"), "initial delta preserved");
                assert!(summary.contains("\"poem.md\"}"), "intermediate delta appended");
                assert!(result.is_none(), "result not yet attached");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn scroll_up_disables_auto_scroll() {
        let mut v = SessionView::new();
        assert!(v.auto_scroll);
        v.scroll_up();
        assert!(!v.auto_scroll);
    }

    #[test]
    fn jump_to_end_rearms_auto_scroll() {
        let mut v = SessionView::new();
        v.scroll_up();
        v.jump_to_end();
        assert!(v.auto_scroll);
        assert_eq!(v.scroll_offset, 0);
    }

    #[test]
    fn empty_delta_event_is_no_op_on_blocks() {
        let mut v = SessionView::new();
        let updated = v.apply_event(StreamEvent {
            event_type: "keepalive".into(),
            session_id: String::new(),
            role: "assistant".into(),
            delta: String::new(),
            tool: String::new(),
            parent_task_id: None,
        });
        assert!(!updated);
        assert!(v.blocks.is_empty());
    }

    #[test]
    fn finalize_turn_clears_streaming_flag() {
        let mut v = SessionView::new();
        v.apply_event(StreamEvent {
            event_type: "message".into(),
            session_id: String::new(),
            role: "assistant".into(),
            delta: "answer".into(),
            tool: String::new(),
            parent_task_id: None,
        });
        match &v.blocks[0] {
            Block::AssistantMessage { streaming, .. } => assert!(streaming),
            _ => panic!(),
        }
        v.finalize_turn();
        match &v.blocks[0] {
            Block::AssistantMessage { streaming, .. } => assert!(!streaming),
            _ => panic!(),
        }
    }

    #[test]
    fn set_transcript_maps_wire_entries_to_blocks() {
        let mut v = SessionView::new();
        v.set_transcript(vec![
            TranscriptEntry {
                role: "user".into(),
                text: "q".into(),
            },
            TranscriptEntry {
                role: "assistant".into(),
                text: "a".into(),
            },
        ]);
        assert_eq!(v.blocks.len(), 2);
        assert!(matches!(&v.blocks[0], Block::UserMessage { text } if text == "q"));
        assert!(matches!(
            &v.blocks[1],
            Block::AssistantMessage { text, streaming } if text == "a" && !*streaming
        ));
    }

    #[test]
    fn set_transcript_preserves_tool_role_as_tool_call_block() {
        // A persisted "tool" entry in GET history must not be silently
        // downgraded to AssistantMessage (P2 regression fix).
        let mut v = SessionView::new();
        v.set_transcript(vec![TranscriptEntry {
            role: "tool".into(),
            text: "exit 0".into(),
        }]);
        assert_eq!(v.blocks.len(), 1);
        assert!(
            matches!(&v.blocks[0], Block::ToolCall { summary, .. } if summary == "exit 0"),
            "tool-role entry must map to ToolCall, got {:?}",
            &v.blocks[0]
        );
    }

    #[test]
    fn push_user_message_helper_appends_block() {
        let mut v = SessionView::new();
        v.push_user_message("hi".into());
        assert_eq!(v.blocks.len(), 1);
        assert!(matches!(&v.blocks[0], Block::UserMessage { text } if text == "hi"));
    }

    #[test]
    fn push_error_helper_appends_error_block() {
        let mut v = SessionView::new();
        v.push_error("boom".into(), true);
        assert!(matches!(
            &v.blocks[0],
            Block::Error { message, retryable } if message == "boom" && *retryable
        ));
    }

    #[test]
    fn push_approval_helper_starts_pending() {
        let mut v = SessionView::new();
        v.push_approval("h1".into(), "Write".into(), "needs ack".into());
        match &v.blocks[0] {
            Block::Approval {
                request_id,
                tool,
                reason,
                status,
            } => {
                assert_eq!(request_id, "h1");
                assert_eq!(tool, "Write");
                assert_eq!(reason, "needs ack");
                assert_eq!(*status, ApprovalStatus::Pending);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn push_turn_separator_appends_marker() {
        let mut v = SessionView::new();
        v.push_turn_separator();
        assert!(matches!(v.blocks[0], Block::TurnSeparator));
    }

    // --- Composer-specific tests (G.0.1) ---

    #[test]
    fn composer_starts_focused_and_empty() {
        let v = SessionView::new();
        assert_eq!(v.focus, ComposerFocus::Composer);
        assert!(v.composer.lines().iter().all(|l| l.is_empty()));
        assert!(v.pending_sends.is_empty());
    }

    #[test]
    fn composer_toggle_focus_switches_active_pane() {
        let mut v = SessionView::new();
        assert_eq!(v.focus, ComposerFocus::Composer);
        v.toggle_focus();
        assert_eq!(v.focus, ComposerFocus::Transcript);
        v.toggle_focus();
        assert_eq!(v.focus, ComposerFocus::Composer);
    }

    #[test]
    fn submit_drains_composer_into_pending_sends() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut v = SessionView::new();
        for ch in "hello".chars() {
            v.input_to_composer(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert!(!v.composer.lines().join("").is_empty());

        v.submit_composer();

        assert_eq!(v.pending_sends.len(), 1);
        assert_eq!(v.pending_sends[0], "hello");
        assert!(v.composer.lines().iter().all(|l| l.is_empty()));
    }

    #[test]
    fn submit_with_empty_buffer_is_noop() {
        let mut v = SessionView::new();
        v.submit_composer();
        assert!(v.pending_sends.is_empty());
    }

    // --- Slash command routing tests (G.1.1) ---

    #[test]
    fn slash_text_routes_to_pending_slash_not_sends() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut v = SessionView::new();
        for ch in "/new".chars() {
            v.input_to_composer(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        v.submit_composer();

        assert_eq!(v.pending_slash.len(), 1);
        assert_eq!(v.pending_slash[0], "/new");
        assert!(v.pending_sends.is_empty());
    }

    #[test]
    fn plain_text_routes_to_pending_sends_not_slash() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut v = SessionView::new();
        for ch in "hello".chars() {
            v.input_to_composer(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        v.submit_composer();

        assert_eq!(v.pending_sends.len(), 1);
        assert!(v.pending_slash.is_empty());
    }

    // --- Bracketed-paste tests (G.2.1) ---

    #[test]
    fn paste_short_content_renders_inline_unchanged() {
        let mut v = SessionView::new();
        v.handle_paste("line one\nline two\nline three".to_owned());
        assert!(v.composer_full_text.is_none());
        let displayed = v.composer.lines().join("\n");
        assert!(displayed.contains("line one"));
    }

    #[test]
    fn paste_long_content_renders_collapsed_token() {
        let mut v = SessionView::new();
        let content = "a\nb\nc\nd\ne\nf".to_owned();
        v.handle_paste(content.clone());
        assert_eq!(v.composer_full_text.as_deref(), Some(content.as_str()));
        let displayed = v.composer.lines().join("\n");
        assert!(displayed.contains("[6-line paste]"));
    }

    #[test]
    fn submit_returns_full_paste_not_collapsed() {
        let mut v = SessionView::new();
        let long_paste = (0..6).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        v.handle_paste(long_paste.clone());
        v.submit_composer();
        assert_eq!(v.pending_sends.len(), 1);
        assert_eq!(v.pending_sends[0], long_paste);
    }

    // --- J.1.4: inline HITL approval block tests ---

    #[test]
    fn approval_required_sse_event_pushes_approval_block() {
        let mut v = SessionView::new();
        let updated = v.apply_event(StreamEvent {
            event_type: "approval_required".into(),
            session_id: "req-42".into(),
            role: String::new(),
            delta: "write /tmp/secret".into(),
            tool: "Write".into(),
            parent_task_id: None,
        });
        assert!(updated);
        assert_eq!(v.blocks.len(), 1);
        match &v.blocks[0] {
            Block::Approval {
                request_id,
                tool,
                reason,
                status,
            } => {
                assert_eq!(request_id, "req-42");
                assert_eq!(tool, "Write");
                assert_eq!(reason, "write /tmp/secret");
                assert_eq!(*status, ApprovalStatus::Pending);
            }
            other => panic!("expected Approval block, got {other:?}"),
        }
    }

    #[test]
    fn first_pending_approval_id_returns_pending_block_id() {
        let mut v = SessionView::new();
        v.push_approval("req-1".into(), "Bash".into(), "run cmd".into());
        assert_eq!(v.first_pending_approval_id(), Some("req-1"));
    }

    #[test]
    fn first_pending_approval_id_returns_none_when_resolved() {
        let mut v = SessionView::new();
        v.push_approval("req-1".into(), "Bash".into(), "run cmd".into());
        v.update_approval_status("req-1", ApprovalStatus::Approved);
        assert_eq!(v.first_pending_approval_id(), None);
    }

    #[test]
    fn update_approval_status_changes_block_status() {
        let mut v = SessionView::new();
        v.push_approval("req-5".into(), "Write".into(), "reason".into());
        assert_eq!(v.first_pending_approval_id(), Some("req-5"));
        v.update_approval_status("req-5", ApprovalStatus::Rejected);
        assert_eq!(v.first_pending_approval_id(), None);
        match &v.blocks[0] {
            Block::Approval { status, .. } => assert_eq!(*status, ApprovalStatus::Rejected),
            other => panic!("expected Approval, got {other:?}"),
        }
    }

    #[test]
    fn approval_block_plain_text_pending_shows_action_hints() {
        let mut v = SessionView::new();
        v.push_approval("r1".into(), "Bash".into(), "run rm -rf".into());
        let text = v.blocks[0].plain_text();
        assert!(text.contains("[a]pprove"), "missing approve hint in: {text}");
        assert!(text.contains("[r]eject"), "missing reject hint in: {text}");
        assert!(text.contains("[e]scalate"), "missing escalate hint in: {text}");
    }

    #[test]
    fn approval_block_plain_text_resolved_shows_status() {
        let mut v = SessionView::new();
        v.push_approval("r1".into(), "Bash".into(), "cmd".into());
        v.update_approval_status("r1", ApprovalStatus::Approved);
        let text = v.blocks[0].plain_text();
        assert!(text.contains("Approved"), "expected Approved in: {text}");
        assert!(!text.contains("[a]pprove"), "should not show hints when resolved: {text}");
    }

    // --- J.2.1: Task block widget tests ---

    #[test]
    fn push_task_creates_collapsed_task_block() {
        let mut v = SessionView::new();
        v.push_task("task-1".into(), "coder-agent".into(), "write poem.md".into());
        assert_eq!(v.blocks.len(), 1);
        match &v.blocks[0] {
            Block::Task {
                task_id,
                agent,
                summary,
                expanded,
                child_thread,
            } => {
                assert_eq!(task_id, "task-1");
                assert_eq!(agent, "coder-agent");
                assert_eq!(summary, "write poem.md");
                assert!(!expanded, "task blocks must start collapsed");
                assert!(child_thread.blocks.is_empty(), "child thread starts empty");
            }
            other => panic!("expected Task block, got {other:?}"),
        }
    }

    #[test]
    fn subagent_start_event_pushes_task_block() {
        let mut v = SessionView::new();
        let updated = v.apply_event(StreamEvent {
            event_type: "subagent_start".into(),
            session_id: "task-42".into(),
            role: String::new(),
            delta: "write the report".into(),
            tool: "writer-agent".into(),
            parent_task_id: None,
        });
        assert!(updated);
        assert_eq!(v.blocks.len(), 1);
        match &v.blocks[0] {
            Block::Task {
                task_id,
                agent,
                summary,
                expanded,
                ..
            } => {
                assert_eq!(task_id, "task-42");
                assert_eq!(agent, "writer-agent");
                assert_eq!(summary, "write the report");
                assert!(!expanded);
            }
            other => panic!("expected Task block, got {other:?}"),
        }
    }

    #[test]
    fn task_block_plain_text_shows_collapsed_arrow() {
        let mut v = SessionView::new();
        v.push_task("t1".into(), "coder".into(), "fix bug".into());
        let text = v.blocks[0].plain_text();
        assert!(text.contains("Task(coder: fix bug)"), "missing task label in: {text}");
        assert!(text.contains("▸"), "collapsed task must show ▸ in: {text}");
        assert!(!text.contains("▾"), "collapsed task must not show ▾ in: {text}");
    }

    #[test]
    fn task_block_toggle_expanded_flips_flag() {
        let mut v = SessionView::new();
        v.push_task("t1".into(), "coder".into(), "fix bug".into());
        assert_eq!(v.blocks[0].toggle_expanded(), Some(true));
        let text = v.blocks[0].plain_text();
        assert!(text.contains("▾"), "expanded task must show ▾ in: {text}");
    }

    #[test]
    fn task_block_task_id_accessor() {
        let mut v = SessionView::new();
        v.push_task("route-key-99".into(), "agent".into(), "do work".into());
        assert_eq!(v.blocks[0].task_id(), Some("route-key-99"));
    }

    #[test]
    fn stream_event_parse_extracts_parent_task_id() {
        use crate::client::StreamEvent;
        let raw = r#"{"event_type":"message","delta":"hello","parent_task_id":"task-7"}"#;
        let ev = StreamEvent::parse(raw).expect("should parse");
        assert_eq!(ev.parent_task_id.as_deref(), Some("task-7"));
    }

    #[test]
    fn stream_event_parse_parent_task_id_absent_is_none() {
        use crate::client::StreamEvent;
        let raw = r#"{"event_type":"message","delta":"hello"}"#;
        let ev = StreamEvent::parse(raw).expect("should parse");
        assert_eq!(ev.parent_task_id, None);
    }
}
