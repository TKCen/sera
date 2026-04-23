//! Session view — metadata header, streaming transcript, tool log.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};
use ratatui::Frame;

use super::agent_list::make_block;
use crate::client::{ConnectionState, SessionSummary, StreamEvent, TranscriptEntry};

/// Viewer state for a single session.  Owns:
/// * metadata (agent id, session id, state)
/// * transcript lines (seeded by GET, appended by SSE)
/// * tool events (SSE only, non-message events)
/// * scroll bookkeeping — auto-scrolls to tail unless the user has paused
pub struct SessionView {
    pub session: Option<SessionSummary>,
    pub transcript: Vec<TranscriptEntry>,
    pub tool_log: Vec<String>,
    pub scroll_offset: u16,
    pub auto_scroll: bool,
    pub conn: ConnectionState,
}

impl SessionView {
    pub fn new() -> Self {
        Self {
            session: None,
            transcript: Vec::new(),
            tool_log: Vec::new(),
            scroll_offset: 0,
            auto_scroll: true,
            conn: ConnectionState::Disconnected,
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

    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(7),
            ])
            .split(area);

        self.render_metadata(frame, chunks[0], focused);
        self.render_transcript(frame, chunks[1], focused);
        self.render_tool_log(frame, chunks[2], focused);
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
                    let header = Line::from(vec![
                        Span::styled(
                            format!("[{}]", entry.role),
                            Style::default()
                                .fg(role_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]);
                    let mut lines = vec![ListItem::new(header)];
                    for body_line in entry.text.lines() {
                        lines.push(ListItem::new(Line::from(Span::raw(
                            body_line.to_owned(),
                        ))));
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

        let list = List::new(shown).block(make_block("Transcript", focused));
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
                .map(|l| ListItem::new(Span::styled(l.clone(), Style::default().fg(Color::Yellow))))
                .collect()
        };
        let list = List::new(items).block(make_block("Tool events", focused));
        frame.render_widget(list, area);
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
}
