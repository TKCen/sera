//! Shared UI rendering helpers.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

/// Create a centered block with a title.
pub fn centered_block(title: &str) -> ratatui::widgets::Block {
    ratatui::widgets::Block::default()
        .title(format!(" {} ", title))
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
}

/// Create a styled error message.
pub fn error_text(message: &str) -> Paragraph {
    Paragraph::new(message).style(Style::default().fg(Color::Red).bold())
}

/// Create a styled info message.
pub fn info_text(message: &str) -> Paragraph {
    Paragraph::new(message).style(Style::default().fg(Color::Green))
}

/// Create a styled warning message.
pub fn warning_text(message: &str) -> Paragraph {
    Paragraph::new(message).style(Style::default().fg(Color::Yellow))
}
