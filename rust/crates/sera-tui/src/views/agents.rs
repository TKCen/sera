//! Agents list view with table.

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Table, Row},
};

use crate::api::Agent;
use super::View;

/// Displays a list of agents in a table.
pub struct AgentsView {
    agents: Vec<Agent>,
    selected: usize,
}

impl AgentsView {
    /// Create a new agents view.
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            selected: 0,
        }
    }

    /// Set the agents to display.
    pub fn set_agents(&mut self, agents: Vec<Agent>) {
        self.agents = agents;
        self.selected = 0;
    }

    /// Move selection up.
    pub fn previous(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn next(&mut self) {
        if self.selected < self.agents.len().saturating_sub(1) {
            self.selected += 1;
        }
    }

    /// Get the ID of the selected agent.
    pub fn selected_id(&self) -> Option<&String> {
        self.agents.get(self.selected).map(|a| &a.id)
    }
}

impl Default for AgentsView {
    fn default() -> Self {
        Self::new()
    }
}

impl View for AgentsView {
    fn render(&self, frame: &mut Frame, area: Rect) {
        if self.agents.is_empty() {
            let text = Paragraph::new("No agents found. Press 'r' to refresh.")
                .style(Style::default().fg(Color::Yellow));
            frame.render_widget(text, area);
            return;
        }

        // Build table rows
        let rows = self.agents.iter().enumerate().map(|(idx, agent)| {
            let style = if idx == self.selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            Row::new(vec![
                agent.name.clone(),
                agent.status.clone(),
                agent.template_ref.clone(),
                agent.created_at.clone(),
            ])
            .style(style)
        });

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(25),
                Constraint::Percentage(20),
                Constraint::Percentage(30),
                Constraint::Percentage(25),
            ],
        )
        .header(
            Row::new(vec!["Name", "Status", "Template", "Created"])
                .style(Style::default().bold().fg(Color::Cyan)),
        )
        .block(
            Block::default()
                .title(" Agents ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(table, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_agent(id: &str, name: &str) -> Agent {
        Agent {
            id: id.to_string(),
            name: name.to_string(),
            display_name: None,
            template_ref: "tpl".to_string(),
            status: "running".to_string(),
            created_at: "2026-04-10T00:00:00Z".to_string(),
            updated_at: "2026-04-10T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn new_view_has_no_selection_when_empty() {
        let view = AgentsView::new();
        assert!(view.selected_id().is_none());
    }

    #[test]
    fn set_agents_resets_selection_to_zero() {
        let mut view = AgentsView::new();
        view.set_agents(vec![make_agent("a", "Alpha"), make_agent("b", "Beta")]);
        view.next();
        assert_eq!(view.selected_id(), Some(&"b".to_string()));

        // Resetting agents moves back to index 0
        view.set_agents(vec![make_agent("x", "X"), make_agent("y", "Y")]);
        assert_eq!(view.selected_id(), Some(&"x".to_string()));
    }

    #[test]
    fn navigation_clamps_at_boundaries() {
        let mut view = AgentsView::new();
        view.set_agents(vec![
            make_agent("a", "Alpha"),
            make_agent("b", "Beta"),
            make_agent("c", "Gamma"),
        ]);

        // previous at top does nothing
        view.previous();
        assert_eq!(view.selected_id(), Some(&"a".to_string()));

        // advance to last
        view.next();
        view.next();
        assert_eq!(view.selected_id(), Some(&"c".to_string()));

        // next at last does nothing
        view.next();
        assert_eq!(view.selected_id(), Some(&"c".to_string()));
    }

    #[test]
    fn selected_id_tracks_current_position() {
        let mut view = AgentsView::new();
        view.set_agents(vec![make_agent("a", "A"), make_agent("b", "B")]);
        assert_eq!(view.selected_id(), Some(&"a".to_string()));
        view.next();
        assert_eq!(view.selected_id(), Some(&"b".to_string()));
        view.previous();
        assert_eq!(view.selected_id(), Some(&"a".to_string()));
    }
}
