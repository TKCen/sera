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
            created_at: "2026-04-01T00:00:00Z".to_string(),
            updated_at: "2026-04-10T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn new_view_has_no_agent() {
        let view = AgentDetailView::new();
        assert!(view.agent.is_none());
    }

    #[test]
    fn set_agent_stores_agent() {
        let mut view = AgentDetailView::new();
        view.set_agent(make_agent("id-1", "my-agent"));
        assert!(view.agent.is_some());
        assert_eq!(view.agent.as_ref().unwrap().id, "id-1");
    }

    #[test]
    fn set_agent_replaces_previous() {
        let mut view = AgentDetailView::new();
        view.set_agent(make_agent("old-id", "old"));
        view.set_agent(make_agent("new-id", "new"));
        assert_eq!(view.agent.as_ref().unwrap().id, "new-id");
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
