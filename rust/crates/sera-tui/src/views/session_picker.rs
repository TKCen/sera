//! Session picker modal — centered overlay listing sessions for the active agent.
//!
//! Triggered by `open_session_picker` (default: Ctrl+P).  Arrow keys navigate,
//! Enter resumes the chosen session, Esc closes.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;

use crate::client::SessionSummary;

/// Modal overlay listing all sessions for the active agent.
pub struct SessionPickerView {
    pub sessions: Vec<SessionSummary>,
    pub cursor: usize,
}

impl SessionPickerView {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            cursor: 0,
        }
    }

    pub fn set_sessions(&mut self, s: Vec<SessionSummary>) {
        self.sessions = s;
        self.cursor = 0;
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.sessions.is_empty() && self.cursor + 1 < self.sessions.len() {
            self.cursor += 1;
        }
    }

    pub fn selected(&self) -> Option<&SessionSummary> {
        self.sessions.get(self.cursor)
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let modal = centered_rect(60, 50, area);

        // Clear the background so the modal overlays cleanly.
        frame.render_widget(Clear, modal);

        if self.sessions.is_empty() {
            let p = ratatui::widgets::Paragraph::new("No sessions found for this agent.")
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .title(" Sessions (Ctrl+P) ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan)),
                );
            frame.render_widget(p, modal);
            return;
        }

        let items: Vec<ListItem<'_>> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let short_id = if s.id.len() > 12 {
                    format!("{}…", &s.id[..11])
                } else {
                    s.id.clone()
                };
                let created = if s.created_at.len() >= 10 {
                    s.created_at[..10].to_owned()
                } else if s.created_at.is_empty() {
                    "—".to_owned()
                } else {
                    s.created_at.clone()
                };
                let focused = i == self.cursor;
                let style = if focused {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(vec![Span::styled(
                    format!(" {:<14} {}  [{}]", short_id, created, s.state),
                    style,
                )]))
            })
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(self.cursor));

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Sessions — ↑/↓ navigate  Enter:resume  Esc:close ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_stateful_widget(list, modal, &mut list_state);
    }
}

impl Default for SessionPickerView {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate a centered [`Rect`] that takes `percent_x`% of width and
/// `percent_y`% of height from `r`.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let w = r.width * percent_x / 100;
    let h = r.height * percent_y / 100;
    let x = r.x + (r.width.saturating_sub(w)) / 2;
    let y = r.y + (r.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w.max(1), h.max(1))
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
    fn new_has_empty_sessions_and_zero_cursor() {
        let p = SessionPickerView::new();
        assert!(p.sessions.is_empty());
        assert_eq!(p.cursor, 0);
        assert!(p.selected().is_none());
    }

    #[test]
    fn set_sessions_resets_cursor() {
        let mut p = SessionPickerView::new();
        p.set_sessions(vec![sess("s1"), sess("s2"), sess("s3")]);
        p.move_down();
        p.move_down();
        assert_eq!(p.cursor, 2);
        p.set_sessions(vec![sess("s4")]);
        assert_eq!(p.cursor, 0);
    }

    #[test]
    fn move_up_at_top_is_noop() {
        let mut p = SessionPickerView::new();
        p.set_sessions(vec![sess("s1"), sess("s2")]);
        p.move_up(); // already at 0
        assert_eq!(p.cursor, 0);
    }

    #[test]
    fn move_down_at_bottom_is_noop() {
        let mut p = SessionPickerView::new();
        p.set_sessions(vec![sess("s1"), sess("s2")]);
        p.move_down(); // -> 1
        p.move_down(); // at bottom
        assert_eq!(p.cursor, 1);
    }

    #[test]
    fn selected_returns_cursor_position() {
        let mut p = SessionPickerView::new();
        p.set_sessions(vec![sess("s1"), sess("s2"), sess("s3")]);
        assert_eq!(p.selected().map(|s| s.id.as_str()), Some("s1"));
        p.move_down();
        assert_eq!(p.selected().map(|s| s.id.as_str()), Some("s2"));
        p.move_down();
        assert_eq!(p.selected().map(|s| s.id.as_str()), Some("s3"));
    }

    #[test]
    fn centered_rect_fits_inside_parent() {
        let parent = Rect::new(0, 0, 100, 40);
        let modal = centered_rect(60, 50, parent);
        assert!(modal.x >= parent.x);
        assert!(modal.y >= parent.y);
        assert!(modal.right() <= parent.right());
        assert!(modal.bottom() <= parent.bottom());
    }
}
