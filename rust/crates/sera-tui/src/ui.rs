//! Shared UI rendering helpers.
// TODO(sera-2q1d): helper fns are part of the shared UI library; not all are
// called by existing views yet but form the intended public API surface.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

/// Create a centered block with a title.
pub fn centered_block(title: &str) -> ratatui::widgets::Block<'_> {
    ratatui::widgets::Block::default()
        .title(format!(" {} ", title))
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
}

/// Create a styled error message.
pub fn error_text(message: &str) -> Paragraph<'_> {
    Paragraph::new(message).style(Style::default().fg(Color::Red).bold())
}

/// Create a styled info message.
pub fn info_text(message: &str) -> Paragraph<'_> {
    Paragraph::new(message).style(Style::default().fg(Color::Green))
}

/// Create a styled warning message.
pub fn warning_text(message: &str) -> Paragraph<'_> {
    Paragraph::new(message).style(Style::default().fg(Color::Yellow))
}
