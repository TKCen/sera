//! Agent list view — one `ratatui::Table` with a selectable row.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use crate::client::Agent;

/// ratatui `Table` view over the agent list.
pub struct AgentListView {
    agents: Vec<Agent>,
    state: TableState,
}

impl AgentListView {
    pub fn new() -> Self {
        let mut state = TableState::default();
        state.select(Some(0));
        Self {
            agents: Vec::new(),
            state,
        }
    }

    pub fn set_agents(&mut self, agents: Vec<Agent>) {
        self.agents = agents;
        let sel = if self.agents.is_empty() { None } else { Some(0) };
        self.state.select(sel);
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    pub fn selected_agent(&self) -> Option<&Agent> {
        self.state.selected().and_then(|i| self.agents.get(i))
    }

    pub fn selected_id(&self) -> Option<String> {
        self.selected_agent().map(|a| a.id.clone())
    }

    pub fn up(&mut self) {
        let i = self.state.selected().unwrap_or(0);
        if i > 0 {
            self.state.select(Some(i - 1));
        }
    }

    pub fn down(&mut self) {
        if self.agents.is_empty() {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        if i + 1 < self.agents.len() {
            self.state.select(Some(i + 1));
        }
    }

    /// Best-effort ISO-8601 → "15m ago" conversion.  Falls back to the
    /// raw string when parsing fails so the column is never empty.
    fn render_heartbeat(v: Option<&String>) -> String {
        v.cloned().unwrap_or_else(|| "—".to_owned())
    }

    /// Colour the status column by keyword.
    fn status_style(status: &str) -> Style {
        let s = status.to_ascii_lowercase();
        if s.contains("running") || s.contains("active") || s.contains("ready") {
            Style::default().fg(Color::Green)
        } else if s.contains("pending") || s.contains("starting") || s.contains("queue") {
            Style::default().fg(Color::Yellow)
        } else if s.contains("error") || s.contains("failed") || s.contains("stopped") {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::White)
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        if self.agents.is_empty() {
            let p = Paragraph::new("No agents. Press 'r' to refresh.")
                .style(Style::default().fg(Color::Yellow))
                .block(make_block("Agents", focused));
            frame.render_widget(p, area);
            return;
        }

        let rows = self.agents.iter().map(|a| {
            let name = a
                .display_name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&a.name);
            Row::new(vec![
                Cell::from(truncate(&a.id, 12)),
                Cell::from(name.to_owned()),
                Cell::from(Span::styled(a.status.clone(), Self::status_style(&a.status))),
                Cell::from(a.template_or_provider.clone()),
                Cell::from(Self::render_heartbeat(a.last_heartbeat_at.as_ref())),
            ])
        });

        let table = Table::new(
            rows,
            [
                Constraint::Length(14),
                Constraint::Percentage(25),
                Constraint::Length(14),
                Constraint::Percentage(25),
                Constraint::Length(22),
            ],
        )
        .header(
            Row::new(vec!["ID", "NAME", "STATUS", "TEMPLATE/PROV", "LAST SEEN"])
                .style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)),
        )
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .block(make_block("Agents", focused));

        frame.render_stateful_widget(table, area, &mut self.state);
    }
}

impl Default for AgentListView {
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

/// Shared border block — highlights when the view has focus.
pub(crate) fn make_block(title: &str, focused: bool) -> Block<'_> {
    let border_color = if focused { Color::Cyan } else { Color::DarkGray };
    Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                title,
                Style::default().add_modifier(Modifier::BOLD).fg(border_color),
            ),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: &str) -> Agent {
        Agent {
            id: id.to_owned(),
            name: format!("name-{id}"),
            display_name: None,
            status: "running".to_owned(),
            template_or_provider: "tpl".to_owned(),
            last_heartbeat_at: None,
        }
    }

    #[test]
    fn new_view_is_empty() {
        let v = AgentListView::new();
        assert!(v.is_empty());
        assert_eq!(v.selected_id(), None);
    }

    #[test]
    fn set_agents_selects_first_row() {
        let mut v = AgentListView::new();
        v.set_agents(vec![agent("a"), agent("b")]);
        assert_eq!(v.selected_id().as_deref(), Some("a"));
    }

    #[test]
    fn down_up_clamps_at_boundaries() {
        let mut v = AgentListView::new();
        v.set_agents(vec![agent("a"), agent("b"), agent("c")]);
        v.up();
        assert_eq!(v.selected_id().as_deref(), Some("a"));
        v.down();
        v.down();
        assert_eq!(v.selected_id().as_deref(), Some("c"));
        v.down();
        assert_eq!(v.selected_id().as_deref(), Some("c"));
    }

    #[test]
    fn status_style_colors_by_keyword() {
        let s = AgentListView::status_style("running");
        assert_eq!(s.fg, Some(Color::Green));
        let s = AgentListView::status_style("error: boom");
        assert_eq!(s.fg, Some(Color::Red));
        let s = AgentListView::status_style("pending");
        assert_eq!(s.fg, Some(Color::Yellow));
    }

    #[test]
    fn truncate_clamps_long_strings() {
        assert_eq!(truncate("abcd", 10), "abcd");
        assert_eq!(truncate("abcdefghij", 5), "abcd…");
    }
}
