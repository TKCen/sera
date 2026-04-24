//! `Action` — the one-way dispatch target for every user input.
//!
//! The event loop translates crossterm events into `Action`s via
//! [`crate::input`], then feeds them to the application state reducer in
//! [`super::dispatch`].  Keeping the middle layer as a plain enum makes
//! the app testable without a live terminal — a test can instantiate an
//! `AppState`, send a sequence of `Action`s, and assert on the resulting
//! state.

/// Which top-level pane is currently focused.
///
/// **J.0.1 layout pivot**: view rotation (NextView/PrevView) is no longer the
/// primary navigator — the TUI is chat-dominant and the main canvas is always
/// the Session view.  `ViewKind` is kept because `Focus` (see below) does not
/// yet subsume the modal-dispatch branches in tests; it is effectively unused
/// by the new rendering path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewKind {
    Agents,
    Session,
    Hitl,
    Evolve,
}

/// Which region of the chat-dominant layout currently has keyboard focus.
///
/// `Composer` is the default — the user types into the multi-line textarea.
/// `Transcript` is entered for scroll navigation.  Modals (agents, HITL,
/// evolve, session picker) are tracked separately on the `App` struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Focus {
    Composer,
    /// Entered for scroll navigation — constructed by J.0.2 (Tab toggle)
    /// once the block-based transcript lands; reserved here in J.0.1 so
    /// the rest of the app code can pattern-match exhaustively.
    #[allow(dead_code)]
    Transcript,
}

impl ViewKind {
    /// The round-robin next pane; wraps at the last variant.
    pub fn next(self) -> Self {
        match self {
            Self::Agents => Self::Session,
            Self::Session => Self::Hitl,
            Self::Hitl => Self::Evolve,
            Self::Evolve => Self::Agents,
        }
    }

    /// The round-robin previous pane; wraps at the first variant.
    pub fn prev(self) -> Self {
        match self {
            Self::Agents => Self::Evolve,
            Self::Session => Self::Agents,
            Self::Hitl => Self::Session,
            Self::Evolve => Self::Hitl,
        }
    }

}

/// Every input the reducer understands.  Kept flat on purpose — nested
/// actions burn test surface for little payoff at this size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Execute a parsed slash command from the composer.
    ExecuteSlash(super::slash::SlashCommand),
    Refresh,
    NextView,
    PrevView,
    Up,
    Down,
    PageUp,
    PageDown,
    Select,
    Back,
    Approve,
    Reject,
    Escalate,
    /// End-of-buffer; only the Session view listens for it, where it
    /// re-enables auto-scroll after the user paused it by scrolling up.
    EndOfBuffer,
    /// Toggle focus between the composer and transcript inside SessionView.
    ToggleComposerFocus,
    /// Submit the composer buffer (Session view only).  Drains the textarea
    /// into `pending_sends`; G.0.2 will wire the send to the gateway.
    SubmitComposer,
    /// Forward a raw key event to the focused composer textarea.
    ComposerInput(crossterm::event::KeyEvent),
    /// Route a bracketed-paste payload to the composer (Session view only).
    PasteToComposer(String),
    /// Select a specific agent by ID and switch to the Session pane.
    /// Dispatched when the AgentList confirms a selection (Enter on a row).
    /// Sets `App.active_agent_id` and triggers session load via
    /// `AppCommand::LoadSessionFor`.
    SelectAgent(String),
    /// Open the session picker modal for the current agent.
    OpenSessionPicker,
    /// Close the session picker without selecting.
    ClosePicker,
    /// Open the agents picker modal (chat-dominant layout, default Ctrl+A).
    OpenAgentsModal,
    /// Open the HITL queue modal (chat-dominant layout, default Ctrl+H).
    OpenHitlModal,
    /// Open the evolve status modal (chat-dominant layout, default Ctrl+E).
    OpenEvolveModal,
    /// Close whichever J.0.1 modal (agents, hitl, evolve) is currently open.
    CloseModal,
    /// Move picker selection up.
    PickerUp,
    /// Move picker selection down.
    PickerDown,
    /// Confirm the currently highlighted session.
    PickerSelect,
    /// Approve the HITL request currently shown in the inline modal.
    /// Constructed by external callers; the modal intercept in `App::dispatch`
    /// maps `Action::Approve` directly to `AppCommand::ApproveModal`.
    #[allow(dead_code)]
    ApproveHitl(String),
    /// Reject the HITL request currently shown in the inline modal.
    #[allow(dead_code)]
    RejectHitl(String),
    /// Escalate the HITL request currently shown in the inline modal.
    #[allow(dead_code)]
    EscalateHitl(String),
    /// Dismiss the inline HITL modal without taking action (leaves request
    /// in the HITL queue pane).
    #[allow(dead_code)]
    DismissHitlModal,
    /// No-op — used when a key doesn't match any binding.  Reducing to
    /// this instead of returning `Option<Action>` lets the dispatch table
    /// stay a plain `match`.
    NoOp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_next_wraps() {
        assert_eq!(ViewKind::Agents.next(), ViewKind::Session);
        assert_eq!(ViewKind::Session.next(), ViewKind::Hitl);
        assert_eq!(ViewKind::Hitl.next(), ViewKind::Evolve);
        assert_eq!(ViewKind::Evolve.next(), ViewKind::Agents);
    }

    #[test]
    fn view_prev_wraps() {
        assert_eq!(ViewKind::Agents.prev(), ViewKind::Evolve);
        assert_eq!(ViewKind::Session.prev(), ViewKind::Agents);
        assert_eq!(ViewKind::Hitl.prev(), ViewKind::Session);
        assert_eq!(ViewKind::Evolve.prev(), ViewKind::Hitl);
    }

    #[test]
    fn action_equality_covers_common_pairs() {
        assert_eq!(Action::Quit, Action::Quit);
        assert_ne!(Action::Quit, Action::Refresh);
    }
}
