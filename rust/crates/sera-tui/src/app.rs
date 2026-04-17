//! Application state and view management.

use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::api::ApiClient;
use crate::views::{agents::AgentsView, knowledge::{KnowledgeEntry, KnowledgeView}, View};

/// Active view in the TUI.
#[derive(Debug, Clone, PartialEq)]
pub enum ActiveView {
    Agents,
    AgentDetail(String),
    Logs(String),
    Knowledge,
}

/// Main application state.
pub struct App {
    pub client: ApiClient,
    pub active_view: ActiveView,
    pub agents_view: AgentsView,
    pub knowledge_view: KnowledgeView,
    pub status_message: String,
}

impl App {
    /// Create a new application instance.
    pub fn new(client: ApiClient) -> Self {
        Self {
            client,
            active_view: ActiveView::Agents,
            agents_view: AgentsView::new(),
            knowledge_view: KnowledgeView::new(),
            status_message: "Press 'r' to refresh, 'q' to quit, 'Enter' to view details, 'm' for knowledge".to_string(),
        }
    }

    /// Refresh data from the API.
    pub async fn refresh(&mut self) {
        match &self.active_view {
            ActiveView::Agents => {
                match self.client.list_agents().await {
                    Ok(agents) => {
                        self.agents_view.set_agents(agents);
                        self.status_message = "Agents loaded".to_string();
                    }
                    Err(e) => {
                        self.status_message = format!("Error: {}", e);
                    }
                }
            }
            ActiveView::AgentDetail(id) => {
                match self.client.get_agent(id).await {
                    Ok(agent) => {
                        self.status_message = format!("Loaded: {}", agent.name);
                    }
                    Err(e) => {
                        self.status_message = format!("Error: {}", e);
                    }
                }
            }
            ActiveView::Logs(id) => {
                match self.client.get_agent_logs(id).await {
                    Ok(_logs) => {
                        self.status_message = "Logs loaded".to_string();
                    }
                    Err(e) => {
                        self.status_message = format!("Error: {}", e);
                    }
                }
            }
            ActiveView::Knowledge => {
                match self.client.list_knowledge(None).await {
                    Ok(raw) => {
                        let entries: Vec<KnowledgeEntry> = raw
                            .iter()
                            .map(KnowledgeEntry::from_json)
                            .collect();
                        self.knowledge_view.set_entries(entries);
                        self.status_message = "Knowledge loaded. j/k navigate, Enter detail, 's' sort, '/' filter, Esc back".to_string();
                    }
                    Err(e) => {
                        self.status_message = format!("Error: {}", e);
                    }
                }
            }
        }
    }

    /// Render the application.
    pub fn render(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // header
                Constraint::Min(0),     // content
                Constraint::Length(1),  // status
            ])
            .split(frame.area());

        // Render header
        let header = ratatui::widgets::Block::default()
            .title(" SERA — Agent Dashboard ")
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(header, chunks[0]);

        // Render content based on active view
        match &self.active_view {
            ActiveView::Agents => {
                self.agents_view.render(frame, chunks[1]);
            }
            ActiveView::AgentDetail(_) => {
                let text = Paragraph::new("Agent detail view not yet implemented")
                    .style(Style::default().fg(Color::Yellow));
                frame.render_widget(text, chunks[1]);
            }
            ActiveView::Logs(_) => {
                let text = Paragraph::new("Logs view not yet implemented")
                    .style(Style::default().fg(Color::Yellow));
                frame.render_widget(text, chunks[1]);
            }
            ActiveView::Knowledge => {
                self.knowledge_view.render(frame, chunks[1]);
            }
        }

        // Render status bar
        let status = Paragraph::new(self.status_message.as_str())
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(status, chunks[2]);
    }

    /// Handle key press events.
    pub async fn handle_key(&mut self, key: KeyCode) {
        match &self.active_view {
            ActiveView::Agents => match key {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.agents_view.previous();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.agents_view.next();
                }
                KeyCode::Enter => {
                    if let Some(id) = self.agents_view.selected_id() {
                        self.active_view = ActiveView::AgentDetail(id.clone());
                        self.refresh().await;
                    }
                }
                KeyCode::Char('l') => {
                    if let Some(id) = self.agents_view.selected_id() {
                        self.active_view = ActiveView::Logs(id.clone());
                        self.refresh().await;
                    }
                }
                KeyCode::Char('m') => {
                    self.active_view = ActiveView::Knowledge;
                    self.refresh().await;
                }
                _ => {}
            },
            ActiveView::AgentDetail(_) | ActiveView::Logs(_) => match key {
                KeyCode::Esc | KeyCode::Backspace => {
                    self.active_view = ActiveView::Agents;
                    self.refresh().await;
                }
                _ => {}
            },
            ActiveView::Knowledge => match key {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.knowledge_view.previous();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.knowledge_view.next();
                }
                KeyCode::Enter => {
                    self.knowledge_view.toggle_detail();
                }
                KeyCode::Char('s') => {
                    self.knowledge_view.cycle_sort();
                }
                KeyCode::Char('/') => {
                    // Toggle filter — for now clear it if set, or set a placeholder
                    // Full filter input would require a separate input mode
                    let current = self.knowledge_view.filtered_entries().len();
                    if current == 0 || self.knowledge_view.has_filter() {
                        self.knowledge_view.set_filter(String::new());
                        self.status_message = "Filter cleared".to_string();
                    }
                }
                KeyCode::Esc | KeyCode::Backspace => {
                    self.active_view = ActiveView::Agents;
                    self.refresh().await;
                }
                _ => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Agent, ApiClient};

    fn make_client() -> ApiClient {
        ApiClient::new("http://localhost:3001".to_string(), "test-key".to_string())
    }

    fn make_agent(id: &str) -> Agent {
        Agent {
            id: id.to_string(),
            name: format!("agent-{id}"),
            display_name: None,
            template_ref: "tpl".to_string(),
            status: "running".to_string(),
            created_at: "2026-04-01T00:00:00Z".to_string(),
            updated_at: "2026-04-01T00:00:00Z".to_string(),
        }
    }

    // --- Initial state ---

    #[test]
    fn new_app_starts_on_agents_view() {
        let app = App::new(make_client());
        assert_eq!(app.active_view, ActiveView::Agents);
    }

    #[test]
    fn new_app_has_non_empty_status_message() {
        let app = App::new(make_client());
        assert!(!app.status_message.is_empty());
    }

    // --- ActiveView equality ---

    #[test]
    fn active_view_variants_compare_correctly() {
        assert_eq!(ActiveView::Agents, ActiveView::Agents);
        assert_eq!(ActiveView::Knowledge, ActiveView::Knowledge);
        assert_eq!(
            ActiveView::AgentDetail("x".to_string()),
            ActiveView::AgentDetail("x".to_string())
        );
        assert_ne!(
            ActiveView::AgentDetail("x".to_string()),
            ActiveView::AgentDetail("y".to_string())
        );
        assert_ne!(ActiveView::Agents, ActiveView::Knowledge);
    }

    // --- handle_key: navigation in Agents view ---

    #[tokio::test]
    async fn key_j_moves_selection_down() {
        let mut app = App::new(make_client());
        app.agents_view.set_agents(vec![make_agent("a"), make_agent("b")]);
        app.handle_key(KeyCode::Char('j')).await;
        assert_eq!(app.agents_view.selected_id(), Some(&"b".to_string()));
    }

    #[tokio::test]
    async fn key_k_moves_selection_up() {
        let mut app = App::new(make_client());
        app.agents_view.set_agents(vec![make_agent("a"), make_agent("b")]);
        app.handle_key(KeyCode::Char('j')).await;
        app.handle_key(KeyCode::Char('k')).await;
        assert_eq!(app.agents_view.selected_id(), Some(&"a".to_string()));
    }

    #[tokio::test]
    async fn key_up_moves_selection_up() {
        let mut app = App::new(make_client());
        app.agents_view.set_agents(vec![make_agent("a"), make_agent("b")]);
        app.handle_key(KeyCode::Down).await;
        app.handle_key(KeyCode::Up).await;
        assert_eq!(app.agents_view.selected_id(), Some(&"a".to_string()));
    }

    // --- handle_key: view transitions ---

    #[tokio::test]
    async fn key_m_switches_to_knowledge_view() {
        let mut app = App::new(make_client());
        app.handle_key(KeyCode::Char('m')).await;
        assert_eq!(app.active_view, ActiveView::Knowledge);
    }

    #[tokio::test]
    async fn esc_from_knowledge_returns_to_agents() {
        let mut app = App::new(make_client());
        app.active_view = ActiveView::Knowledge;
        app.handle_key(KeyCode::Esc).await;
        assert_eq!(app.active_view, ActiveView::Agents);
    }

    #[tokio::test]
    async fn backspace_from_agent_detail_returns_to_agents() {
        let mut app = App::new(make_client());
        app.active_view = ActiveView::AgentDetail("some-id".to_string());
        app.handle_key(KeyCode::Backspace).await;
        assert_eq!(app.active_view, ActiveView::Agents);
    }

    #[tokio::test]
    async fn esc_from_logs_returns_to_agents() {
        let mut app = App::new(make_client());
        app.active_view = ActiveView::Logs("some-id".to_string());
        app.handle_key(KeyCode::Esc).await;
        assert_eq!(app.active_view, ActiveView::Agents);
    }

    // --- handle_key: Enter with no agents selected does not change view ---

    #[tokio::test]
    async fn enter_on_empty_agents_view_stays_on_agents() {
        let mut app = App::new(make_client());
        // No agents loaded — selected_id() returns None
        app.handle_key(KeyCode::Enter).await;
        assert_eq!(app.active_view, ActiveView::Agents);
    }

    // --- handle_key: 'l' with no selection stays on agents ---

    #[tokio::test]
    async fn key_l_on_empty_agents_view_stays_on_agents() {
        let mut app = App::new(make_client());
        app.handle_key(KeyCode::Char('l')).await;
        assert_eq!(app.active_view, ActiveView::Agents);
    }

    // --- handle_key: knowledge view navigation ---

    #[tokio::test]
    async fn knowledge_view_key_s_cycles_sort() {
        let mut app = App::new(make_client());
        app.active_view = ActiveView::Knowledge;
        // Just verify it doesn't panic and stays in Knowledge view
        app.handle_key(KeyCode::Char('s')).await;
        assert_eq!(app.active_view, ActiveView::Knowledge);
    }
}
