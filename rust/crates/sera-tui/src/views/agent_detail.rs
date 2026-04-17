//! Agent detail view.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::api::Agent;
use super::View;

/// Displays details for a single agent.
// TODO(sera-2q1d): view is scaffolded; not yet instantiated in the main app loop.
#[allow(dead_code)]
pub struct AgentDetailView {
    agent: Option<Agent>,
}

#[allow(dead_code)]
impl AgentDetailView {
    /// Create a new agent detail view.
    pub fn new() -> Self {
        Self { agent: None }
    }

    /// Set the agent to display.
    pub fn set_agent(&mut self, agent: Agent) {
        self.agent = Some(agent);
    }
}

impl Default for AgentDetailView {
    fn default() -> Self {
        Self::new()
    }
}

impl View for AgentDetailView {
    fn render(&self, frame: &mut Frame, area: Rect) {
        match &self.agent {
            Some(agent) => {
                let content = format!(
                    "ID: {}\nName: {}\nTemplate: {}\nStatus: {}\nCreated: {}\nUpdated: {}",
                    agent.id, agent.name, agent.template_ref, agent.status, agent.created_at, agent.updated_at
                );
                let paragraph = Paragraph::new(content)
                    .block(
                        ratatui::widgets::Block::default()
                            .title(" Agent Details ")
                            .borders(ratatui::widgets::Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan)),
                    )
                    .style(Style::default().fg(Color::White));
                frame.render_widget(paragraph, area);
            }
            None => {
                let text = Paragraph::new("No agent selected")
                    .style(Style::default().fg(Color::Yellow));
                frame.render_widget(text, area);
            }
        }
    }
}
