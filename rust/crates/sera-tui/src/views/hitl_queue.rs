//! HITL queue view — list pending permission requests across agents.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use super::agent_list::make_block;
use crate::client::HitlRequest;

pub struct HitlQueueView {
    requests: Vec<HitlRequest>,
    state: TableState,
    /// Last error surfaced by an approve/reject/escalate call — displayed
    /// in the view footer so terminal-state rejections (per §4.25) don't
    /// panic the app.
    pub last_error: Option<String>,
}

impl HitlQueueView {
    pub fn new() -> Self {
        let mut state = TableState::default();
        state.select(Some(0));
        Self {
            requests: Vec::new(),
            state,
            last_error: None,
        }
    }

    pub fn set_requests(&mut self, r: Vec<HitlRequest>) {
        self.requests = r;
        let sel = if self.requests.is_empty() { None } else { Some(0) };
        self.state.select(sel);
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    pub fn selected_id(&self) -> Option<String> {
        self.state
            .selected()
            .and_then(|i| self.requests.get(i))
            .map(|r| r.id.clone())
    }

    pub fn up(&mut self) {
        let i = self.state.selected().unwrap_or(0);
        if i > 0 {
            self.state.select(Some(i - 1));
        }
    }

    pub fn down(&mut self) {
        if self.requests.is_empty() {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        if i + 1 < self.requests.len() {
            self.state.select(Some(i + 1));
        }
    }

    pub fn set_error(&mut self, e: impl Into<String>) {
        self.last_error = Some(e.into());
    }

    pub fn clear_error(&mut self) {
        self.last_error = None;
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        if self.requests.is_empty() {
            let p = Paragraph::new("No pending HITL requests.")
                .style(Style::default().fg(Color::Green))
                .block(make_block("HITL queue", focused));
            frame.render_widget(p, area);
            return;
        }

        let rows = self.requests.iter().map(|r| {
            let status_style = match r.status.as_str() {
                "pending" => Style::default().fg(Color::Yellow),
                "approved" => Style::default().fg(Color::Green),
                "denied" | "rejected" => Style::default().fg(Color::Red),
                _ => Style::default().fg(Color::White),
            };
            Row::new(vec![
                Cell::from(truncate(&r.id, 10)),
                Cell::from(truncate(&r.agent_id, 12)),
                Cell::from(truncate(&r.summary, 40)),
                Cell::from(truncate(&r.age, 20)),
                Cell::from(r.status.clone()).style(status_style),
            ])
        });

        let table = Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Length(14),
                Constraint::Min(30),
                Constraint::Length(22),
                Constraint::Length(10),
            ],
        )
        .header(
            Row::new(vec!["ID", "AGENT", "SUMMARY", "AGE", "STATUS"])
                .style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)),
        )
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .block(make_block("HITL queue", focused));

        frame.render_stateful_widget(table, area, &mut self.state);
    }
}

impl Default for HitlQueueView {
    fn default() -> Self {
        Self::new()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(id: &str) -> HitlRequest {
        HitlRequest {
            id: id.to_owned(),
            agent_id: "agent-a".to_owned(),
            summary: format!("req-{id}"),
            age: "2026-04-18T10:00:00Z".to_owned(),
            status: "pending".to_owned(),
        }
    }

    #[test]
    fn new_view_is_empty() {
        let v = HitlQueueView::new();
        assert!(v.is_empty());
        assert_eq!(v.selected_id(), None);
    }

    #[test]
    fn set_requests_selects_first() {
        let mut v = HitlQueueView::new();
        v.set_requests(vec![req("1"), req("2")]);
        assert_eq!(v.selected_id().as_deref(), Some("1"));
    }

    #[test]
    fn navigation_respects_bounds() {
        let mut v = HitlQueueView::new();
        v.set_requests(vec![req("1"), req("2"), req("3")]);
        v.up();
        assert_eq!(v.selected_id().as_deref(), Some("1"));
        v.down();
        v.down();
        v.down();
        assert_eq!(v.selected_id().as_deref(), Some("3"));
    }

    #[test]
    fn error_message_round_trips() {
        let mut v = HitlQueueView::new();
        v.set_error("invalid transition");
        assert_eq!(v.last_error.as_deref(), Some("invalid transition"));
        v.clear_error();
        assert!(v.last_error.is_none());
    }
}
