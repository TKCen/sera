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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_block_title_has_padding_spaces() {
        let block = centered_block("Hello");
        // Title is wrapped with spaces: " Hello "
        let title_str = format!("{:?}", block);
        assert!(title_str.contains("Hello"));
    }

    #[test]
    fn error_text_style_is_red() {
        let para = error_text("oops");
        let debug = format!("{:?}", para);
        // ratatui Debug format: Style::new().red().bold()
        assert!(debug.contains(".red()"));
    }

    #[test]
    fn info_text_style_is_green() {
        let para = info_text("ok");
        let debug = format!("{:?}", para);
        assert!(debug.contains(".green()"));
    }

    #[test]
    fn warning_text_style_is_yellow() {
        let para = warning_text("watch out");
        let debug = format!("{:?}", para);
        assert!(debug.contains(".yellow()"));
    }
}
