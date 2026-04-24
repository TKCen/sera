//! Session view — metadata header, streaming transcript, tool log, composer.

use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;
use tui_textarea::TextArea;

use super::agent_list::make_block;
use crate::client::{ConnectionState, SessionSummary, StreamEvent, TranscriptEntry};

/// Which pane inside the Session view holds keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerFocus {
    Composer,
    Transcript,
}

/// Viewer state for a single session.  Owns:
/// * metadata (agent id, session id, state)
/// * transcript lines (seeded by GET, appended by SSE)
/// * tool events (SSE only, non-message events)
/// * scroll bookkeeping — auto-scrolls to tail unless the user has paused
/// * a multi-line composer for drafting outgoing messages
pub struct SessionView {
    pub session: Option<SessionSummary>,
    pub transcript: Vec<TranscriptEntry>,
    pub tool_log: Vec<String>,
    pub scroll_offset: u16,
    pub auto_scroll: bool,
    pub conn: ConnectionState,
    pub composer: TextArea<'static>,
    pub focus: ComposerFocus,
    /// Messages drained from the composer via `submit_composer`.
    /// G.0.2 (sera-5d4k) will wire these to POST /api/chat.
    pub pending_sends: Vec<String>,
}

impl SessionView {
    pub fn new() -> Self {
        let mut composer = TextArea::default();
        composer.set_placeholder_text("Type a message…");
        Self {
            session: None,
            transcript: Vec::new(),
            tool_log: Vec::new(),
            scroll_offset: 0,
            auto_scroll: true,
            conn: ConnectionState::Disconnected,
            composer,
            focus: ComposerFocus::Composer,
            pending_sends: Vec::new(),
        }
    }

    pub fn set_session(&mut self, session: SessionSummary) {
        self.session = Some(session);
        self.transcript.clear();
        self.tool_log.clear();
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    pub fn set_transcript(&mut self, entries: Vec<TranscriptEntry>) {
        self.transcript = entries;
    }

    /// Apply a [`StreamEvent`] to the view state.  Returns true if the
    /// update is "display-worthy" (i.e. the app should rerender).
    pub fn apply_event(&mut self, ev: StreamEvent) -> bool {
        let et = ev.event_type.to_ascii_lowercase();
        if et == "tool" || et == "tool_start" || et == "tool_end" || !ev.tool.is_empty() {
            // Tool events land in the tool log pane, not the transcript.
            let line = if ev.delta.is_empty() {
                format!("[{}] {}", ev.event_type, ev.tool)
            } else {
                format!("[{}] {}: {}", ev.event_type, ev.tool, ev.delta)
            };
            self.tool_log.push(line);
            return true;
        }
        if ev.delta.is_empty() {
            return false;
        }
        // Append delta to the latest entry if the role matches; else push
        // a new entry.  This is the single-turn streaming assumption —
        // it matches the autonomous gateway's actual emit pattern.
        match self.transcript.last_mut() {
            Some(last) if last.role == ev.role => last.text.push_str(&ev.delta),
            _ => self.transcript.push(TranscriptEntry {
                role: if ev.role.is_empty() {
                    "assistant".to_owned()
                } else {
                    ev.role
                },
                text: ev.delta,
            }),
        }
        true
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
    /// Resets the textarea to empty.
    pub fn take_composer_text(&mut self) -> String {
        let text = self.composer.lines().join("\n");
        let mut fresh = TextArea::default();
        fresh.set_placeholder_text("Type a message…");
        self.composer = fresh;
        text
    }

    /// Submit the current composer buffer: drain text → push to
    /// `pending_sends` → log.  No-op when the buffer is blank.
    pub fn submit_composer(&mut self) {
        let text = self.take_composer_text();
        if text.trim().is_empty() {
            return;
        }
        tracing::info!(message = %text, "composer submit queued (pending G.0.2 wiring)");
        self.pending_sends.push(text);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // metadata header
                Constraint::Min(3),    // transcript
                Constraint::Length(7), // tool log
                Constraint::Length(5), // composer
            ])
            .split(area);

        self.render_metadata(frame, chunks[0], focused);
        self.render_transcript(frame, chunks[1], focused);
        self.render_tool_log(frame, chunks[2], focused);
        self.render_composer(frame, chunks[3]);
    }

    fn render_metadata(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let text = match &self.session {
            Some(s) => format!(
                "session={}  agent={}  state={}  conn={}",
                truncate_or_dash(&s.id, 12),
                truncate_or_dash(&s.agent_id, 12),
                s.state,
                self.conn.label()
            ),
            None => "No session selected — choose an agent and press Enter".to_owned(),
        };
        let p = Paragraph::new(text)
            .style(Style::default().fg(Color::White))
            .block(make_block("Session", focused));
        frame.render_widget(p, area);
    }

    fn render_transcript(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let transcript_focused = focused && self.focus == ComposerFocus::Transcript;
        let items: Vec<ListItem<'_>> = if self.transcript.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "(no transcript yet — waiting for first event)",
                Style::default().fg(Color::DarkGray),
            )))]
        } else {
            self.transcript
                .iter()
                .flat_map(|entry| {
                    let role_color = match entry.role.as_str() {
                        "user" => Color::Cyan,
                        "assistant" => Color::Green,
                        "system" => Color::Magenta,
                        "tool" => Color::Yellow,
                        _ => Color::White,
                    };
                    let header = Line::from(vec![Span::styled(
                        format!("[{}]", entry.role),
                        Style::default()
                            .fg(role_color)
                            .add_modifier(Modifier::BOLD),
                    )]);
                    let mut lines = vec![ListItem::new(header)];
                    for body_line in entry.text.lines() {
                        lines.push(ListItem::new(Line::from(Span::raw(body_line.to_owned()))));
                    }
                    lines.push(ListItem::new(Line::from("")));
                    lines
                })
                .collect()
        };

        // When auto-scroll is on, we implicitly want the tail visible.
        // ratatui's List doesn't support scroll-to-end natively on stateless
        // calls, so we take the last N items where N is the visible area.
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

    fn render_tool_log(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let items: Vec<ListItem<'_>> = if self.tool_log.is_empty() {
            vec![ListItem::new(Span::styled(
                "(no tool events)",
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            // Tail the latest 5 tool events — older ones fall off the
            // bottom pane; the operator can scroll the full session log
            // via the transcript.
            self.tool_log
                .iter()
                .rev()
                .take(5)
                .rev()
                .map(|l| {
                    ListItem::new(Span::styled(l.clone(), Style::default().fg(Color::Yellow)))
                })
                .collect()
        };
        let list = List::new(items).block(make_block("Tool events", focused));
        frame.render_widget(list, area);
    }

    fn render_composer(&self, frame: &mut Frame, area: Rect) {
        let composer_focused = self.focus == ComposerFocus::Composer;
        let border_style = if composer_focused {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        // Render the textarea widget into the inner area (inside the border).
        let block = Block::default()
            .title("Composer — Ctrl+Enter to send")
            .borders(Borders::ALL)
            .border_style(border_style);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(&self.composer, inner);
    }
}

impl Default for SessionView {
    fn default() -> Self {
        Self::new()
    }
}

fn truncate_or_dash(s: &str, max: usize) -> String {
    if s.is_empty() {
        "—".to_owned()
    } else if s.chars().count() <= max {
        s.to_owned()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
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
    fn apply_event_appends_delta_to_last_entry() {
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
        assert_eq!(v.transcript.len(), 1);
        assert_eq!(v.transcript[0].text, "hello world");
        assert_eq!(v.transcript[0].role, "assistant");
    }

    #[test]
    fn apply_event_pushes_new_entry_on_role_change() {
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
        assert_eq!(v.transcript.len(), 2);
        assert_eq!(v.transcript[0].role, "user");
        assert_eq!(v.transcript[1].role, "assistant");
    }

    #[test]
    fn apply_event_routes_tool_to_tool_log() {
        let mut v = SessionView::new();
        v.apply_event(StreamEvent {
            event_type: "tool_start".into(),
            session_id: String::new(),
            role: String::new(),
            delta: "args".into(),
            tool: "bash".into(),
        });
        assert_eq!(v.transcript.len(), 0);
        assert_eq!(v.tool_log.len(), 1);
        assert!(v.tool_log[0].contains("bash"));
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
    fn empty_delta_event_is_no_op_on_transcript() {
        let mut v = SessionView::new();
        let updated = v.apply_event(StreamEvent {
            event_type: "keepalive".into(),
            session_id: String::new(),
            role: "assistant".into(),
            delta: String::new(),
            tool: String::new(),
        });
        assert!(!updated);
        assert!(v.transcript.is_empty());
    }

    #[test]
    fn truncate_or_dash_behaves() {
        assert_eq!(truncate_or_dash("", 5), "—");
        assert_eq!(truncate_or_dash("abc", 5), "abc");
        assert_eq!(truncate_or_dash("abcdefgh", 5), "abcd…");
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
        // Type "hello" into the composer.
        for ch in "hello".chars() {
            v.input_to_composer(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert!(!v.composer.lines().join("").is_empty());

        v.submit_composer();

        assert_eq!(v.pending_sends.len(), 1);
        assert_eq!(v.pending_sends[0], "hello");
        // Composer should be empty after drain.
        assert!(v.composer.lines().iter().all(|l| l.is_empty()));
    }

    #[test]
    fn submit_with_empty_buffer_is_noop() {
        let mut v = SessionView::new();
        v.submit_composer();
        assert!(v.pending_sends.is_empty());
    }
}
