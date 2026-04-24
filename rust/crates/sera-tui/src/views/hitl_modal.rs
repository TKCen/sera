//! Inline HITL approval modal — centered overlay rendered over the active pane.
//!
//! When a permission request fires on the operator's active session, the app
//! sets `App::show_hitl_modal = Some(req)` and `ui::render` calls
//! [`render_hitl_modal`] after the normal pane render so the overlay appears
//! on top.
//!
//! Key bindings (shown in the modal footer, driven by [`TuiKeybindings`]):
//! * approve key (`a`) — approve and dismiss
//! * reject key (`x`) — reject and dismiss
//! * escalate key (`e`) — escalate and dismiss
//! * back key (`Esc`) — dismiss without action (request stays in HITL queue)

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::client::HitlRequest;
use crate::keybindings::{display_first, TuiKeybindings};

/// Render a centered modal overlay for `req`.
///
/// The modal is 60 columns wide and 12 rows tall, centred in `frame.area()`.
/// It renders on top of whatever pane is currently active — the caller is
/// responsible for rendering the background pane first.
pub fn render_hitl_modal(frame: &mut Frame, req: &HitlRequest, kb: &TuiKeybindings) {
    let area = centered_rect(60, 12, frame.area());

    // Clear the region so the modal has a clean background.
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(Span::styled(
            " Permission Request ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    // Inner area (inside the border).
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split inner into body (top) + hint (bottom 1 line).
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    // Body: agent, tool/summary, requested scope (age field carries timestamp).
    let body_lines = vec![
        Line::from(vec![
            Span::styled("Agent:   ", Style::default().fg(Color::DarkGray)),
            Span::raw(req.agent_id.as_str()),
        ]),
        Line::from(vec![
            Span::styled("Request: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                req.summary.as_str(),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Time:    ", Style::default().fg(Color::DarkGray)),
            Span::raw(req.age.as_str()),
        ]),
        Line::from(vec![
            Span::styled("Status:  ", Style::default().fg(Color::DarkGray)),
            Span::raw(req.status.as_str()),
        ]),
    ];

    let body = Paragraph::new(body_lines).wrap(Wrap { trim: false });
    frame.render_widget(body, rows[0]);

    // Hint line at the bottom of the modal.
    let hint = Line::from(vec![
        Span::styled(
            format!("{}:approve", display_first(&kb.approve)),
            Style::default().fg(Color::Green),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{}:reject", display_first(&kb.reject)),
            Style::default().fg(Color::Red),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{}:escalate", display_first(&kb.escalate)),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{}:dismiss", display_first(&kb.back)),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let hint_p = Paragraph::new(hint).alignment(Alignment::Center);
    frame.render_widget(hint_p, rows[1]);
}

/// Compute a [`Rect`] centred inside `area` with the given width and height.
/// Clamps to `area` so the modal never overflows on tiny terminals.
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
    use crate::keybindings::TuiKeybindings;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn req() -> HitlRequest {
        HitlRequest {
            id: "h1".into(),
            agent_id: "agent-alpha".into(),
            summary: "write /tmp/secret".into(),
            age: "2026-04-24T10:00:00Z".into(),
            status: "pending".into(),
        }
    }

    #[test]
    fn centered_rect_fits_inside_area() {
        let area = Rect::new(0, 0, 80, 24);
        let r = centered_rect(60, 12, area);
        assert!(r.x + r.width <= area.x + area.width);
        assert!(r.y + r.height <= area.y + area.height);
        assert_eq!(r.width, 60);
        assert_eq!(r.height, 12);
    }

    #[test]
    fn centered_rect_clamps_to_area() {
        let area = Rect::new(0, 0, 20, 5);
        let r = centered_rect(60, 12, area);
        assert_eq!(r.width, 20);
        assert_eq!(r.height, 5);
    }

    #[test]
    fn render_modal_produces_output_with_title_and_keys() {
        let kb = TuiKeybindings::defaults();
        let r = req();
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_hitl_modal(f, &r, &kb)).unwrap();
        let buf = term.backend().buffer().clone();
        let rendered: String = buf.content().iter().map(|c| c.symbol()).collect::<Vec<_>>().join("");
        assert!(rendered.contains("Permission Request"), "title missing");
        assert!(rendered.contains("agent-alpha"), "agent_id missing");
        assert!(rendered.contains("write /tmp/secret"), "summary missing");
    }
}
