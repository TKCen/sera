//! Central render entry point.
//!
//! Owns only the top-level layout (title bar, body, footer) — each pane
//! delegates to its view module.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{actions::ViewKind, App, StatusLevel};
use crate::client::ConnectionState;
use crate::views::status_bar::StatusBar;

/// Render the whole screen.
pub fn render(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_title(frame, chunks[0], app);

    // Body is the focused view rendered full-area.
    match app.focus {
        ViewKind::Agents => app.agents.render(frame, chunks[1], true),
        ViewKind::Session => app.session.render(frame, chunks[1], true),
        ViewKind::Hitl => app.hitl.render(frame, chunks[1], true),
        ViewKind::Evolve => app.evolve.render(frame, chunks[1], true),
    }

    render_footer(frame, chunks[2], app);

    // Help modal — rendered on top of everything when /help is active.
    if app.show_help {
        render_help_modal(frame, chunks[1]);
    }

    // Status bar: agent name + session short-id + connection state.
    let agent = app.active_agent_id.as_deref();
    let session_id = app.session.session.as_ref().map(|s| s.id.as_str());
    StatusBar {
        agent,
        session_id,
        conn: app.connection,
    }
    .render(frame, chunks[3]);
}

fn render_title(frame: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let tabs = [
        ViewKind::Agents,
        ViewKind::Session,
        ViewKind::Hitl,
        ViewKind::Evolve,
    ];
    let mut spans: Vec<Span<'_>> = Vec::with_capacity(tabs.len() * 2 + 4);
    spans.push(Span::styled(
        " SERA ",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw("│ "));
    for (i, v) in tabs.iter().enumerate() {
        let focused = *v == app.focus;
        let style = if focused {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(format!(" {} ", v.label()), style));
        if i + 1 < tabs.len() {
            spans.push(Span::raw(" "));
        }
    }
    spans.push(Span::raw("  "));
    spans.push(conn_badge(app.connection));

    let title = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(title, area);
}

fn conn_badge(state: ConnectionState) -> Span<'static> {
    let (label, color) = match state {
        ConnectionState::Connected => ("● connected", Color::Green),
        ConnectionState::Reconnecting => ("● reconnecting", Color::Yellow),
        ConnectionState::Disconnected => ("● disconnected", Color::Red),
    };
    Span::styled(
        label,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn render_help_modal(frame: &mut Frame, area: Rect) {
    let modal_area = centered_rect(50, 10, area);
    frame.render_widget(Clear, modal_area);
    let text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  /new, /clear  ", Style::default().fg(Color::Cyan)),
            Span::raw("clear transcript and tool log"),
        ]),
        Line::from(vec![
            Span::styled("  /agent <name> ", Style::default().fg(Color::Cyan)),
            Span::raw("switch active agent"),
        ]),
        Line::from(vec![
            Span::styled("  /help         ", Style::default().fg(Color::Cyan)),
            Span::raw("toggle this help modal"),
        ]),
        Line::from(vec![
            Span::styled("  /quit         ", Style::default().fg(Color::Cyan)),
            Span::raw("exit the TUI"),
        ]),
        Line::from(""),
    ];
    let modal = Paragraph::new(text).block(
        Block::default()
            .title(" Commands ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(modal, modal_area);
}

/// Return a [`Rect`] centered in `area` with the given width (columns) and
/// height (rows).  Both are clamped to the parent dimensions.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

fn render_footer(frame: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let hint = app.footer_hint();
    let status_style = match app.status.level {
        StatusLevel::Info => Style::default().fg(Color::Green),
        StatusLevel::Warn => Style::default().fg(Color::Yellow),
        StatusLevel::Error => Style::default().fg(Color::Red),
    };

    // Split footer into hint (top line) and status (bottom line).
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    let hint_p = Paragraph::new(hint).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(hint_p, chunks[0]);
    let status_p = Paragraph::new(app.status.text.clone()).style(status_style);
    frame.render_widget(status_p, chunks[1]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::client::{ConnectionState, GatewayClient};
    use crate::keybindings::TuiKeybindings;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn client() -> GatewayClient {
        GatewayClient::new(
            "http://127.0.0.1:1",
            "test",
            std::time::Duration::from_millis(1),
        )
        .unwrap()
    }

    #[test]
    fn render_agents_view_produces_output() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        let backend = TestBackend::new(80, 20);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut app)).unwrap();
        let buf = term.backend().buffer().clone();
        let rendered: String = buf
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .join("");
        assert!(rendered.contains("SERA"));
        assert!(rendered.contains("Agents"));
    }

    #[test]
    fn connection_badge_labels_all_states() {
        for (state, expect) in [
            (ConnectionState::Connected, "connected"),
            (ConnectionState::Reconnecting, "reconnecting"),
            (ConnectionState::Disconnected, "disconnected"),
        ] {
            let span = conn_badge(state);
            assert!(
                span.content.contains(expect),
                "badge for {state:?} did not mention {expect}"
            );
        }
    }
}
