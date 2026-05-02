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
use super::blocks::{ApprovalStatus, Block, ToolResult};
use crate::client::{ConnectionState, SessionSummary, StreamEvent, TranscriptEntry};

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

    /// Push an inline approval block.  Wired by J.1.4 (sera-7olp).
    #[allow(dead_code)] // J.1.4 will wire this
    pub fn push_approval(&mut self, request_id: String, tool: String, reason: String) {
        self.blocks.push(Block::Approval {
            request_id,
            tool,
            reason,
            status: ApprovalStatus::Pending,
        });
    }

    /// Push a turn separator.  Renderers use this to draw a thin rule
    /// between turns.
    #[allow(dead_code)] // J.0.4 will wire this on turn_completed
    pub fn push_turn_separator(&mut self) {
        self.blocks.push(Block::TurnSeparator);
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
    pub fn render_chat(&self, frame: &mut Frame, area: Rect, focused: bool) {
        self.render_transcript(frame, area, focused);
    }

    /// **J.0.1**: render only the composer into `area`.
    pub fn render_composer_only(&self, frame: &mut Frame, area: Rect) {
        self.render_composer(frame, area);
    }

    fn render_transcript(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let transcript_focused = focused && self.focus == ComposerFocus::Transcript;
        let items: Vec<ListItem<'_>> = if self.blocks.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "(no transcript yet — waiting for first event)",
                Style::default().fg(Color::DarkGray),
            )))]
        } else {
            self.blocks
                .iter()
                .flat_map(|block| block_to_list_items(block))
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
fn block_to_list_items(block: &Block) -> Vec<ListItem<'static>> {
    match block {
        Block::TurnSeparator => vec![
            ListItem::new(Line::from(Span::styled(
                "─".repeat(60),
                Style::default().fg(Color::DarkGray),
            ))),
        ],
        Block::ToolCall { .. } | Block::Task { .. } | Block::Approval { .. } | Block::Error { .. } => {
            let header_color = match block {
                Block::ToolCall { .. } => Color::Yellow,
                Block::Task { .. } => Color::Magenta,
                Block::Approval { .. } => Color::Yellow,
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
            for body_line in text.lines() {
                items.push(ListItem::new(Line::from(Span::raw(body_line.to_owned()))));
            }
            items.push(ListItem::new(Line::from("")));
            items
        }
    }
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
        });
        v.apply_event(StreamEvent {
            event_type: "message".into(),
            session_id: "s1".into(),
            role: "assistant".into(),
            delta: "world".into(),
            tool: String::new(),
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
        });
        v.apply_event(StreamEvent {
            event_type: "message".into(),
            session_id: String::new(),
            role: "assistant".into(),
            delta: "pong".into(),
            tool: String::new(),
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
        });
        v.apply_event(StreamEvent {
            event_type: "tool_end".into(),
            session_id: String::new(),
            role: String::new(),
            delta: "exit 0".into(),
            tool: "bash".into(),
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
        });
        v.apply_event(StreamEvent {
            event_type: "tool".into(),
            session_id: String::new(),
            role: String::new(),
            delta: "\"poem.md\"}".into(),
            tool: "Write".into(),
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
}
