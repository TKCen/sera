//! Application state machine and event dispatch loop.
//!
//! The app owns:
//! * one [`GatewayClient`] for every request (HTTP + SSE)
//! * four [`views`] — agent list, session, HITL queue, evolve status
//! * an [`Action`] dispatcher (pure — no I/O, testable with a simple
//!   `reduce(state, action)` call)
//! * async refresh helpers that load data from the gateway

pub mod actions;
pub mod slash;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{self, UnboundedSender};
use tokio_stream::StreamExt as _;

use crate::autocomplete::{detect_trigger, AutocompletePopup};
use crate::client::{
    Agent, ClientError, ConnectionState, EvolveProposal, GatewayClient, HitlRequest, SseUpdate,
};
use crate::keybindings::TuiKeybindings;
use crate::views::agent_list::AgentListView;
use crate::views::blocks::ApprovalStatus;
use crate::views::evolve_status::EvolveStatusView;
use crate::views::hitl_queue::HitlQueueView;
use crate::views::session::SessionView;
use crate::views::session_picker::SessionPickerView;

pub use actions::{Action, Focus, ViewKind};
pub use slash::SlashCommand;

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

/// Result of a background fetch the runtime ran on a spawned task.  The
/// main loop drains these from `app_rx` and applies them via
/// [`apply_app_update`] so network I/O never holds `&mut App` across an
/// await on the draw path.
///
/// Each variant carries the per-resource generation captured when the
/// task was spawned.  Overlapping refreshes can complete out of order
/// (e.g. an `OpenHitlModal` `RefreshAll` racing with the `Approve`-
/// triggered refresh that follows); `apply_app_update` drops any
/// result whose generation no longer matches `App`'s current value so
/// an older response cannot overwrite newer state.
#[derive(Debug)]
pub enum AppUpdate {
    Agents(u64, Result<Vec<Agent>, ClientError>),
    Hitl(u64, Result<Vec<HitlRequest>, ClientError>),
    Evolve(u64, Result<Vec<EvolveProposal>, ClientError>),
}

/// State for the disconnected-gateway banner (J.0.7, sera-j0o8).
///
/// Rendered as a centred paragraph over the chat canvas while the gateway
/// is unreachable.  Cleared when a connection succeeds.
#[derive(Debug, Clone)]
pub struct DisconnectBanner {
    /// Monotonic time at which the next automatic retry will be attempted.
    pub retry_at: Instant,
    /// Current backoff interval: 1 → 2 → 4 → 8 → 16 → 30 s (capped).
    pub backoff: Duration,
}

impl DisconnectBanner {
    /// First banner: 1 s backoff, retry 1 s from now.
    pub fn new() -> Self {
        let backoff = Duration::from_secs(1);
        Self { retry_at: Instant::now() + backoff, backoff }
    }

    /// Advance backoff level (doubles, capped at 30 s) and reschedule.
    pub fn advance(&mut self) {
        self.backoff = (self.backoff * 2).min(Duration::from_secs(30));
        self.retry_at = Instant::now() + self.backoff;
    }

    /// Whole seconds until the next automatic retry, saturating at zero.
    pub fn secs_until_retry(&self) -> u64 {
        self.retry_at
            .checked_duration_since(Instant::now())
            .map(|d| d.as_secs())
            .unwrap_or(0)
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
    /// Fetch sessions for the picker modal (agent_id filter).
    LoadSessionsForPicker(String),
    /// Load a specific session by id (from picker selection).
    OpenSession(String),
    /// Approve the HITL request shown in the inline modal.
    ApproveModal(String),
    /// Reject the HITL request shown in the inline modal.
    RejectModal(String),
    /// Escalate the HITL request shown in the inline modal.
    EscalateModal(String),
    /// Cancel the in-flight streaming turn (J.0.4 / sera-zate).
    /// Carries the session_id to POST to /api/chat/cancel.
    CancelTurn(String),
    /// Re-attempt a `SendChat` that previously failed due to a disconnected
    /// gateway.  Bypasses the backoff timer and retries immediately.
    RetrySendChat { agent: String, message: String },
    /// Approve an inline transcript Approval block (J.1.4).
    /// On success the block status is updated to Approved.
    ApproveInlineBlock(String),
    /// Reject an inline transcript Approval block (J.1.4).
    RejectInlineBlock(String),
    /// Escalate an inline transcript Approval block (J.1.4).
    EscalateInlineBlock(String),
}

/// Root application state.
pub struct App {
    /// **J.0.1**: legacy view pointer — kept so the existing
    /// NextView/PrevView actions and tests still compile.  In the
    /// chat-dominant layout the main canvas is always Session, so this
    /// field is no longer used by `ui::render`.
    pub focus: ViewKind,
    /// **J.0.1**: keyboard focus inside the chat-dominant layout.
    /// Composer (typing) vs Transcript (scrolling).  Default is Composer.
    /// Read by J.0.2 (block-based transcript) onwards; currently set but
    /// not yet consulted by any renderer.
    #[allow(dead_code)]
    pub chat_focus: Focus,
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

    /// The session id of the current session.  Populated when a session is
    /// loaded; used by `CancelTurn` to POST /api/chat/cancel.
    pub active_session_id: Option<String>,

    /// True while a chat turn is actively streaming from the gateway.
    /// Set to true when `SendChat` is dispatched; cleared when the SSE
    /// stream ends (Connected state) or a `CancelTurn` completes.
    /// The input layer checks this to route ESC to `CancelTurn` instead
    /// of `Back` / modal-close.
    pub turn_streaming: bool,

    /// Session picker modal state.  Rendered on top of the current view
    /// when `show_session_picker` is true.
    pub session_picker: SessionPickerView,
    /// Whether the session picker modal is currently visible.
    pub show_session_picker: bool,
    /// When a HITL request fires on the active session, this is populated
    /// and a centered modal overlay is rendered over the current pane.
    /// `None` means no modal is open.
    pub show_hitl_modal: Option<HitlRequest>,

    /// **J.0.1**: whether the agents picker modal (Ctrl+A) is visible.
    pub show_agents_modal: bool,
    /// **J.0.1**: whether the HITL queue modal (Ctrl+H) is visible.
    /// Distinct from `show_hitl_modal` (the inline approval overlay).
    pub show_hitl_queue_modal: bool,
    /// **J.0.1**: whether the evolve status modal (Ctrl+E) is visible.
    pub show_evolve_modal: bool,

    /// Commands emitted by `dispatch` that the runtime must execute.
    /// The field is `pub` so the runtime (in `run`) can drain it each
    /// tick without needing a getter.
    pub pending: Vec<AppCommand>,

    /// When true, the help modal is rendered over the session pane.
    pub show_help: bool,

    /// Set when the gateway is unreachable; cleared on first successful
    /// connection.  Renderer shows a centred banner while this is `Some`.
    pub disconnect_banner: Option<DisconnectBanner>,

    /// Most-recent `SendChat` payload that failed with a connection error.
    /// Stored so `RetryConnection` can re-issue it without re-typing.
    pub pending_retry: Option<(String, String)>,

    /// Per-resource generation counters bumped on every refresh spawn.
    /// `apply_app_update` compares the result's captured generation
    /// against these to drop stale out-of-order responses (see
    /// [`AppUpdate`]).
    pub agents_gen: u64,
    pub hitl_gen: u64,
    pub evolve_gen: u64,

    /// Active autocomplete popup, or None when closed.
    pub autocomplete: Option<AutocompletePopup>,
}

impl App {
    pub fn new(client: GatewayClient, keybindings: TuiKeybindings) -> Self {
        Self {
            focus: ViewKind::Agents,
            chat_focus: Focus::Composer,
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
            active_session_id: None,
            turn_streaming: false,
            session_picker: SessionPickerView::new(),
            show_session_picker: false,
            show_hitl_modal: None,
            show_agents_modal: false,
            show_hitl_queue_modal: false,
            show_evolve_modal: false,
            pending: Vec::new(),
            show_help: false,
            disconnect_banner: None,
            pending_retry: None,
            agents_gen: 0,
            hitl_gen: 0,
            evolve_gen: 0,
            autocomplete: None,
        }
    }

    /// Returns true when any J.0.1 overlay modal (agents / HITL queue /
    /// evolve status) is currently shown.  Used by `dispatch` to reroute
    /// input to modal-scoped actions.
    pub fn any_j01_modal_open(&self) -> bool {
        self.show_agents_modal || self.show_hitl_queue_modal || self.show_evolve_modal
    }

    /// Apply `action` to the state.  Pure apart from pushing commands
    /// onto `self.pending`; a test can construct an `App` with a
    /// `GatewayClient::new("http://127.0.0.1:1", …)` that never fires
    /// and still exercise the full reducer.
    pub fn dispatch(&mut self, action: Action) {
        // J.0.1 modal precedence — when any chat-dominant modal is open
        // (agents / HITL queue / evolve), swallow background input and only
        // honour close/quit + modal-scoped navigation.
        if self.any_j01_modal_open() {
            match action {
                Action::CloseModal | Action::Back => {
                    // ESC closes the topmost modal — evolve > hitl > agents.
                    if self.show_evolve_modal {
                        self.show_evolve_modal = false;
                    } else if self.show_hitl_queue_modal {
                        self.show_hitl_queue_modal = false;
                    } else if self.show_agents_modal {
                        self.show_agents_modal = false;
                    }
                }
                Action::Quit => self.should_quit = true,
                Action::Up => {
                    if self.show_agents_modal {
                        self.agents.up();
                    } else if self.show_hitl_queue_modal {
                        self.hitl.up();
                    } else if self.show_evolve_modal {
                        self.evolve.up();
                    }
                }
                Action::Down => {
                    if self.show_agents_modal {
                        self.agents.down();
                    } else if self.show_hitl_queue_modal {
                        self.hitl.down();
                    } else if self.show_evolve_modal {
                        self.evolve.down();
                    }
                }
                Action::Select | Action::SelectAgent(_) => {
                    if self.show_agents_modal
                        && let Some(id) = self.agents.selected_id()
                    {
                        self.active_agent_id = Some(id.clone());
                        self.show_agents_modal = false;
                        self.pending.push(AppCommand::LoadSessionFor(id));
                    }
                }
                Action::Approve => {
                    if self.show_hitl_queue_modal
                        && let Some(id) = self.hitl.selected_id()
                    {
                        self.hitl.clear_error();
                        self.pending.push(AppCommand::Approve(id));
                    }
                }
                Action::Reject => {
                    if self.show_hitl_queue_modal
                        && let Some(id) = self.hitl.selected_id()
                    {
                        self.hitl.clear_error();
                        self.pending.push(AppCommand::Reject(id));
                    }
                }
                Action::Escalate => {
                    if self.show_hitl_queue_modal
                        && let Some(id) = self.hitl.selected_id()
                    {
                        self.hitl.clear_error();
                        self.pending.push(AppCommand::Escalate(id));
                    }
                }
                Action::Refresh => self.pending.push(AppCommand::RefreshAll),
                // Swallow everything else — composer typing must not leak
                // behind the modal.
                _ => {}
            }
            return;
        }

        // When the inline HITL modal is open, remap approve/reject/escalate/back
        // to modal-scoped actions and swallow everything else so the background
        // pane doesn't receive input while the modal is shown.
        if self.show_hitl_modal.is_some() {
            match action {
                Action::Approve => {
                    let id = self.show_hitl_modal.as_ref().map(|r| r.id.clone()).unwrap_or_default();
                    self.show_hitl_modal = None;
                    self.pending.push(AppCommand::ApproveModal(id));
                }
                Action::Reject => {
                    let id = self.show_hitl_modal.as_ref().map(|r| r.id.clone()).unwrap_or_default();
                    self.show_hitl_modal = None;
                    self.pending.push(AppCommand::RejectModal(id));
                }
                Action::Escalate => {
                    let id = self.show_hitl_modal.as_ref().map(|r| r.id.clone()).unwrap_or_default();
                    self.show_hitl_modal = None;
                    self.pending.push(AppCommand::EscalateModal(id));
                }
                Action::Back | Action::DismissHitlModal => {
                    self.show_hitl_modal = None;
                }
                // Quit still works even with modal open.
                Action::Quit => self.should_quit = true,
                // All other keys are swallowed while the modal is open.
                _ => {}
            }
            return;
        }

        // J.1.4: when a pending inline Approval block exists in the transcript,
        // intercept approve/reject/escalate and route to the inline block action.
        // Only active when the Session view has focus so that approve/reject/
        // escalate keypresses in the HITL queue view (or other views) are not
        // silently redirected to the first pending inline block.
        if self.focus == ViewKind::Session
            && let Some(id) = self.session.first_pending_approval_id().map(str::to_owned) {
            match action {
                Action::Approve => {
                    self.pending.push(AppCommand::ApproveInlineBlock(id));
                    return;
                }
                Action::Reject => {
                    self.pending.push(AppCommand::RejectInlineBlock(id));
                    return;
                }
                Action::Escalate => {
                    self.pending.push(AppCommand::EscalateInlineBlock(id));
                    return;
                }
                // All other actions fall through to the normal handler.
                _ => {}
            }
        }

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
                    // Submitting always closes any open popup.
                    self.autocomplete = None;
                    self.session.submit_composer();
                    // Drain slash commands first — parse and dispatch each.
                    let slashes: Vec<String> = self.session.pending_slash.drain(..).collect();
                    for raw in slashes {
                        match slash::parse(&raw) {
                            Ok(cmd) => self.dispatch(Action::ExecuteSlash(cmd)),
                            Err(msg) => self.status = Status::warn(msg),
                        }
                    }
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
            Action::ExecuteSlash(cmd) => match cmd {
                SlashCommand::New => {
                    self.session.blocks.clear();
                    self.status = Status::info("new turn");
                }
                SlashCommand::Agent(name) => {
                    self.dispatch(Action::SelectAgent(name));
                }
                SlashCommand::Help => {
                    self.show_help = !self.show_help;
                }
                SlashCommand::Quit => {
                    self.should_quit = true;
                }
            },
            Action::ComposerInput(key) => {
                if let ViewKind::Session = self.focus {
                    self.session.input_to_composer(key);
                    // Re-derive autocomplete from the first composer line.
                    let first_line = self.session.composer.lines()
                        .first().cloned().unwrap_or_default();
                    self.autocomplete = derive_autocomplete(&first_line);
                }
            }
            Action::PasteToComposer(content) => {
                if let ViewKind::Session = self.focus {
                    self.session.handle_paste(content);
                }
            }
            Action::OpenSessionPicker => {
                if let Some(agent_id) = self.active_agent_id.clone() {
                    self.pending
                        .push(AppCommand::LoadSessionsForPicker(agent_id));
                }
            }
            Action::ClosePicker => {
                self.show_session_picker = false;
            }
            Action::OpenAgentsModal => {
                self.show_agents_modal = true;
                // Fetch fresh agent list when opening so the operator sees
                // current state rather than whatever was cached.
                self.pending.push(AppCommand::RefreshAll);
            }
            Action::OpenHitlModal => {
                self.show_hitl_queue_modal = true;
                self.pending.push(AppCommand::RefreshAll);
            }
            Action::OpenEvolveModal => {
                self.show_evolve_modal = true;
                self.pending.push(AppCommand::RefreshAll);
            }
            Action::CloseModal => {
                // No J.0.1 modal is open in this branch (would have been
                // handled above).  Fall through to a no-op so the dispatch
                // match stays exhaustive.
            }
            Action::PickerUp => {
                if self.show_session_picker {
                    self.session_picker.move_up();
                }
            }
            Action::PickerDown => {
                if self.show_session_picker {
                    self.session_picker.move_down();
                }
            }
            Action::PickerSelect => {
                if self.show_session_picker
                    && let Some(session) = self.session_picker.selected()
                {
                    let id = session.id.clone();
                    self.show_session_picker = false;
                    self.pending.push(AppCommand::OpenSession(id));
                }
            }
            Action::RetryConnection => {
                // Operator hit retry while the disconnected banner is visible.
                // Re-queue a pending chat send, or reset the backoff timer.
                if let Some((agent, message)) = self.pending_retry.take() {
                    self.pending.push(AppCommand::RetrySendChat { agent, message });
                } else if let Some(banner) = &mut self.disconnect_banner {
                    banner.retry_at = Instant::now();
                }
            }
            Action::PopupUp => {
                if let Some(p) = &mut self.autocomplete { p.move_up(); }
            }
            Action::PopupDown => {
                if let Some(p) = &mut self.autocomplete { p.move_down(); }
            }
            Action::PopupSelect => {
                if let Some(popup) = self.autocomplete.take()
                    && let Some(item) = popup.selected_item()
                {
                    self.session.insert_autocomplete(
                        &popup.mode, &popup.filter, &item.insert,
                    );
                }
            }
            Action::PopupDismiss => { self.autocomplete = None; }
            // These are only dispatched via the modal intercept path above, so
            // reaching here means the modal was already closed.  Treat as no-op.
            Action::ApproveHitl(_)
            | Action::RejectHitl(_)
            | Action::EscalateHitl(_)
            | Action::DismissHitlModal => {}
            Action::CancelTurn => {
                // Only act when a turn is actually in flight.
                if self.turn_streaming
                    && let Some(session_id) = self.active_session_id.clone()
                {
                    self.status = Status::warn("cancelling…");
                    self.pending.push(AppCommand::CancelTurn(session_id));
                }
            }
            // J.1.4: inline transcript Approval block actions.
            // Dispatch HTTP command; runtime will update block status on success.
            Action::ApproveInlineBlock(id) => {
                self.pending.push(AppCommand::ApproveInlineBlock(id));
            }
            Action::RejectInlineBlock(id) => {
                self.pending.push(AppCommand::RejectInlineBlock(id));
            }
            Action::EscalateInlineBlock(id) => {
                self.pending.push(AppCommand::EscalateInlineBlock(id));
            }
            Action::NoOp => {}
        }
    }

    /// Hand an SSE update to the session view.  Separated so the runtime
    /// can route channel messages without holding a borrow across await.
    pub fn apply_sse(&mut self, update: SseUpdate) {
        match update {
            SseUpdate::Event(ev) => {
                // Always refresh active_session_id from incoming events so
                // that a newly-created session replaces any stale id already
                // held from a previous turn.
                if !ev.session_id.is_empty() {
                    self.active_session_id = Some(ev.session_id.clone());
                }
                self.session.apply_event(ev);
            }
            SseUpdate::State(s) => {
                // Only clear turn_streaming when the stream transitions out
                // of Reconnecting (in-flight) — not on the Connected state
                // that arrives at stream *start*, which would kill ESC cancel
                // before any tokens are generated.
                let was_streaming = self.connection == ConnectionState::Reconnecting;
                self.connection = s;
                self.session.set_connection(s);
                if was_streaming && s != ConnectionState::Reconnecting {
                    self.turn_streaming = false;
                }
                // Clear the disconnected banner the moment the gateway comes back;
                // create one when it goes away.
                if s == ConnectionState::Connected {
                    self.disconnect_banner = None;
                    self.pending_retry = None;
                } else if s == ConnectionState::Disconnected
                    && self.disconnect_banner.is_none()
                {
                    self.disconnect_banner = Some(DisconnectBanner::new());
                }
            }
        }
    }

    /// Called by the runtime when `SendChat` is about to be executed.
    /// Marks the turn as streaming and captures the session id so ESC can
    /// cancel it.  The session id is taken from `active_session_id` if
    /// already known; the runtime updates it after the POST response when
    /// the gateway echoes back the session.
    pub fn mark_turn_started(&mut self) {
        self.turn_streaming = true;
    }

    /// Called by the runtime when a cancel completes (success or no-op).
    pub fn mark_turn_cancelled(&mut self) {
        self.turn_streaming = false;
        self.status = Status::warn("turn cancelled");
    }

    /// Footer hint row — context-sensitive for the chat-dominant layout.
    /// Modal-open states show modal-relevant bindings; otherwise the hint
    /// reflects the composer/transcript focus.
    pub fn footer_hint(&self) -> String {
        let kb = &self.keybindings;

        // HITL inline approval modal takes priority — its bindings are
        // already in its own footer but we still swap the global hint so
        // the operator doesn't see stale composer hints.
        if self.show_hitl_modal.is_some() {
            return format!(
                "{}:approve  {}:reject  {}:escalate  {}:dismiss",
                display_first(&kb.approve),
                display_first(&kb.reject),
                display_first(&kb.escalate),
                display_first(&kb.back),
            );
        }

        if self.show_agents_modal {
            return format!(
                "{}:select  {}:↑  {}:↓  esc:close",
                display_first(&kb.select),
                display_first(&kb.up),
                display_first(&kb.down),
            );
        }
        if self.show_hitl_queue_modal {
            return format!(
                "{}:approve  {}:reject  {}:escalate  {}:↑  {}:↓  esc:close",
                display_first(&kb.approve),
                display_first(&kb.reject),
                display_first(&kb.escalate),
                display_first(&kb.up),
                display_first(&kb.down),
            );
        }
        if self.show_evolve_modal {
            return format!(
                "{}:↑  {}:↓  esc:close",
                display_first(&kb.up),
                display_first(&kb.down),
            );
        }
        if self.show_session_picker {
            return format!(
                "{}:select  {}:↑  {}:↓  esc:close",
                display_first(&kb.select),
                display_first(&kb.up),
                display_first(&kb.down),
            );
        }

        // Streaming turn — ESC cancels.
        if self.turn_streaming {
            return format!(
                "{}:cancel turn  {}:quit",
                display_first(&kb.cancel_turn),
                display_first(&kb.quit),
            );
        }

        // No modal — chat-dominant base hints.
        format!(
            "{}:send  {}:focus  {}:agents  {}:hitl  {}:evolve  {}:sessions  {}:quit",
            display_first(&kb.submit_message),
            display_first(&kb.toggle_composer_focus),
            display_first(&kb.open_agents_modal),
            display_first(&kb.open_hitl_modal),
            display_first(&kb.open_evolve_modal),
            display_first(&kb.open_session_picker),
            display_first(&kb.quit),
        )
    }
}

/// Show the modal for `req` if the active session belongs to its agent and no
/// modal is already open.  Called after every HITL refresh so the operator
/// gets an immediate pop-up when a request arrives for their current session.
pub fn maybe_show_hitl_modal(app: &mut App, req: HitlRequest) {
    if app.show_hitl_modal.is_none() {
        app.show_hitl_modal = Some(req);
    }
}

/// Apply a freshly-fetched HITL list to the app, preserving the auto-popup
/// behavior.  Split out so the modal-trigger logic stays on the main thread
/// after the fetch itself moved off the draw path onto a spawned task.
pub fn apply_hitl_update(app: &mut App, list: Vec<HitlRequest>) {
    let n = list.len();
    if let Some(active_agent) = app.active_agent_id.clone()
        && let Some(req) = list
            .iter()
            .find(|r| r.agent_id == active_agent && r.status == "pending")
            .cloned()
    {
        maybe_show_hitl_modal(app, req);
    }
    app.hitl.set_requests(list);
    app.status = Status::info(format!("{n} HITL request(s)"));
}

/// Apply a background fetch result to the app.  Called by the main loop
/// after draining `app_rx`.
///
/// Drops any result whose generation no longer matches `App`'s current
/// per-resource counter — that means a newer refresh has been issued
/// since this task was spawned, so the in-flight payload would be
/// stale relative to the operator's most recent intent.
pub fn apply_app_update(app: &mut App, update: AppUpdate) {
    match update {
        AppUpdate::Agents(seq, result) => {
            if seq != app.agents_gen {
                return;
            }
            match result {
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
        AppUpdate::Hitl(seq, result) => {
            if seq != app.hitl_gen {
                return;
            }
            match result {
                Ok(list) => apply_hitl_update(app, list),
                Err(e) => {
                    app.status = Status::warn(format!("HITL list unavailable: {e}"));
                }
            }
        }
        AppUpdate::Evolve(seq, result) => {
            if seq != app.evolve_gen {
                return;
            }
            match result {
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
    }
}

/// Derive an [] from the current first composer line.
fn derive_autocomplete(line: &str) -> Option<AutocompletePopup> {
    use crate::autocomplete::PopupMode;
    let (mode, filter) = detect_trigger(line)?;
    let cwd = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let popup = match mode {
        PopupMode::Slash => AutocompletePopup::for_slash(&filter),
        PopupMode::AtFile => AutocompletePopup::for_at_file(&filter, &cwd),
    };
    if popup.is_empty() { None } else { Some(popup) }
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
    pub app_tx: UnboundedSender<AppUpdate>,
}

impl Runtime {
    pub fn new(sse_tx: UnboundedSender<SseUpdate>, app_tx: UnboundedSender<AppUpdate>) -> Self {
        Self {
            sse_task: None,
            sse_tx,
            app_tx,
        }
    }

    /// Drain pending commands, spawning async work for each.  Refresh
    /// fetches run on detached tasks that ship their result through
    /// `app_tx`; user-driven HITL actions still await inline because
    /// they only fire in response to explicit operator input, not on
    /// the cold-start draw path.
    pub async fn execute(&mut self, app: &mut App) {
        let pending = std::mem::take(&mut app.pending);
        for cmd in pending {
            match cmd {
                AppCommand::RefreshAll => self.spawn_refresh_all(app),
                AppCommand::RefreshCurrent => self.spawn_refresh_focus(app),
                AppCommand::LoadSessionFor(agent_id) => {
                    self.load_session_for(app, agent_id).await;
                }
                AppCommand::Approve(id) | AppCommand::ApproveModal(id) => {
                    match app.client.approve_hitl(&id).await {
                        Ok(()) => {
                            app.status = Status::info(format!("approved {id}"));
                            self.spawn_refresh_hitl(app);
                        }
                        Err(e) => {
                            app.hitl.set_error(e.to_string());
                            app.status = Status::error(format!("approve failed: {e}"));
                        }
                    }
                }
                AppCommand::Reject(id) | AppCommand::RejectModal(id) => {
                    match app.client.reject_hitl(&id).await {
                        Ok(()) => {
                            app.status = Status::info(format!("rejected {id}"));
                            self.spawn_refresh_hitl(app);
                        }
                        Err(e) => {
                            app.hitl.set_error(e.to_string());
                            app.status = Status::error(format!("reject failed: {e}"));
                        }
                    }
                }
                AppCommand::Escalate(id) | AppCommand::EscalateModal(id) => {
                    match app.client.escalate_hitl(&id).await {
                        Ok(()) => {
                            app.status = Status::info(format!("escalated {id}"));
                            self.spawn_refresh_hitl(app);
                        }
                        Err(e) => {
                            app.hitl.set_error(e.to_string());
                            app.status = Status::error(format!("escalate failed: {e}"));
                        }
                    }
                }
                AppCommand::SendChat { agent, message } => {
                    self.send_chat(app, agent, message).await;
                }
                AppCommand::CancelTurn(session_id) => {
                    match app.client.cancel_turn(&session_id).await {
                        Ok(true) => {
                            app.mark_turn_cancelled();
                        }
                        Ok(false) => {
                            // Turn already finished — clear streaming flag.
                            app.mark_turn_cancelled();
                            app.status = Status::info("turn already completed");
                        }
                        Err(e) => {
                            app.turn_streaming = false;
                            app.status = Status::error(format!("cancel failed: {e}"));
                        }
                    }
                }
                AppCommand::RetrySendChat { agent, message } => {
                    // Immediate retry — same code path as SendChat.
                    self.send_chat(app, agent, message).await;
                }
                AppCommand::LoadSessionsForPicker(agent_id) => {
                    match app.client.list_sessions(Some(&agent_id)).await {
                        Ok(sessions) => {
                            let n = sessions.len();
                            app.session_picker.set_sessions(sessions);
                            app.show_session_picker = true;
                            app.status = Status::info(format!("{n} session(s) — use ↑/↓ Enter to resume, Esc to close"));
                        }
                        Err(e) => {
                            app.status = Status::error(format!("session list failed: {e}"));
                        }
                    }
                }
                AppCommand::OpenSession(session_id) => {
                    self.load_session_by_id(app, session_id).await;
                }
                // J.1.4: inline transcript Approval block actions.
                // Call the HITL HTTP endpoint; update the block status in the
                // transcript on success so the operator sees the resolved state.
                AppCommand::ApproveInlineBlock(id) => {
                    match app.client.approve_hitl(&id).await {
                        Ok(()) => {
                            app.session
                                .update_approval_status(&id, ApprovalStatus::Approved);
                            app.status = Status::info(format!("approved {id}"));
                            self.spawn_refresh_hitl(app);
                        }
                        Err(e) => {
                            app.status = Status::error(format!("approve failed: {e}"));
                        }
                    }
                }
                AppCommand::RejectInlineBlock(id) => {
                    match app.client.reject_hitl(&id).await {
                        Ok(()) => {
                            app.session
                                .update_approval_status(&id, ApprovalStatus::Rejected);
                            app.status = Status::info(format!("rejected {id}"));
                            self.spawn_refresh_hitl(app);
                        }
                        Err(e) => {
                            app.status = Status::error(format!("reject failed: {e}"));
                        }
                    }
                }
                AppCommand::EscalateInlineBlock(id) => {
                    match app.client.escalate_hitl(&id).await {
                        Ok(()) => {
                            app.session
                                .update_approval_status(&id, ApprovalStatus::Escalated);
                            app.status = Status::info(format!("escalated {id}"));
                            self.spawn_refresh_hitl(app);
                        }
                        Err(e) => {
                            app.status = Status::error(format!("escalate failed: {e}"));
                        }
                    }
                }
            }
        }
    }

    /// Spawn fetches for agents + HITL + evolve concurrently; results land
    /// on `app_tx`.
    fn spawn_refresh_all(&self, app: &mut App) {
        self.spawn_refresh_agents(app);
        self.spawn_refresh_hitl(app);
        self.spawn_refresh_evolve(app);
    }

    fn spawn_refresh_focus(&self, app: &mut App) {
        match app.focus {
            ViewKind::Agents => self.spawn_refresh_agents(app),
            ViewKind::Session => { /* driven by SSE + explicit load */ }
            ViewKind::Hitl => self.spawn_refresh_hitl(app),
            ViewKind::Evolve => self.spawn_refresh_evolve(app),
        }
    }

    fn spawn_refresh_agents(&self, app: &mut App) {
        app.agents_gen = app.agents_gen.wrapping_add(1);
        let seq = app.agents_gen;
        let client = Arc::clone(&app.client);
        let tx = self.app_tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(AppUpdate::Agents(seq, client.list_agents().await));
        });
    }

    fn spawn_refresh_hitl(&self, app: &mut App) {
        app.hitl_gen = app.hitl_gen.wrapping_add(1);
        let seq = app.hitl_gen;
        let client = Arc::clone(&app.client);
        let tx = self.app_tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(AppUpdate::Hitl(seq, client.list_hitl().await));
        });
    }

    fn spawn_refresh_evolve(&self, app: &mut App) {
        app.evolve_gen = app.evolve_gen.wrapping_add(1);
        let seq = app.evolve_gen;
        let client = Arc::clone(&app.client);
        let tx = self.app_tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(AppUpdate::Evolve(seq, client.list_evolve_proposals().await));
        });
    }

    /// Spawn a task that POSTs to `/api/chat` and pipes SSE events into the
    /// session view via the existing `sse_tx` channel.
    async fn send_chat(&mut self, app: &mut App, agent: String, message: String) {
        let client = Arc::clone(&app.client);
        let forward_to = self.sse_tx.clone();

        // Mark turn as in-flight so ESC routes to CancelTurn.
        app.mark_turn_started();
        // Stash the payload for RetryConnection before moving into the spawn.
        // Cleared by apply_sse when Connected is received.
        app.pending_retry = Some((agent.clone(), message.clone()));

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

    /// Load a specific session by id (called from picker selection).
    async fn load_session_by_id(&mut self, app: &mut App, session_id: String) {
        // Fetch transcript and re-subscribe SSE for the chosen session.
        let transcript = app
            .client
            .session_transcript(&session_id)
            .await
            .unwrap_or_default();

        // Build a minimal SessionSummary for the view header.
        let summary = crate::client::SessionSummary {
            id: session_id.clone(),
            agent_id: app.active_agent_id.clone().unwrap_or_default(),
            created_at: String::new(),
            state: "active".to_owned(),
        };
        app.session.set_session(summary);
        app.session.set_transcript(transcript);
        app.focus = ViewKind::Session;

        if let Some(handle) = self.sse_task.take() {
            handle.abort();
        }
        let (bridge_tx, mut bridge_rx) = mpsc::channel::<SseUpdate>(64);
        let forward_to = self.sse_tx.clone();
        tokio::spawn(async move {
            while let Some(u) = bridge_rx.recv().await {
                if forward_to.send(u).is_err() {
                    break;
                }
            }
        });
        self.sse_task = Some(app.client.spawn_sse(session_id.clone(), bridge_tx));
        app.active_session_id = Some(session_id.clone());
        app.status = Status::info(format!("resumed session {session_id}"));
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
                    app.active_session_id = Some(session.id.clone());
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
            parent_task_id: None,
        }));
        assert_eq!(app.session.blocks.len(), 1);
        match &app.session.blocks[0] {
            crate::views::blocks::Block::AssistantMessage { text, .. } => assert_eq!(text, "hi"),
            other => panic!("expected AssistantMessage, got {other:?}"),
        }
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
    fn footer_hint_changes_with_modal_state() {
        // J.0.1: footer_hint reflects modal-open state, not the legacy
        // ViewKind focus.  The base (no-modal) hint mentions "send" for
        // the composer; opening the HITL queue modal swaps in approve.
        let mut app = App::new(client(), TuiKeybindings::defaults());
        let base_hint = app.footer_hint();
        assert!(base_hint.contains("send"));
        app.show_hitl_queue_modal = true;
        let hitl_hint = app.footer_hint();
        assert_ne!(base_hint, hitl_hint);
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

    // --- ExecuteSlash dispatch tests (G.1.1) ---

    #[test]
    fn execute_slash_new_clears_block_transcript() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.session.blocks.push(crate::views::blocks::Block::UserMessage {
            text: "old".into(),
        });
        app.session.blocks.push(crate::views::blocks::Block::ToolCall {
            tool: "bash".into(),
            summary: "echo".into(),
            args: serde_json::Value::Null,
            result: None,
            expanded: false,
        });
        app.dispatch(Action::ExecuteSlash(SlashCommand::New));
        assert!(app.session.blocks.is_empty());
        assert_eq!(app.status.text, "new turn");
    }

    #[test]
    fn execute_slash_quit_sets_should_quit() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.dispatch(Action::ExecuteSlash(SlashCommand::Quit));
        assert!(app.should_quit);
    }

    #[test]
    fn execute_slash_agent_delegates_to_select_agent() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.dispatch(Action::ExecuteSlash(SlashCommand::Agent("bot-7".to_owned())));
        assert_eq!(app.active_agent_id.as_deref(), Some("bot-7"));
        assert_eq!(app.focus, ViewKind::Session);
        assert!(matches!(
            app.pending.last(),
            Some(AppCommand::LoadSessionFor(id)) if id == "bot-7"
        ));
    }

    #[test]
    fn execute_slash_help_toggles_show_help() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        assert!(!app.show_help);
        app.dispatch(Action::ExecuteSlash(SlashCommand::Help));
        assert!(app.show_help);
        app.dispatch(Action::ExecuteSlash(SlashCommand::Help));
        assert!(!app.show_help);
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
        // Dispatch is pure — blocks not cleared yet (that's runtime's job).
        // But active_agent_id is set and LoadSessionFor is queued.
        assert_eq!(app.active_agent_id.as_deref(), Some("fresh-agent"));
        assert!(matches!(
            app.pending.last(),
            Some(AppCommand::LoadSessionFor(id)) if id == "fresh-agent"
        ));
    }

    // --- sera-e7fp: non-blocking startup tests ---

    /// First frame must be renderable with no prior network round-trip:
    /// pre-loop, the loop seeds an `AppCommand::RefreshAll` and then
    /// `terminal.draw` runs.  This test stands in for the latter against
    /// a `TestBackend`, proving `ui::render` does not require any
    /// data to be loaded first.
    #[test]
    fn first_frame_renders_before_any_refresh_completes() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut app = App::new(client(), TuiKeybindings::defaults());
        // Mirror what main.rs does at startup.
        app.status = Status::info("connecting…");
        app.pending.push(AppCommand::RefreshAll);

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|f| crate::ui::render(f, &mut app))
            .expect("first frame draws without network");

        let buf = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                rendered.push_str(buf[(x, y)].symbol());
            }
            rendered.push('\n');
        }
        // Status text seeded before the first draw must be visible.
        assert!(
            rendered.contains("connecting"),
            "first frame should surface the connecting status; got:\n{rendered}"
        );
    }

    #[test]
    fn apply_app_update_agents_ok_populates_view() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        // Simulate a spawn bumping the generation.
        app.agents_gen = 1;
        apply_app_update(
            &mut app,
            AppUpdate::Agents(1, Ok(vec![agent("x"), agent("y")])),
        );
        assert_eq!(app.agents.selected_id().as_deref(), Some("x"));
        assert!(app.status.text.contains("2 agent"));
    }

    #[test]
    fn apply_app_update_agents_err_sets_error_status() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.agents_gen = 1;
        apply_app_update(
            &mut app,
            AppUpdate::Agents(1, Err(ClientError::NotAvailable("/api/agents".into()))),
        );
        assert!(matches!(app.status.level, StatusLevel::Error));
        assert!(app.status.text.contains("agent list failed"));
    }

    #[test]
    fn apply_hitl_update_pops_modal_for_active_agent() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.active_agent_id = Some("a1".into());
        apply_hitl_update(
            &mut app,
            vec![HitlRequest {
                id: "h1".into(),
                agent_id: "a1".into(),
                summary: "read".into(),
                age: "".into(),
                status: "pending".into(),
            }],
        );
        assert!(app.show_hitl_modal.is_some());
        assert_eq!(app.show_hitl_modal.as_ref().unwrap().id, "h1");
    }

    #[test]
    fn apply_hitl_update_skips_modal_when_no_active_agent() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        apply_hitl_update(
            &mut app,
            vec![HitlRequest {
                id: "h1".into(),
                agent_id: "a1".into(),
                summary: "read".into(),
                age: "".into(),
                status: "pending".into(),
            }],
        );
        assert!(app.show_hitl_modal.is_none());
        assert_eq!(app.hitl.selected_id().as_deref(), Some("h1"));
    }

    /// When two refreshes overlap (e.g. modal-open `RefreshAll` followed
    /// by an `Approve` post-success refresh), the older response must
    /// not overwrite the newer one's state.
    #[test]
    fn apply_app_update_drops_stale_generation() {
        let mut app = App::new(client(), TuiKeybindings::defaults());

        // Two spawns issued in order: gen 1, then gen 2.
        app.agents_gen = 2;

        // Newer response (gen 2) lands first and applies cleanly.
        apply_app_update(
            &mut app,
            AppUpdate::Agents(2, Ok(vec![agent("new")])),
        );
        assert_eq!(app.agents.selected_id().as_deref(), Some("new"));
        let status_after_new = app.status.text.clone();

        // Older response (gen 1) arrives late — must be dropped.
        apply_app_update(
            &mut app,
            AppUpdate::Agents(1, Ok(vec![agent("stale")])),
        );
        assert_eq!(
            app.agents.selected_id().as_deref(),
            Some("new"),
            "stale gen 1 must not overwrite the newer gen 2 payload"
        );
        assert_eq!(
            app.status.text, status_after_new,
            "stale gen must not touch the status line either"
        );
    }

    #[test]
    fn apply_app_update_drops_stale_hitl_and_evolve() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.hitl_gen = 5;
        app.evolve_gen = 5;

        // Old HITL response should not pop a modal or touch state.
        app.active_agent_id = Some("a1".into());
        apply_app_update(
            &mut app,
            AppUpdate::Hitl(
                4,
                Ok(vec![HitlRequest {
                    id: "stale".into(),
                    agent_id: "a1".into(),
                    summary: "".into(),
                    age: "".into(),
                    status: "pending".into(),
                }]),
            ),
        );
        assert!(
            app.show_hitl_modal.is_none(),
            "stale HITL must not trigger the auto-popup"
        );
        assert_eq!(app.hitl.selected_id(), None);

        // Old evolve response should not touch the proposals view.
        let baseline_status = app.status.text.clone();
        apply_app_update(
            &mut app,
            AppUpdate::Evolve(4, Err(ClientError::NotAvailable("/api/evolve".into()))),
        );
        assert_eq!(
            app.status.text, baseline_status,
            "stale evolve error must not surface in the status line"
        );
    }

    // --- J.0.4: CancelTurn dispatch tests (sera-zate) ---

    #[test]
    fn cancel_turn_while_streaming_emits_command() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.turn_streaming = true;
        app.active_session_id = Some("ses-abc".into());
        app.dispatch(Action::CancelTurn);
        assert!(matches!(
            app.pending.last(),
            Some(AppCommand::CancelTurn(id)) if id == "ses-abc"
        ));
        assert!(app.status.text.contains("cancelling"));
    }

    #[test]
    fn cancel_turn_when_not_streaming_is_noop() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.turn_streaming = false;
        app.active_session_id = Some("ses-abc".into());
        app.dispatch(Action::CancelTurn);
        assert!(app.pending.is_empty());
    }

    #[test]
    fn cancel_turn_without_session_id_is_noop() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.turn_streaming = true;
        app.active_session_id = None;
        app.dispatch(Action::CancelTurn);
        assert!(app.pending.is_empty());
    }

    #[test]
    fn apply_sse_event_captures_session_id() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        assert!(app.active_session_id.is_none());
        app.apply_sse(SseUpdate::Event(StreamEvent {
            event_type: "message".into(),
            session_id: "ses-xyz".into(),
            role: "assistant".into(),
            delta: "hi".into(),
            tool: String::new(),
            parent_task_id: None,
        }));
        assert_eq!(app.active_session_id.as_deref(), Some("ses-xyz"));
    }

    #[test]
    fn apply_sse_event_refreshes_stale_session_id() {
        // A stale active_session_id (e.g. from a previous turn / agent)
        // must be replaced when a new session id arrives, otherwise
        // CancelTurn would POST against the wrong session.
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.active_session_id = Some("ses-old".into());
        app.apply_sse(SseUpdate::Event(StreamEvent {
            event_type: "message".into(),
            session_id: "ses-new".into(),
            role: "assistant".into(),
            delta: "hi".into(),
            tool: String::new(),
            parent_task_id: None,
        }));
        assert_eq!(app.active_session_id.as_deref(), Some("ses-new"));
    }

    #[test]
    fn apply_sse_state_connected_clears_turn_streaming() {
        // turn_streaming should only be cleared when the stream actually
        // ends — i.e. when transitioning OUT of Reconnecting.  The Connected
        // state that fires at stream *start* (right after the POST) must
        // not clear it, otherwise ESC cancel is unreachable during the
        // generation window.
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.turn_streaming = true;
        // Mid-flight: connection enters Reconnecting (in-flight phase).
        app.apply_sse(SseUpdate::State(ConnectionState::Reconnecting));
        assert!(app.turn_streaming, "still streaming during Reconnecting");
        // Stream-start Connected emitted after POST returns OK — turn is
        // still streaming tokens, ESC must still cancel.
        app.apply_sse(SseUpdate::State(ConnectionState::Connected));
        assert!(
            !app.turn_streaming,
            "transition out of Reconnecting clears turn_streaming"
        );
    }

    // ── J.0.7 disconnect-banner tests ──────────────────────────────────────

    #[test]
    fn disconnect_banner_new_has_1s_backoff() {
        let b = DisconnectBanner::new();
        assert_eq!(b.backoff.as_secs(), 1);
    }

    #[test]
    fn disconnect_banner_advance_doubles_capped_at_30s() {
        let mut b = DisconnectBanner::new();
        b.advance(); assert_eq!(b.backoff.as_secs(), 2);
        b.advance(); assert_eq!(b.backoff.as_secs(), 4);
        b.advance(); assert_eq!(b.backoff.as_secs(), 8);
        b.advance(); assert_eq!(b.backoff.as_secs(), 16);
        b.advance(); assert_eq!(b.backoff.as_secs(), 30);
        b.advance(); assert_eq!(b.backoff.as_secs(), 30, "should not exceed 30s cap");
    }

    #[test]
    fn apply_sse_connected_clears_banner_and_pending_retry() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.disconnect_banner = Some(DisconnectBanner::new());
        app.pending_retry = Some(("agent".into(), "msg".into()));
        app.apply_sse(SseUpdate::State(ConnectionState::Connected));
        assert!(app.disconnect_banner.is_none(), "banner must clear on connect");
        assert!(app.pending_retry.is_none(), "pending_retry must clear on connect");
    }

    #[test]
    fn apply_sse_disconnected_creates_banner_when_none() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        assert!(app.disconnect_banner.is_none());
        app.apply_sse(SseUpdate::State(ConnectionState::Disconnected));
        assert!(app.disconnect_banner.is_some(), "banner must appear on disconnect");
    }

    #[test]
    fn apply_sse_disconnected_preserves_existing_banner_backoff() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        let mut b = DisconnectBanner::new();
        b.advance(); // backoff now 2s
        app.disconnect_banner = Some(b);
        app.apply_sse(SseUpdate::State(ConnectionState::Disconnected));
        assert_eq!(
            app.disconnect_banner.as_ref().unwrap().backoff.as_secs(),
            2,
            "second disconnect must not reset existing banner backoff"
        );
    }

    #[test]
    fn apply_sse_state_connected_without_prior_reconnecting_keeps_turn_streaming() {
        // Defensive: a stray Connected that does not follow Reconnecting
        // (e.g. unrelated reconnection signal) must NOT prematurely end
        // the streaming window.
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.turn_streaming = true;
        // connection starts at Disconnected; jump straight to Connected.
        app.apply_sse(SseUpdate::State(ConnectionState::Connected));
        assert!(
            app.turn_streaming,
            "Connected without a prior Reconnecting must not clear turn_streaming"
        );
    }

    #[test]
    fn retry_connection_with_pending_retry_queues_command() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.pending_retry = Some(("ag".into(), "hello".into()));
        app.dispatch(Action::RetryConnection);
        assert!(app.pending_retry.is_none(), "pending_retry must be consumed");
        assert!(
            matches!(
                app.pending.last(),
                Some(AppCommand::RetrySendChat { agent, message })
                    if agent == "ag" && message == "hello"
            ),
            "expected RetrySendChat command"
        );
    }

    // --- J.1.4: inline HITL approval block dispatch tests ---

    #[test]
    fn approve_inline_block_routes_to_approve_inline_command() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.focus = ViewKind::Session;
        // Seed a pending approval block.
        app.session
            .push_approval("req-99".into(), "Write".into(), "reason".into());
        assert_eq!(app.session.first_pending_approval_id(), Some("req-99"));

        // Dispatching Approve with a pending block should push ApproveInlineBlock.
        app.dispatch(Action::Approve);
        assert!(
            matches!(
                app.pending.last(),
                Some(AppCommand::ApproveInlineBlock(id)) if id == "req-99"
            ),
            "expected ApproveInlineBlock(req-99), got: {:?}",
            app.pending.last()
        );
    }

    #[test]
    fn apply_sse_state_reconnecting_keeps_turn_streaming() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.turn_streaming = true;
        app.apply_sse(SseUpdate::State(ConnectionState::Reconnecting));
        assert!(app.turn_streaming);
    }

    #[test]
    fn footer_hint_shows_cancel_when_streaming() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.turn_streaming = true;
        let hint = app.footer_hint();
        assert!(hint.contains("cancel turn"), "streaming hint must mention cancel turn; got: {hint}");
    }

    #[test]
    fn footer_hint_shows_send_when_not_streaming() {
        let app = App::new(client(), TuiKeybindings::defaults());
        let hint = app.footer_hint();
        assert!(hint.contains("send"), "idle hint must mention send; got: {hint}");
    }

    #[test]
    fn retry_connection_without_pending_retry_resets_banner_timer() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        let mut b = DisconnectBanner::new();
        b.retry_at = std::time::Instant::now() + std::time::Duration::from_secs(60);
        app.disconnect_banner = Some(b);
        app.dispatch(Action::RetryConnection);
        let secs = app.disconnect_banner.as_ref().unwrap().secs_until_retry();
        assert_eq!(secs, 0, "banner timer must be reset to now");
    }

    #[test]
    fn reject_inline_block_routes_to_reject_inline_command() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.focus = ViewKind::Session;
        app.session
            .push_approval("req-7".into(), "Bash".into(), "cmd".into());
        app.dispatch(Action::Reject);
        assert!(
            matches!(
                app.pending.last(),
                Some(AppCommand::RejectInlineBlock(id)) if id == "req-7"
            ),
            "expected RejectInlineBlock(req-7), got: {:?}",
            app.pending.last()
        );
    }

    #[test]
    fn escalate_inline_block_routes_to_escalate_inline_command() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        app.focus = ViewKind::Session;
        app.session
            .push_approval("req-3".into(), "Read".into(), "sensitive file".into());
        app.dispatch(Action::Escalate);
        assert!(
            matches!(
                app.pending.last(),
                Some(AppCommand::EscalateInlineBlock(id)) if id == "req-3"
            ),
            "expected EscalateInlineBlock(req-3), got: {:?}",
            app.pending.last()
        );
    }

    #[test]
    fn approve_falls_through_to_hitl_queue_when_no_pending_approval_block() {
        let mut app = App::new(client(), TuiKeybindings::defaults());
        // No approval blocks in transcript — focus HITL view and add a request.
        app.focus = ViewKind::Hitl;
        app.hitl.set_requests(vec![HitlRequest {
            id: "h1".into(),
            agent_id: "a1".into(),
            summary: "read".into(),
            age: "".into(),
            status: "pending".into(),
        }]);
        app.dispatch(Action::Approve);
        // Should have routed to the HITL queue pane handler.
        assert!(
            matches!(app.pending.last(), Some(AppCommand::Approve(id)) if id == "h1"),
            "expected Approve(h1), got: {:?}",
            app.pending.last()
        );
    }
}
