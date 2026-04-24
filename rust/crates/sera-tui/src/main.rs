//! SERA TUI — operator terminal UI built with ratatui + crossterm.
//!
//! Four panes rotate under Tab / Shift-Tab:
//! * **Agents** — list of agent instances (GET /api/agents)
//! * **Session** — metadata + streaming transcript (SSE where available)
//! * **HITL** — pending permission requests, approve/reject/escalate
//! * **Evolve** — read-only view over evolve proposals
//!
//! All keybindings are configurable via [`keybindings::TuiKeybindings`].
//! No hardcoded key-code checks in dispatch code (project CLAUDE.md rule).

use std::io;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

mod app;
mod client;
mod config;
mod input;
mod keybindings;
mod ui;
mod views;

use app::{App, Runtime};
use client::GatewayClient;
use config::Config;
use app::actions::ViewKind;
use input::{translate, translate_session};
use keybindings::TuiKeybindings;

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config::parse();
    install_panic_hook();

    let client = GatewayClient::new(
        &cfg.api_url,
        &cfg.api_key,
        Duration::from_secs(cfg.timeout_secs),
    )
    .context("building gateway client")?;

    let mut terminal = init_terminal().context("initialising terminal")?;
    let tick = Duration::from_millis(cfg.tick_ms);
    let result = run(&mut terminal, client, tick).await;
    restore_terminal(&mut terminal).ok();
    result
}

/// Initialise crossterm + alternate screen and return a ratatui Terminal.
fn init_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(out);
    Ok(Terminal::new(backend)?)
}

/// Restore terminal state on exit.  Safe to call twice (idempotent).
fn restore_terminal<B: ratatui::backend::Backend + io::Write>(
    terminal: &mut Terminal<B>,
) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

/// A panic hook that restores the terminal before printing the panic.
/// Without this, a panic mid-render leaves the operator's shell in raw
/// mode and useless.
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        original(info);
    }));
}

/// Main event loop.
async fn run<B: ratatui::backend::Backend + io::Write>(
    terminal: &mut Terminal<B>,
    client: GatewayClient,
    tick: Duration,
) -> Result<()> {
    let (sse_tx, mut sse_rx) = mpsc::unbounded_channel();
    let mut app = App::new(client, TuiKeybindings::defaults());
    let mut runtime = Runtime::new(sse_tx);

    // Initial fetch.
    Runtime::refresh_all(&mut app).await;

    loop {
        terminal.draw(|f| ui::render(f, &mut app))?;

        // Drain any pending SSE updates first — non-blocking.
        while let Ok(update) = sse_rx.try_recv() {
            app.apply_sse(update);
        }

        // Poll crossterm for input with a short budget so SSE + timer
        // have a chance to run each tick.
        if event::poll(tick)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            // When the session picker modal is open it intercepts all keys;
            // only Up/Down/Enter/Esc are forwarded — everything else is dropped.
            let action = if app.show_session_picker {
                use crossterm::event::KeyCode;
                use keybindings::matches_key;
                if matches_key(&key, &app.keybindings.up) {
                    crate::app::Action::PickerUp
                } else if matches_key(&key, &app.keybindings.down) {
                    crate::app::Action::PickerDown
                } else if matches_key(&key, &app.keybindings.select) {
                    crate::app::Action::PickerSelect
                } else if matches_key(&key, &app.keybindings.back)
                    || key.code == KeyCode::Esc
                {
                    crate::app::Action::ClosePicker
                } else {
                    crate::app::Action::NoOp
                }
            } else {
                let a = if app.focus == ViewKind::Session {
                    translate_session(&key, &app.keybindings, app.session.composer_focused())
                } else {
                    translate(&key, &app.keybindings)
                };
                // When Enter is pressed in the Agents pane, resolve the selected
                // agent ID here and dispatch SelectAgent so the action carries an
                // explicit ID (spec G.0.3).
                if a == crate::app::Action::Select
                    && app.focus == ViewKind::Agents
                    && let Some(id) = app.agents.selected_id()
                {
                    crate::app::Action::SelectAgent(id)
                } else {
                    a
                }
            };
            app.dispatch(action);
        }

        // Execute any commands the dispatcher queued.
        if !app.pending.is_empty() {
            runtime.execute(&mut app).await;
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
