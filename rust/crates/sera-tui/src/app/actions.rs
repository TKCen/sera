//! `Action` — the one-way dispatch target for every user input.
//!
//! The event loop translates crossterm events into `Action`s via
//! [`crate::input`], then feeds them to the application state reducer in
//! [`super::dispatch`].  Keeping the middle layer as a plain enum makes
//! the app testable without a live terminal — a test can instantiate an
//! `AppState`, send a sequence of `Action`s, and assert on the resulting
//! state.

/// Which top-level pane is currently focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewKind {
    Agents,
    Session,
    Hitl,
    Evolve,
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

    /// Short label for header/footer chrome.
    pub fn label(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::Session => "Session",
            Self::Hitl => "HITL",
            Self::Evolve => "Evolve",
        }
    }
}

/// Every input the reducer understands.  Kept flat on purpose — nested
/// actions burn test surface for little payoff at this size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
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
    /// Select a specific agent by ID and switch to the Session pane.
    /// Dispatched when the AgentList confirms a selection (Enter on a row).
    /// Sets `App.active_agent_id` and triggers session load via
    /// `AppCommand::LoadSessionFor`.
    SelectAgent(String),
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
    fn view_label_is_non_empty() {
        for v in [ViewKind::Agents, ViewKind::Session, ViewKind::Hitl, ViewKind::Evolve] {
            assert!(!v.label().is_empty(), "label empty for {v:?}");
        }
    }

    #[test]
    fn action_equality_covers_common_pairs() {
        assert_eq!(Action::Quit, Action::Quit);
        assert_ne!(Action::Quit, Action::Refresh);
    }
}
