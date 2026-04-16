//! SERA TUI — terminal user interface built with ratatui + crossterm.
//!
//! Replaces the Go TUI (tui/).
//! Provides a dashboard for viewing and interacting with SERA agent instances.
//! Also works as a CLI when invoked with subcommands.

use anyhow::Result;
use clap::Parser;
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
mod cli;
mod cli_commands;
mod ui;
mod views;

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli::Cli::parse();
    let client = api::ApiClient::new(args.api_url, args.api_key);

    match args.command {
        None | Some(cli::Commands::Tui) => run_tui(client).await,
        Some(cmd) => cli_commands::dispatch(client, cmd).await,
    }
}

/// Launch the interactive ratatui TUI.
async fn run_tui(client: api::ApiClient) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = app::App::new(client);

    // Initial data load
    app.refresh().await;

    // Main loop — 'q' quits, 'r' refreshes, 'm' opens knowledge view
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
