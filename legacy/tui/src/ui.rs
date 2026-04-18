use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::{App, AppState};

pub fn render(frame: &mut Frame, app: &mut App) {
    match app.state {
        AppState::AgentList => render_agent_list(frame, app),
        AppState::Chat => render_chat(frame, app),
    }
}

fn render_agent_list(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let items: Vec<ListItem> = app
        .agents
        .iter()
        .map(|a| {
            ListItem::new(format!(
                "{} [{}]",
                a.name, a.status
            ))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" SERA Agents — ↑↓ navigate, Enter select, q quit "),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn render_chat(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let agent_name = app
        .selected_agent
        .as_ref()
        .map(|a| a.name.clone())
        .unwrap_or_else(|| "Unknown".to_owned());

    // Horizontal: left 67% | right 33%
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(67), Constraint::Percentage(33)])
        .split(area);

    // Left: chat viewport + input
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(5)])
        .split(cols[0]);

    // Track viewport heights for auto-scroll
    app.chat_viewport_height = left[0].height.saturating_sub(2);
    app.thoughts_viewport_height = cols[1].height.saturating_sub(2);

    // Chat history
    let chat_text = build_chat_text(app);
    let chat_para = Paragraph::new(chat_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} — PgUp/PgDn scroll, Esc back ", agent_name)),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.chat_scroll, 0));
    frame.render_widget(chat_para, left[0]);

    // Input
    let input_title = if app.is_loading {
        " Waiting for response… "
    } else {
        " Message — Enter to send, Esc to go back "
    };
    let input_para = Paragraph::new(app.input.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(input_title)
                .border_style(if app.is_loading {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::Blue)
                }),
        );
    frame.render_widget(input_para, left[1]);

    // Thoughts panel
    let thoughts_text = build_thoughts_text(app);
    let thoughts_para = Paragraph::new(thoughts_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Thoughts "),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.thoughts_scroll, 0));
    frame.render_widget(thoughts_para, cols[1]);
}

fn build_chat_text(app: &App) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    for msg in &app.messages {
        let (color, label) = if msg.sender == "You" {
            (Color::Green, "You")
        } else {
            (Color::Cyan, "SERA")
        };
        lines.push(Line::from(vec![Span::styled(
            format!("{}: ", label),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]));
        for text_line in msg.text.lines() {
            lines.push(Line::from(text_line.to_owned()));
        }
        lines.push(Line::from(""));
    }

    if !app.current_response.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "SERA: ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]));
        for text_line in app.current_response.lines() {
            lines.push(Line::from(text_line.to_owned()));
        }
        lines.push(Line::from(vec![Span::styled(
            "▮",
            Style::default().fg(Color::Yellow),
        )]));
    } else if app.is_loading {
        lines.push(Line::from(vec![Span::styled(
            "SERA is thinking…",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )]));
    }

    if let Some(err) = &app.error {
        lines.push(Line::from(vec![Span::styled(
            format!("⚠ {}", err),
            Style::default().fg(Color::Red),
        )]));
    }

    Text::from(lines)
}

fn build_thoughts_text(app: &App) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    for thought in &app.thoughts {
        let (color, icon) = match thought.step_type.as_str() {
            "observe" => (Color::Blue, "👁 "),
            "plan" => (Color::Yellow, "📋 "),
            "act" => (Color::Green, "▶ "),
            "reflect" => (Color::Magenta, "🔄 "),
            "tool-call" => (Color::Cyan, "🔧 "),
            "tool-result" => (Color::LightCyan, "✅ "),
            _ => (Color::Gray, "• "),
        };

        lines.push(Line::from(vec![Span::styled(
            format!("{}{}", icon, thought.step_type.to_uppercase()),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]));

        for (i, line) in thought.content.lines().enumerate() {
            if i >= 4 {
                lines.push(Line::from(vec![Span::styled(
                    "  …".to_owned(),
                    Style::default().fg(Color::DarkGray),
                )]));
                break;
            }
            lines.push(Line::from(vec![Span::styled(
                format!("  {}", line),
                Style::default().fg(Color::Gray),
            )]));
        }
        lines.push(Line::from(""));
    }

    Text::from(lines)
}
