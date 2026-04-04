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
