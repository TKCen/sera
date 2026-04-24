//! Central render entry point — **chat-dominant** layout (J.0.1).
//!
//! The screen is one big transcript canvas with a composer pinned at the
//! bottom and a single-line status/hint footer.  Agents, HITL queue, and
//! evolve status are accessed as modal overlays via Ctrl+A/H/E.
//!
//! Layout:
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ main (Min(3))         — Session chat canvas  │
//! ├──────────────────────────────────────────────┤
//! │ composer (Length(5))  — multi-line input     │
//! ├──────────────────────────────────────────────┤
//! │ status (Length(1))    — agent/session/conn   │
//! │ hint   (Length(1))    — contextual keys      │
//! └──────────────────────────────────────────────┘
//! ```

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, StatusLevel};
use crate::views::hitl_modal::render_hitl_modal;
use crate::views::status_bar::StatusBar;

/// Render the whole screen.
pub fn render(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // main chat canvas
            Constraint::Length(5), // composer
            Constraint::Length(1), // status bar
            Constraint::Length(1), // key hint
        ])
        .split(frame.area());

    // Main canvas: Session chat (metadata + transcript + tool log).
    app.session.render_chat(frame, chunks[0], true);

    // Composer — always visible, regardless of modal state.
    app.session.render_composer_only(frame, chunks[1]);

    // Status bar: agent name + session short-id + connection state.
    let agent = app.active_agent_id.as_deref();
    let session_id = app.session.session.as_ref().map(|s| s.id.as_str());
    StatusBar {
        agent,
        session_id,
        conn: app.connection,
    }
    .render(frame, chunks[2]);

    // Key hint footer — context-sensitive.
    render_hint(frame, chunks[3], app);

    // Modal overlays — rendered on top of everything, topmost last.
    if app.show_agents_modal {
        render_agents_modal(frame, app);
    }
    if app.show_hitl_queue_modal {
        render_hitl_queue_modal(frame, app);
    }
    if app.show_evolve_modal {
        render_evolve_modal(frame, app);
    }

    // Session picker modal (Ctrl+P) — pre-existing modal.
    if app.show_session_picker {
        app.session_picker.render(frame, frame.area());
    }

    // Help modal — rendered when /help is active.
    if app.show_help {
        render_help_modal(frame, frame.area());
    }

    // Inline HITL approval modal — highest priority overlay.
    if let Some(req) = &app.show_hitl_modal {
        render_hitl_modal(frame, req, &app.keybindings);
    }
}

fn render_hint(frame: &mut Frame, area: Rect, app: &App) {
    let hint = app.footer_hint();
    let status_style = match app.status.level {
        StatusLevel::Info => Style::default().fg(Color::DarkGray),
        StatusLevel::Warn => Style::default().fg(Color::Yellow),
        StatusLevel::Error => Style::default().fg(Color::Red),
    };
    // When there's an active status message, prefer it; otherwise show the hint.
    let text = if app.status.text.is_empty() || app.status.text == "ready" {
        hint
    } else {
        format!("{}  ·  {}", hint, app.status.text)
    };
    let p = Paragraph::new(text).style(status_style);
    frame.render_widget(p, area);
}

fn render_agents_modal(frame: &mut Frame, app: &mut App) {
    let area = centered_modal(70, 60, frame.area());
    frame.render_widget(Clear, area);
    // Draw a titled border, then hand the inner area to the existing view.
    let block = Block::default()
        .title(" Agents — Enter:select  ↑/↓  esc:close ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    app.agents.render(frame, inner, true);
}

fn render_hitl_queue_modal(frame: &mut Frame, app: &mut App) {
    let area = centered_modal(75, 60, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" HITL Queue — a:approve  x:reject  e:escalate  esc:close ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    app.hitl.render(frame, inner, true);
}

fn render_evolve_modal(frame: &mut Frame, app: &mut App) {
    let area = centered_modal(75, 60, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Evolve — ↑/↓  esc:close ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    app.evolve.render(frame, inner, true);
}

fn render_help_modal(frame: &mut Frame, area: Rect) {
    let modal_area = centered_rect(60, 14, area);
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
        Line::from(vec![
            Span::styled("  Ctrl+A        ", Style::default().fg(Color::Cyan)),
            Span::raw("agents modal"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+H        ", Style::default().fg(Color::Cyan)),
            Span::raw("HITL queue modal"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+E        ", Style::default().fg(Color::Cyan)),
            Span::raw("evolve modal"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+P        ", Style::default().fg(Color::Cyan)),
            Span::raw("session picker"),
        ]),
        Line::from(""),
    ];
    let modal = Paragraph::new(text).style(Style::default()).block(
        Block::default()
            .title(" Commands ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title_style(Style::default().add_modifier(Modifier::BOLD)),
    );
    frame.render_widget(modal, modal_area);
}

/// Centered rectangle by percentage of the parent area.
fn centered_modal(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let w = r.width * percent_x / 100;
    let h = r.height * percent_y / 100;
    let x = r.x + (r.width.saturating_sub(w)) / 2;
    let y = r.y + (r.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w.max(1), h.max(1))
}

/// Return a [`Rect`] centered in `area` with the given width and height.
/// Both are clamped to the parent dimensions.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::client::GatewayClient;
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
    fn render_chat_canvas_produces_output() {
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
        // Chat-dominant canvas shows the Session header text.
        assert!(rendered.contains("No session selected"));
    }

    #[test]
    fn render_with_agents_modal_shows_agents_title() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.show_agents_modal = true;
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
        assert!(rendered.contains("Agents"), "agents modal title not rendered");
    }
}
