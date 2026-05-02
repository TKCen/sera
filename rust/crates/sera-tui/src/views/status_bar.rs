//! Bottom status bar — one line showing connection state, active agent, and
//! session short-id.
//!
//! Lane state is intentionally stubbed as "idle" until a future bead adds a
//! real lane-state source.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::client::ConnectionState;

/// Stateless status bar widget.  Caller constructs one per render tick from
/// fields already held by `App` and `SessionView`.
pub struct StatusBar<'a> {
    /// Active agent name derived from the current session, or `None`.
    pub agent: Option<&'a str>,
    /// Full session id — first 8 chars are shown; `None` renders as `-`.
    pub session_id: Option<&'a str>,
    /// Current connection state.
    pub conn: ConnectionState,
    /// Cumulative prompt tokens reported by the gateway across the
    /// session's completed turns (sera-c7bf).
    pub prompt_tokens: u64,
    /// Cumulative completion tokens — see [`Self::prompt_tokens`].
    pub completion_tokens: u64,
    /// Cumulative cost in USD. `0.0` for local models.
    pub cost_usd: f64,
}

impl StatusBar<'_> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let agent_label = self.agent.unwrap_or("no agent");
        let session_label: String = match self.session_id {
            None | Some("") => "-".to_owned(),
            Some(id) => id.chars().take(8).collect(),
        };

        let (conn_label, conn_color) = conn_style(self.conn);

        let total_tokens = self
            .prompt_tokens
            .saturating_add(self.completion_tokens);
        let tokens_label = format!(
            "tokens={} ({}↑/{}↓)",
            total_tokens, self.prompt_tokens, self.completion_tokens
        );
        // Two decimals when nonzero, zero decimals at $0 to keep the line tight.
        let cost_label = if self.cost_usd > 0.0 {
            format!("cost=${:.4}", self.cost_usd)
        } else {
            "cost=$0".to_owned()
        };

        let spans = vec![
            Span::raw(" agent="),
            Span::styled(
                agent_label.to_owned(),
                Style::default().fg(Color::White),
            ),
            Span::raw(" · session="),
            Span::styled(session_label, Style::default().fg(Color::White)),
            Span::raw(" · "),
            Span::styled(conn_label, Style::default().fg(conn_color)),
            Span::raw(" · lane=idle · "),
            Span::styled(tokens_label, Style::default().fg(Color::White)),
            Span::raw(" · "),
            Span::styled(cost_label, Style::default().fg(Color::White)),
            Span::raw(" "),
        ];

        let bar = Paragraph::new(Line::from(spans))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(bar, area);
    }
}

fn conn_style(state: ConnectionState) -> (&'static str, Color) {
    match state {
        ConnectionState::Connected => ("connected", Color::Cyan),
        ConnectionState::Reconnecting => ("reconnecting", Color::Yellow),
        ConnectionState::Disconnected => ("disconnected", Color::DarkGray),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_to_string(bar: StatusBar<'_>, width: u16) -> String {
        let backend = TestBackend::new(width, 1);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| bar.render(f, f.area())).unwrap();
        term.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .join("")
    }

    fn bar_with<'a>(agent: Option<&'a str>, session_id: Option<&'a str>, conn: ConnectionState) -> StatusBar<'a> {
        StatusBar {
            agent,
            session_id,
            conn,
            prompt_tokens: 0,
            completion_tokens: 0,
            cost_usd: 0.0,
        }
    }

    #[test]
    fn renders_no_agent_label_when_active_none() {
        let bar = bar_with(None, None, ConnectionState::Disconnected);
        let out = render_to_string(bar, 120);
        assert!(out.contains("no agent"), "expected 'no agent' in: {out}");
    }

    #[test]
    fn renders_agent_name_when_set() {
        let bar = bar_with(Some("sera"), Some("abc123def456"), ConnectionState::Connected);
        let out = render_to_string(bar, 120);
        assert!(out.contains("sera"), "expected 'sera' in: {out}");
        assert!(out.contains("abc123de"), "expected truncated session id in: {out}");
    }

    #[test]
    fn renders_token_and_cost_labels() {
        let bar = StatusBar {
            agent: Some("sera"),
            session_id: Some("s1"),
            conn: ConnectionState::Connected,
            prompt_tokens: 42,
            completion_tokens: 17,
            cost_usd: 0.0,
        };
        let out = render_to_string(bar, 160);
        assert!(out.contains("tokens=59"), "expected 'tokens=59' in: {out}");
        assert!(out.contains("42↑/17↓"), "expected '42↑/17↓' in: {out}");
        assert!(out.contains("cost=$0"), "expected 'cost=$0' in: {out}");
    }

    #[test]
    fn connection_state_color_maps_correctly() {
        assert_eq!(conn_style(ConnectionState::Connected).1, Color::Cyan);
        assert_eq!(conn_style(ConnectionState::Reconnecting).1, Color::Yellow);
        assert_eq!(conn_style(ConnectionState::Disconnected).1, Color::DarkGray);
    }

    #[test]
    fn session_id_truncated_to_8_chars() {
        let bar = bar_with(Some("test"), Some("0123456789abcdef"), ConnectionState::Connected);
        let out = render_to_string(bar, 120);
        assert!(out.contains("01234567"), "expected first 8 chars in: {out}");
        assert!(!out.contains("01234567890"), "should not contain full id in: {out}");
    }

    #[test]
    fn missing_session_id_renders_dash() {
        let bar = bar_with(Some("test"), None, ConnectionState::Connected);
        let out = render_to_string(bar, 120);
        assert!(out.contains("session=-"), "expected 'session=-' in: {out}");
    }
}
