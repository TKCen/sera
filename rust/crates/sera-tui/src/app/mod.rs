//! Application state machine and event dispatch loop.
//!
//! The app owns:
//! * one [`GatewayClient`] for every request (HTTP + SSE)
//! * four [`views`] — agent list, session, HITL queue, evolve status
//! * an [`Action`] dispatcher (pure — no I/O, testable with a simple
//!   `reduce(state, action)` call)
//! * async refresh helpers that load data from the gateway

pub mod actions;

use std::sync::Arc;

use tokio::sync::mpsc::{self, UnboundedSender};
use tokio_stream::StreamExt as _;

use crate::client::{ConnectionState, GatewayClient, SseUpdate};
use crate::keybindings::TuiKeybindings;
use crate::views::agent_list::AgentListView;
use crate::views::evolve_status::EvolveStatusView;
use crate::views::hitl_queue::HitlQueueView;
use crate::views::session::SessionView;

pub use actions::{Action, ViewKind};

/// Footer-bar messages the app surfaces to the operator.
#[derive(Debug, Clone)]
pub struct Status {
    pub text: String,
    pub level: StatusLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Info,
    Warn,
    Error,
}

impl Status {
    pub fn info(t: impl Into<String>) -> Self {
        Self {
            text: t.into(),
            level: StatusLevel::Info,
        }
    }
    pub fn warn(t: impl Into<String>) -> Self {
        Self {
            text: t.into(),
            level: StatusLevel::Warn,
        }
    }
    pub fn error(t: impl Into<String>) -> Self {
        Self {
            text: t.into(),
            level: StatusLevel::Error,
        }
    }
}

/// Async command emitted by the reducer when a view transition needs
/// I/O (refresh data, approve HITL ticket, subscribe SSE).  The runtime
/// loop executes these out-of-band so the dispatcher stays pure.
#[derive(Debug, Clone)]
pub enum AppCommand {
    RefreshAll,
    RefreshCurrent,
    LoadSessionFor(String),
    Approve(String),
    Reject(String),
    Escalate(String),
    /// POST a message to /api/chat and pipe the SSE stream into SessionView.
    SendChat { agent: String, message: String },
}

/// Root application state.
pub struct App {
    pub focus: ViewKind,
    pub should_quit: bool,
    pub keybindings: TuiKeybindings,
    pub status: Status,

    pub agents: AgentListView,
    pub session: SessionView,
    pub hitl: HitlQueueView,
    pub evolve: EvolveStatusView,

    pub connection: ConnectionState,
    pub client: Arc<GatewayClient>,

    /// The agent currently being viewed / targeted by composer sends.
    /// Set by `Action::Select` and `Action::SelectAgent`.
    pub active_agent_id: Option<String>,

    /// Commands emitted by `dispatch` that the runtime must execute.
    /// The field is `pub` so the runtime (in `run`) can drain it each
    /// tick without needing a getter.
    pub pending: Vec<AppCommand>,
}

impl App {
    pub fn new(client: GatewayClient, keybindings: TuiKeybindings) -> Self {
        Self {
            focus: ViewKind::Agents,
            should_quit: false,
            keybindings,
            status: Status::info("ready"),
            agents: AgentListView::new(),
            session: SessionView::new(),
            hitl: HitlQueueView::new(),
            evolve: EvolveStatusView::new(),
            connection: ConnectionState::Disconnected,
            client: Arc::new(client),
            active_agent_id: None,
            pending: Vec::new(),
        }
    }

    /// Apply `action` to the state.  Pure apart from pushing commands
    /// onto `self.pending`; a test can construct an `App` with a
    /// `GatewayClient::new("http://127.0.0.1:1", …)` that never fires
    /// and still exercise the full reducer.
    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::NextView => {
                self.focus = self.focus.next();
                self.pending.push(AppCommand::RefreshCurrent);
            }
            Action::PrevView => {
                self.focus = self.focus.prev();
                self.pending.push(AppCommand::RefreshCurrent);
            }
            Action::Refresh => self.pending.push(AppCommand::RefreshAll),
            Action::Up => match self.focus {
                ViewKind::Agents => self.agents.up(),
                ViewKind::Session => self.session.scroll_up(),
                ViewKind::Hitl => self.hitl.up(),
                ViewKind::Evolve => self.evolve.up(),
            },
            Action::Down => match self.focus {
                ViewKind::Agents => self.agents.down(),
                ViewKind::Session => self.session.scroll_down(),
                ViewKind::Hitl => self.hitl.down(),
                ViewKind::Evolve => self.evolve.down(),
            },
            Action::PageUp => {
                if let ViewKind::Session = self.focus {
                    self.session.page_up();
                }
            }
            Action::PageDown => {
                if let ViewKind::Session = self.focus {
                    self.session.page_down();
                }
            }
            Action::Select => {
                if self.focus == ViewKind::Agents
                    && let Some(id) = self.agents.selected_id()
                {
                    self.active_agent_id = Some(id.clone());
                    self.focus = ViewKind::Session;
                    self.pending.push(AppCommand::LoadSessionFor(id));
                }
            }
            Action::SelectAgent(id) => {
                self.active_agent_id = Some(id.clone());
                self.focus = ViewKind::Session;
                self.pending.push(AppCommand::LoadSessionFor(id));
            }
            Action::Back => {
                if self.focus != ViewKind::Agents {
                    self.focus = ViewKind::Agents;
                    self.pending.push(AppCommand::RefreshCurrent);
                }
            }
            Action::Approve => {
                if let ViewKind::Hitl = self.focus
                    && let Some(id) = self.hitl.selected_id()
                {
                    self.hitl.clear_error();
                    self.pending.push(AppCommand::Approve(id));
                }
            }
            Action::Reject => {
                if let ViewKind::Hitl = self.focus
                    && let Some(id) = self.hitl.selected_id()
                {
                    self.hitl.clear_error();
                    self.pending.push(AppCommand::Reject(id));
                }
            }
            Action::Escalate => {
                if let ViewKind::Hitl = self.focus
                    && let Some(id) = self.hitl.selected_id()
                {
                    self.hitl.clear_error();
                    self.pending.push(AppCommand::Escalate(id));
                }
            }
            Action::EndOfBuffer => {
                if let ViewKind::Session = self.focus {
                    self.session.jump_to_end();
                }
            }
            Action::ToggleComposerFocus => {
                if let ViewKind::Session = self.focus {
                    self.session.toggle_focus();
                }
            }
            Action::SubmitComposer => {
                if let ViewKind::Session = self.focus {
                    self.session.submit_composer();
                    // Drain pending_sends into SendChat commands.
                    let messages: Vec<String> = self.session.pending_sends.drain(..).collect();
                    for message in messages {
                        match &self.active_agent_id {
                            Some(agent) => {
                                self.pending.push(AppCommand::SendChat {
                                    agent: agent.clone(),
                                    message,
                                });
                            }
                            None => {
                                tracing::warn!(
                                    message = %message,
                                    "composer send dropped: no active_agent_id (G.0.3 will set it)"
                                );
                                self.status = Status::warn("no agent selected — choose an agent first");
                            }
                        }
                    }
                }
            }
            Action::ComposerInput(key) => {
                if let ViewKind::Session = self.focus {
                    self.session.input_to_composer(key);
                }
            }
            Action::NoOp => {}
        }
    }

    /// Hand an SSE update to the session view.  Separated so the runtime
    /// can route channel messages without holding a borrow across await.
    pub fn apply_sse(&mut self, update: SseUpdate) {
        match update {
            SseUpdate::Event(ev) => {
                self.session.apply_event(ev);
            }
            SseUpdate::State(s) => {
                self.connection = s;
                self.session.set_connection(s);
            }
        }
    }

    /// Footer hint row for the currently focused view.  Changes by pane so
    /// the operator sees the relevant keybindings.
    pub fn footer_hint(&self) -> String {
        let kb = &self.keybindings;
        let base = format!(
            "{}:quit  {}:refresh  {}:tab  {}:S-tab",
            display_first(&kb.quit),
            display_first(&kb.refresh),
            display_first(&kb.next_view),
            display_first(&kb.prev_view)
        );
        let extra = match self.focus {
            ViewKind::Agents => format!(
                "  {}:select  {}:↑  {}:↓",
                display_first(&kb.select),
                display_first(&kb.up),
                display_first(&kb.down)
            ),
            ViewKind::Session => format!(
                "  {}:back  {}:↑  {}:↓  {}:end",
                display_first(&kb.back),
                display_first(&kb.up),
                display_first(&kb.down),
                display_first(&kb.end_of_buffer)
            ),
            ViewKind::Hitl => format!(
                "  {}:approve  {}:reject  {}:escalate",
                display_first(&kb.approve),
                display_first(&kb.reject),
                display_first(&kb.escalate)
            ),
            ViewKind::Evolve => format!("  {}:↑  {}:↓", display_first(&kb.up), display_first(&kb.down)),
        };
        base + &extra
    }
}

fn display_first(bindings: &[crate::keybindings::KeyBinding]) -> String {
    bindings
        .first()
        .map(|b| b.display())
        .unwrap_or_else(|| "?".into())
}

/// Runtime glue: executes [`AppCommand`]s, drives the SSE task, and
/// ferries [`SseUpdate`]s onto the in-process channel consumed by the
/// event loop.
pub struct Runtime {
    pub sse_task: Option<tokio::task::JoinHandle<()>>,
    pub sse_tx: UnboundedSender<SseUpdate>,
}

impl Runtime {
    pub fn new(sse_tx: UnboundedSender<SseUpdate>) -> Self {
        Self {
            sse_task: None,
            sse_tx,
        }
    }

    /// Drain pending commands, spawning async work for each.  Completed
    /// data lands back on the App via status messages or via direct
    /// setters — we keep the flow unidirectional via the event loop.
    pub async fn execute(&mut self, app: &mut App) {
        let pending = std::mem::take(&mut app.pending);
        for cmd in pending {
            match cmd {
                AppCommand::RefreshAll => Self::refresh_all(app).await,
                AppCommand::RefreshCurrent => self.refresh_focus(app).await,
                AppCommand::LoadSessionFor(agent_id) => {
                    self.load_session_for(app, agent_id).await;
                }
                AppCommand::Approve(id) => match app.client.approve_hitl(&id).await {
                    Ok(()) => {
                        app.status = Status::info(format!("approved {id}"));
                        Self::refresh_hitl(app).await;
                    }
                    Err(e) => {
                        app.hitl.set_error(e.to_string());
                        app.status = Status::error(format!("approve failed: {e}"));
                    }
                },
                AppCommand::Reject(id) => match app.client.reject_hitl(&id).await {
                    Ok(()) => {
                        app.status = Status::info(format!("rejected {id}"));
                        Self::refresh_hitl(app).await;
                    }
                    Err(e) => {
                        app.hitl.set_error(e.to_string());
                        app.status = Status::error(format!("reject failed: {e}"));
                    }
                },
                AppCommand::Escalate(id) => match app.client.escalate_hitl(&id).await {
                    Ok(()) => {
                        app.status = Status::info(format!("escalated {id}"));
                        Self::refresh_hitl(app).await;
                    }
                    Err(e) => {
                        app.hitl.set_error(e.to_string());
                        app.status = Status::error(format!("escalate failed: {e}"));
                    }
                },
                AppCommand::SendChat { agent, message } => {
                    self.send_chat(app, agent, message).await;
                }
            }
        }
    }

    pub async fn refresh_all(app: &mut App) {
        Self::refresh_agents(app).await;
        Self::refresh_hitl(app).await;
        Self::refresh_evolve(app).await;
    }

    async fn refresh_focus(&mut self, app: &mut App) {
        match app.focus {
            ViewKind::Agents => Self::refresh_agents(app).await,
            ViewKind::Session => { /* driven by SSE + explicit load */ }
            ViewKind::Hitl => Self::refresh_hitl(app).await,
            ViewKind::Evolve => Self::refresh_evolve(app).await,
        }
    }

    async fn refresh_agents(app: &mut App) {
        match app.client.list_agents().await {
            Ok(list) => {
                let n = list.len();
                app.agents.set_agents(list);
                app.status = Status::info(format!("{n} agent(s) loaded"));
            }
            Err(e) => {
                app.status = Status::error(format!("agent list failed: {e}"));
            }
        }
    }

    async fn refresh_hitl(app: &mut App) {
        match app.client.list_hitl().await {
            Ok(list) => {
                let n = list.len();
                app.hitl.set_requests(list);
                app.status = Status::info(format!("{n} HITL request(s)"));
            }
            Err(e) => {
                app.status = Status::warn(format!("HITL list unavailable: {e}"));
            }
        }
    }

    async fn refresh_evolve(app: &mut App) {
        match app.client.list_evolve_proposals().await {
            Ok(list) => {
                let n = list.len();
                app.evolve.set_proposals(list);
                app.status = Status::info(format!("{n} evolve proposal(s)"));
            }
            Err(e) => {
                app.status = Status::warn(format!("evolve list unavailable: {e}"));
            }
        }
    }

    /// Spawn a task that POSTs to `/api/chat` and pipes SSE events into the
    /// session view via the existing `sse_tx` channel.
    async fn send_chat(&mut self, app: &mut App, agent: String, message: String) {
        let client = Arc::clone(&app.client);
        let forward_to = self.sse_tx.clone();

        // Transition to Reconnecting to give visual feedback while connecting.
        app.apply_sse(SseUpdate::State(ConnectionState::Reconnecting));
        app.status = Status::info(format!("sending to {agent}…"));

        tokio::spawn(async move {
            // Signal: connecting.
            let _ = forward_to.send(SseUpdate::State(ConnectionState::Reconnecting));

            match client.post_chat(&agent, &message).await {
                Err(e) => {
                    tracing::warn!(error = %e, "post_chat HTTP error");
                    let _ = forward_to.send(SseUpdate::State(ConnectionState::Disconnected));
                }
                Ok(mut stream) => {
                    let _ = forward_to.send(SseUpdate::State(ConnectionState::Connected));
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(ev) => {
                                if forward_to.send(SseUpdate::Event(ev)).is_err() {
                                    return;
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "post_chat stream error");
                                let _ = forward_to
                                    .send(SseUpdate::State(ConnectionState::Disconnected));
                                return;
                            }
                        }
                    }
                    // Stream ended cleanly — back to Connected/idle.
                    let _ = forward_to.send(SseUpdate::State(ConnectionState::Connected));
                }
            }
        });
    }

    async fn load_session_for(&mut self, app: &mut App, agent_id: String) {
        match app.client.list_sessions(Some(&agent_id)).await {
            Ok(mut sessions) => {
                if let Some(session) = sessions.pop() {
                    // Hydrate transcript synchronously, then spawn SSE.
                    let transcript = app
                        .client
                        .session_transcript(&session.id)
                        .await
                        .unwrap_or_default();
                    app.session.set_session(session.clone());
                    app.session.set_transcript(transcript);

                    // Re-subscribe SSE — cancel any existing stream first.
                    if let Some(handle) = self.sse_task.take() {
                        handle.abort();
                    }
                    // Bridge the mpsc sender into the unbounded channel.
                    let (bridge_tx, mut bridge_rx) = mpsc::channel::<SseUpdate>(64);
                    let forward_to = self.sse_tx.clone();
                    tokio::spawn(async move {
                        while let Some(u) = bridge_rx.recv().await {
                            if forward_to.send(u).is_err() {
                                break;
                            }
                        }
                    });
                    self.sse_task = Some(app.client.spawn_sse(session.id.clone(), bridge_tx));
                    app.status = Status::info(format!("session {} loaded", session.id));
                } else {
                    // No sessions yet — clear any stale transcript so the
                    // composer pane starts fresh; the first Ctrl+Enter will
                    // create the session server-side.
                    app.session.set_transcript(Vec::new());
                    app.status = Status::info(format!("no sessions for agent {agent_id} — ready to chat"));
                }
            }
            Err(e) => {
                app.status = Status::error(format!("session load failed: {e}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Agent, GatewayClient, HitlRequest, SessionSummary, StreamEvent, SseUpdate};

    fn client() -> GatewayClient {
        GatewayClient::new("http://127.0.0.1:1", "test", std::time::Duration::from_millis(1))
            .unwrap()
    }

    fn agent(id: &str) -> Agent {
        Agent {
            id: id.to_owned(),
            name: format!("name-{id}"),
            display_name: None,
            status: "running".to_owned(),
            template_or_provider: "tpl".to_owned(),
            last_heartbeat_at: None,
        }
    }

    #[test]
    fn new_app_focuses_agents_and_is_not_quit() {
        let app = App::new(client(), TuiKeybindings::defaults());
        assert_eq!(app.focus, ViewKind::Agents);
        assert!(!app.should_quit);
    }

    #[test]
    fn dispatch_quit_sets_should_quit() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.dispatch(Action::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn dispatch_next_view_rotates_forward() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.dispatch(Action::NextView);
        assert_eq!(app.focus, ViewKind::Session);
        app.dispatch(Action::NextView);
        assert_eq!(app.focus, ViewKind::Hitl);
        app.dispatch(Action::NextView);
        assert_eq!(app.focus, ViewKind::Evolve);
        app.dispatch(Action::NextView);
        assert_eq!(app.focus, ViewKind::Agents);
    }

    #[test]
    fn dispatch_prev_view_rotates_backward() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.dispatch(Action::PrevView);
        assert_eq!(app.focus, ViewKind::Evolve);
    }

    #[test]
    fn dispatch_select_with_agent_loads_session_view() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.agents.set_agents(vec![agent("a"), agent("b")]);
        app.dispatch(Action::Select);
        assert_eq!(app.focus, ViewKind::Session);
        assert!(matches!(
            app.pending.last(),
            Some(AppCommand::LoadSessionFor(id)) if id == "a"
        ));
    }

    #[test]
    fn dispatch_select_with_no_agents_is_noop() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.dispatch(Action::Select);
        assert_eq!(app.focus, ViewKind::Agents);
        assert!(app.pending.is_empty());
    }

    #[test]
    fn dispatch_back_from_session_returns_to_agents() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.focus = ViewKind::Session;
        app.dispatch(Action::Back);
        assert_eq!(app.focus, ViewKind::Agents);
    }

    #[test]
    fn dispatch_up_down_on_agents_moves_selection() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.agents.set_agents(vec![agent("a"), agent("b"), agent("c")]);
        app.dispatch(Action::Down);
        assert_eq!(app.agents.selected_id().as_deref(), Some("b"));
        app.dispatch(Action::Up);
        assert_eq!(app.agents.selected_id().as_deref(), Some("a"));
    }

    #[test]
    fn approve_emits_command_when_hitl_focused() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.focus = ViewKind::Hitl;
        app.hitl.set_requests(vec![HitlRequest {
            id: "h1".into(),
            agent_id: "a1".into(),
            summary: "read".into(),
            age: "".into(),
            status: "pending".into(),
        }]);
        app.dispatch(Action::Approve);
        assert!(matches!(app.pending.last(), Some(AppCommand::Approve(id)) if id == "h1"));
    }

    #[test]
    fn approve_on_non_hitl_view_is_noop() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.focus = ViewKind::Agents;
        app.dispatch(Action::Approve);
        assert!(app.pending.is_empty());
    }

    #[test]
    fn apply_sse_event_lands_on_session_view() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.session.set_session(SessionSummary {
            id: "s1".into(),
            agent_id: "a1".into(),
            created_at: String::new(),
            state: "active".into(),
        });
        app.apply_sse(SseUpdate::Event(StreamEvent {
            event_type: "message".into(),
            session_id: "s1".into(),
            role: "assistant".into(),
            delta: "hi".into(),
            tool: String::new(),
        }));
        assert_eq!(app.session.transcript.len(), 1);
        assert_eq!(app.session.transcript[0].text, "hi");
    }

    #[test]
    fn apply_sse_state_flips_connection() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.apply_sse(SseUpdate::State(ConnectionState::Connected));
        assert_eq!(app.connection, ConnectionState::Connected);
        app.apply_sse(SseUpdate::State(ConnectionState::Disconnected));
        assert_eq!(app.connection, ConnectionState::Disconnected);
    }

    #[test]
    fn end_of_buffer_only_acts_on_session() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.session.scroll_up();
        assert!(!app.session.auto_scroll);
        app.focus = ViewKind::Session;
        app.dispatch(Action::EndOfBuffer);
        assert!(app.session.auto_scroll);
    }

    #[test]
    fn footer_hint_changes_with_focus() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        let agents_hint = app.footer_hint();
        app.focus = ViewKind::Hitl;
        let hitl_hint = app.footer_hint();
        assert_ne!(agents_hint, hitl_hint);
        assert!(hitl_hint.contains("approve"));
    }

    #[test]
    fn refresh_action_enqueues_refresh_all() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.dispatch(Action::Refresh);
        assert!(matches!(app.pending.last(), Some(AppCommand::RefreshAll)));
    }

    #[test]
    fn select_agent_sets_active_agent_id() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        assert_eq!(app.active_agent_id, None);
        app.dispatch(Action::SelectAgent("agent-42".to_owned()));
        assert_eq!(app.active_agent_id.as_deref(), Some("agent-42"));
    }

    #[test]
    fn select_agent_switches_to_session_pane() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        assert_eq!(app.focus, ViewKind::Agents);
        app.dispatch(Action::SelectAgent("agent-42".to_owned()));
        assert_eq!(app.focus, ViewKind::Session);
        assert!(matches!(
            app.pending.last(),
            Some(AppCommand::LoadSessionFor(id)) if id == "agent-42"
        ));
    }

    #[test]
    fn select_agent_without_existing_session_clears_transcript() {
        // The transcript-clearing happens inside load_session_for (runtime),
        // but we verify the dispatch sets the right command so the runtime
        // will reach the clear path on empty session list.
        let mut app = App::new(client(), TuiKeybindings::defaults());
        // Pre-populate transcript with stale data.
        app.session.set_transcript(vec![
            crate::client::TranscriptEntry { role: "user".into(), text: "old message".into() },
        ]);
        app.dispatch(Action::SelectAgent("fresh-agent".to_owned()));
        // Dispatch is pure — transcript not cleared yet (that's runtime's job).
        // But active_agent_id is set and LoadSessionFor is queued.
        assert_eq!(app.active_agent_id.as_deref(), Some("fresh-agent"));
        assert!(matches!(
            app.pending.last(),
            Some(AppCommand::LoadSessionFor(id)) if id == "fresh-agent"
        ));
    }
}
