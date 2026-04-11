mod api;
mod models;
mod ui;
mod ws;

use api::ApiClient;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use models::{AgentInstance, ChatMessage, ThoughtEvent, WsEvent};
use ratatui::{backend::CrosstermBackend, widgets::ListState, Terminal};
use std::{
    io::{self, Read},
    sync::mpsc,
    time::Duration,
};

#[derive(Parser)]
#[command(name = "sera-tui", about = "SERA agent terminal interface")]
struct Cli {
    /// Send a prompt non-interactively and print the response.
    /// Pass \"-\" to read the prompt from stdin.
    #[arg(short, long, value_name = "PROMPT")]
    prompt: Option<String>,

    /// Target agent by name or ID (default: first available)
    #[arg(short, long, value_name = "AGENT")]
    agent: Option<String>,
}

pub enum AppState {
    AgentList,
    Chat,
}

pub struct App {
    pub state: AppState,
    pub agents: Vec<AgentInstance>,
    pub list_state: ListState,
    pub selected_agent: Option<AgentInstance>,
    pub messages: Vec<ChatMessage>,
    pub thoughts: Vec<ThoughtEvent>,
    pub input: String,
    pub current_response: String,
    pub is_loading: bool,
    pub session_id: Option<String>,
    pub ws_rx: Option<mpsc::Receiver<WsEvent>>,
    pub chat_scroll: u16,
    pub thoughts_scroll: u16,
    pub chat_viewport_height: u16,
    pub thoughts_viewport_height: u16,
    pub error: Option<String>,
    pub quit: bool,
}

impl App {
    fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            state: AppState::AgentList,
            agents: Vec::new(),
            list_state,
            selected_agent: None,
            messages: Vec::new(),
            thoughts: Vec::new(),
            input: String::new(),
            current_response: String::new(),
            is_loading: false,
            session_id: None,
            ws_rx: None,
            chat_scroll: 0,
            thoughts_scroll: 0,
            chat_viewport_height: 20,
            thoughts_viewport_height: 20,
            error: None,
            quit: false,
        }
    }

    fn scroll_chat_to_bottom(&mut self) {
        let mut lines: u16 = 0;
        for msg in &self.messages {
            lines += 1 + msg.text.lines().count() as u16 + 1;
        }
        if !self.current_response.is_empty() {
            lines += 1 + self.current_response.lines().count() as u16 + 1;
        } else if self.is_loading {
            lines += 1;
        }
        self.chat_scroll = lines.saturating_sub(self.chat_viewport_height);
    }

    fn scroll_thoughts_to_bottom(&mut self) {
        let mut lines: u16 = 0;
        for t in &self.thoughts {
            lines += 1 + t.content.lines().count().min(4) as u16 + 1;
        }
        self.thoughts_scroll = lines.saturating_sub(self.thoughts_viewport_height);
    }
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    let base_url =
        std::env::var("SERA_API_URL").unwrap_or_else(|_| "http://localhost:3001".to_owned());
    let api_key =
        std::env::var("SERA_API_KEY").unwrap_or_else(|_| "sera_bootstrap_dev_123".to_owned());

    let api = ApiClient::new(base_url, api_key);

    if let Some(prompt_arg) = cli.prompt {
        return run_prompt(&api, prompt_arg, cli.agent);
    }

    run_tui(api)
}

/// Non-interactive mode: send a single prompt, print the reply, exit.
fn run_prompt(api: &ApiClient, prompt_arg: String, agent_filter: Option<String>) -> io::Result<()> {
    let message = if prompt_arg == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf.trim().to_owned()
    } else {
        prompt_arg
    };

    if message.is_empty() {
        eprintln!("sera-tui: empty prompt");
        std::process::exit(1);
    }

    let agents = api
        .get_instances()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    if agents.is_empty() {
        eprintln!("sera-tui: no agent instances found");
        std::process::exit(1);
    }

    let agent = match &agent_filter {
        Some(filter) => agents
            .iter()
            .find(|a| a.name == *filter || a.id == *filter)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("agent '{}' not found", filter),
                )
            })?,
        None => &agents[0],
    };

    let resp = api
        .send_chat_sync(&message, &agent.id, None)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    println!("{}", resp.response);
    Ok(())
}

/// Interactive TUI mode.
fn run_tui(api: ApiClient) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    match api.get_instances() {
        Ok(instances) => app.agents = instances,
        Err(e) => app.error = Some(format!("Failed to load agents: {}", e)),
    }

    let result = event_loop(&mut terminal, &mut app, &api);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    api: &ApiClient,
) -> io::Result<()> {
    loop {
        // Collect SSE events into a local buffer (avoids holding borrow on app.ws_rx
        // while also needing to mutate other app fields).
        let ws_events: Vec<WsEvent> = match &app.ws_rx {
            Some(rx) => {
                let mut buf = Vec::new();
                while let Ok(ev) = rx.try_recv() {
                    buf.push(ev);
                }
                buf
            }
            None => Vec::new(),
        };

        for event in ws_events {
            match event {
                WsEvent::Token(token) => {
                    app.current_response.push_str(&token);
                    app.scroll_chat_to_bottom();
                }
                WsEvent::Done => {
                    let text = std::mem::take(&mut app.current_response);
                    app.messages.push(ChatMessage {
                        sender: "SERA".to_owned(),
                        text,
                    });
                    app.is_loading = false;
                    app.scroll_chat_to_bottom();
                }
                WsEvent::Thought(thought) => {
                    app.thoughts.push(thought);
                    app.scroll_thoughts_to_bottom();
                }
                WsEvent::Error(e) => {
                    app.error = Some(format!("SSE: {}", e));
                    app.is_loading = false;
                }
            }
        }

        terminal.draw(|frame| ui::render(frame, app))?;

        if app.quit {
            return Ok(());
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                handle_key(app, key, api);
            }
        }
    }
}

fn handle_key(app: &mut App, key: event::KeyEvent, api: &ApiClient) {
    let is_chat = matches!(app.state, AppState::Chat);

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.quit = true;
        return;
    }

    if !is_chat {
        // Agent list navigation
        match key.code {
            KeyCode::Char('q') => app.quit = true,
            KeyCode::Down | KeyCode::Char('j') => {
                let max = app.agents.len().saturating_sub(1);
                let next = app.list_state.selected().map(|i| (i + 1).min(max)).unwrap_or(0);
                app.list_state.select(Some(next));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let prev = app.list_state.selected().map(|i| i.saturating_sub(1)).unwrap_or(0);
                app.list_state.select(Some(prev));
            }
            KeyCode::Enter => {
                if let Some(idx) = app.list_state.selected() {
                    if let Some(agent) = app.agents.get(idx).cloned() {
                        app.selected_agent = Some(agent);
                        app.messages.clear();
                        app.thoughts.clear();
                        app.input.clear();
                        app.current_response.clear();
                        app.is_loading = false;
                        app.session_id = None;
                        app.ws_rx = None;
                        app.error = None;
                        app.chat_scroll = 0;
                        app.thoughts_scroll = 0;
                        app.state = AppState::Chat;
                    }
                }
            }
            _ => {}
        }
        return;
    }

    // Chat mode
    match key.code {
        KeyCode::Esc => {
            app.state = AppState::AgentList;
            app.ws_rx = None;
        }
        KeyCode::Enter => {
            if app.input.is_empty() || app.is_loading {
                return;
            }
            let message = std::mem::take(&mut app.input);
            let agent_id = app
                .selected_agent
                .as_ref()
                .map(|a| a.id.clone())
                .unwrap_or_else(|| "sera".to_owned());

            app.messages.push(ChatMessage {
                sender: "You".to_owned(),
                text: message.clone(),
            });
            app.is_loading = true;
            app.current_response.clear();
            app.error = None;
            app.scroll_chat_to_bottom();

            match api.send_chat_stream(&message, &agent_id, app.session_id.as_deref()) {
                Ok(reader) => {
                    let (tx, rx) = mpsc::channel::<WsEvent>();
                    app.ws_rx = Some(rx);
                    ws::spawn_sse_thread(reader, tx);
                }
                Err(e) => {
                    app.error = Some(format!("Send failed: {}", e));
                    app.is_loading = false;
                }
            }
        }
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::PageUp => {
            app.chat_scroll = app.chat_scroll.saturating_sub(app.chat_viewport_height / 2);
        }
        KeyCode::PageDown => {
            app.chat_scroll += app.chat_viewport_height / 2;
        }
        KeyCode::Char(c) => {
            if !app.is_loading {
                app.input.push(c);
            }
        }
        _ => {}
    }
}
