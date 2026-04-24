//! SERA TUI — operator terminal UI built with ratatui + crossterm.
//!
//! **J.0.1 chat-dominant layout**: the main canvas is a full-screen Session
//! view (metadata + transcript + tool log), with a composer pinned at the
//! bottom and a one-line status/hint footer.  Agents, HITL queue, and
//! evolve status are accessed as modal overlays:
//! * **Ctrl+A** — agents modal (select / switch agent)
//! * **Ctrl+H** — HITL queue modal (approve/reject/escalate)
//! * **Ctrl+E** — evolve status modal (read-only)
//! * **Ctrl+P** — session picker modal (resume existing session)
//!
//! All keybindings are configurable via [`keybindings::TuiKeybindings`].
//! No hardcoded key-code checks in dispatch code (project CLAUDE.md rule).

use std::io;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyEventKind,
    },
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
use input::translate_session;
use keybindings::{matches_key, TuiKeybindings};

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
    execute!(out, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(out);
    Ok(Terminal::new(backend)?)
}

/// Restore terminal state on exit.  Safe to call twice (idempotent).
fn restore_terminal<B: ratatui::backend::Backend + io::Write>(
    terminal: &mut Terminal<B>,
) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
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
            DisableMouseCapture,
            DisableBracketedPaste
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
        if event::poll(tick)? {
            match event::read()? {
                // Pastes go to the composer — the composer is always the
                // input sink in the chat-dominant layout, as long as no
                // modal is on top.
                Event::Paste(content)
                    if !app.show_session_picker
                        && !app.any_j01_modal_open()
                        && app.show_hitl_modal.is_none() =>
                {
                    app.dispatch(crate::app::Action::PasteToComposer(content));
                }
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    use crossterm::event::KeyCode;
                    let action = if app.show_session_picker {
                        // Session picker intercept: only navigation/select/esc.
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
                    } else if app.any_j01_modal_open() {
                        // J.0.1 modal intercept: translate plain key bindings
                        // (up/down/select/approve/reject/escalate/quit/esc).
                        // The composer does NOT receive keystrokes while a
                        // modal is open.
                        if matches_key(&key, &app.keybindings.quit) {
                            crate::app::Action::Quit
                        } else if matches_key(&key, &app.keybindings.back)
                            || key.code == KeyCode::Esc
                        {
                            crate::app::Action::CloseModal
                        } else if matches_key(&key, &app.keybindings.up) {
                            crate::app::Action::Up
                        } else if matches_key(&key, &app.keybindings.down) {
                            crate::app::Action::Down
                        } else if matches_key(&key, &app.keybindings.select) {
                            crate::app::Action::Select
                        } else if matches_key(&key, &app.keybindings.approve) {
                            crate::app::Action::Approve
                        } else if matches_key(&key, &app.keybindings.reject) {
                            crate::app::Action::Reject
                        } else if matches_key(&key, &app.keybindings.escalate) {
                            crate::app::Action::Escalate
                        } else if matches_key(&key, &app.keybindings.refresh) {
                            crate::app::Action::Refresh
                        } else {
                            crate::app::Action::NoOp
                        }
                    } else {
                        // Chat-dominant default — Session canvas is always
                        // the background, composer may or may not have focus.
                        translate_session(
                            &key,
                            &app.keybindings,
                            app.session.composer_focused(),
                        )
                    };
                    app.dispatch(action);
                }
                _ => {}
            }
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
