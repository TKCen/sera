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
