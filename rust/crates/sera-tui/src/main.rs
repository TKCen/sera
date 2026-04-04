//! SERA TUI — terminal user interface built with ratatui + crossterm.
//!
//! Replaces the Go TUI (tui/).
//! Provides a dashboard for viewing and interacting with SERA agent instances.

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;
use std::time::Duration;

mod api;
mod app;
mod ui;
mod views;

#[tokio::main]
async fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let api_url =
        std::env::var("SERA_API_URL").unwrap_or_else(|_| "http://localhost:3001".to_string());
    let api_key = std::env::var("SERA_API_KEY")
        .unwrap_or_else(|_| "sera_bootstrap_dev_123".to_string());

    let client = api::ApiClient::new(api_url, api_key);
    let mut app = app::App::new(client);

    // Initial data load
    app.refresh().await;

    // Main loop
    loop {
        terminal.draw(|f| app.render(f))?;

        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('r') => app.refresh().await,
                other => app.handle_key(other).await,
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
