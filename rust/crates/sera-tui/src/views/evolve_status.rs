//! Evolve proposal status view — read-only for sera-m41j.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use super::agent_list::make_block;
use crate::client::EvolveProposal;

pub struct EvolveStatusView {
    proposals: Vec<EvolveProposal>,
    state: TableState,
}

impl EvolveStatusView {
    pub fn new() -> Self {
        let mut state = TableState::default();
        state.select(Some(0));
        Self {
            proposals: Vec::new(),
            state,
        }
    }

    pub fn set_proposals(&mut self, p: Vec<EvolveProposal>) {
        self.proposals = p;
        let sel = if self.proposals.is_empty() { None } else { Some(0) };
        self.state.select(sel);
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.proposals.is_empty()
    }

    pub fn up(&mut self) {
        let i = self.state.selected().unwrap_or(0);
        if i > 0 {
            self.state.select(Some(i - 1));
        }
    }

    pub fn down(&mut self) {
        if self.proposals.is_empty() {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        if i + 1 < self.proposals.len() {
            self.state.select(Some(i + 1));
        }
    }

    fn state_style(s: &str) -> Style {
        match s.to_ascii_lowercase().as_str() {
            "pending" | "proposed" => Style::default().fg(Color::Yellow),
            "approved" | "applied" => Style::default().fg(Color::Green),
            "rejected" | "failed" => Style::default().fg(Color::Red),
            _ => Style::default().fg(Color::White),
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        if self.proposals.is_empty() {
            let p = Paragraph::new("No evolve proposals.")
                .style(Style::default().fg(Color::DarkGray))
                .block(make_block("Evolve proposals", focused));
            frame.render_widget(p, area);
            return;
        }

        let rows = self.proposals.iter().map(|p| {
            Row::new(vec![
                Cell::from(truncate(&p.id, 10)),
                Cell::from(truncate(&p.proposer, 14)),
                Cell::from(truncate(&p.target, 14)),
                Cell::from(p.state.clone()).style(Self::state_style(&p.state)),
                Cell::from(truncate(&p.age, 22)),
            ])
        });

        let table = Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Length(16),
                Constraint::Length(16),
                Constraint::Length(12),
                Constraint::Length(24),
            ],
        )
        .header(
            Row::new(vec!["ID", "PROPOSER", "TARGET", "STATE", "AGE"])
                .style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)),
        )
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .block(make_block("Evolve proposals", focused));

        frame.render_stateful_widget(table, area, &mut self.state);
    }
}

impl Default for EvolveStatusView {
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

    fn p(id: &str, state: &str) -> EvolveProposal {
        EvolveProposal {
            id: id.to_owned(),
            proposer: "agent-a".to_owned(),
            target: "agent-b".to_owned(),
            state: state.to_owned(),
            age: "2026-04-18T10:00:00Z".to_owned(),
        }
    }

    #[test]
    fn new_view_is_empty() {
        let v = EvolveStatusView::new();
        assert!(v.is_empty());
    }

    #[test]
    fn set_proposals_resets_selection() {
        let mut v = EvolveStatusView::new();
        v.set_proposals(vec![p("p1", "pending"), p("p2", "approved")]);
        assert!(!v.is_empty());
        // Selection lands on first row
        assert_eq!(v.state.selected(), Some(0));
    }

    #[test]
    fn navigation_bounds() {
        let mut v = EvolveStatusView::new();
        v.set_proposals(vec![p("1", "pending"), p("2", "approved"), p("3", "failed")]);
        v.up();
        assert_eq!(v.state.selected(), Some(0));
        v.down();
        v.down();
        v.down();
        assert_eq!(v.state.selected(), Some(2));
    }

    #[test]
    fn state_style_reflects_keyword() {
        assert_eq!(
            EvolveStatusView::state_style("pending").fg,
            Some(Color::Yellow)
        );
        assert_eq!(
            EvolveStatusView::state_style("approved").fg,
            Some(Color::Green)
        );
        assert_eq!(
            EvolveStatusView::state_style("rejected").fg,
            Some(Color::Red)
        );
    }
}
