//! Logs view for displaying agent logs.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::api::LogEntry;
use super::View;

/// Displays scrollable log entries.
pub struct LogsView {
    logs: Vec<LogEntry>,
    scroll_offset: usize,
}

impl LogsView {
    /// Create a new logs view.
    pub fn new() -> Self {
        Self {
            logs: Vec::new(),
            scroll_offset: 0,
        }
    }

    /// Set the logs to display.
    pub fn set_logs(&mut self, logs: Vec<LogEntry>) {
        self.logs = logs;
        self.scroll_offset = 0;
    }

    /// Scroll up.
    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    /// Scroll down.
    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }
}

impl Default for LogsView {
    fn default() -> Self {
        Self::new()
    }
}

impl View for LogsView {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let content = if self.logs.is_empty() {
            "No logs available".to_string()
        } else {
            self.logs
                .iter()
                .map(|entry| {
                    format!("[{}] {}: {}", entry.timestamp, entry.level, entry.message)
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let paragraph = Paragraph::new(content)
            .block(
                ratatui::widgets::Block::default()
                    .title(" Logs ")
                    .borders(ratatui::widgets::Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .style(Style::default().fg(Color::White))
            .scroll((self.scroll_offset as u16, 0));

        frame.render_widget(paragraph, area);
    }
}
