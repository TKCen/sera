//! crossterm `KeyEvent` → [`Action`] translation.
//!
//! Split out so the event loop can unit-test the key map without importing
//! crossterm's `Event` hierarchy.

use crossterm::event::KeyEvent;

use crate::app::actions::Action;
use crate::keybindings::{matches_key, TuiKeybindings};

/// Return the first matching [`Action`] for `event`, or [`Action::NoOp`]
/// when no binding applies.
///
/// Order matters: earlier checks win.  We lead with navigation / view
/// switching because those are hit the hardest and the remaining branches
/// are cheap `==` comparisons.
pub fn translate(event: &KeyEvent, kb: &TuiKeybindings) -> Action {
    if matches_key(event, &kb.quit) {
        return Action::Quit;
    }
    if matches_key(event, &kb.refresh) {
        return Action::Refresh;
    }
    if matches_key(event, &kb.next_view) {
        return Action::NextView;
    }
    if matches_key(event, &kb.prev_view) {
        return Action::PrevView;
    }
    if matches_key(event, &kb.up) {
        return Action::Up;
    }
    if matches_key(event, &kb.down) {
        return Action::Down;
    }
    if matches_key(event, &kb.page_up) {
        return Action::PageUp;
    }
    if matches_key(event, &kb.page_down) {
        return Action::PageDown;
    }
    if matches_key(event, &kb.select) {
        return Action::Select;
    }
    if matches_key(event, &kb.back) {
        return Action::Back;
    }
    if matches_key(event, &kb.approve) {
        return Action::Approve;
    }
    if matches_key(event, &kb.reject) {
        return Action::Reject;
    }
    if matches_key(event, &kb.escalate) {
        return Action::Escalate;
    }
    if matches_key(event, &kb.end_of_buffer) {
        return Action::EndOfBuffer;
    }
    Action::NoOp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn ev(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn q_translates_to_quit() {
        let kb = TuiKeybindings::defaults();
        assert_eq!(translate(&ev(KeyCode::Char('q')), &kb), Action::Quit);
    }

    #[test]
    fn r_translates_to_refresh() {
        let kb = TuiKeybindings::defaults();
        assert_eq!(translate(&ev(KeyCode::Char('r')), &kb), Action::Refresh);
    }

    #[test]
    fn tab_translates_to_next_view() {
        let kb = TuiKeybindings::defaults();
        assert_eq!(translate(&ev(KeyCode::Tab), &kb), Action::NextView);
    }

    #[test]
    fn approve_reject_escalate_are_bound() {
        let kb = TuiKeybindings::defaults();
        assert_eq!(translate(&ev(KeyCode::Char('a')), &kb), Action::Approve);
        assert_eq!(translate(&ev(KeyCode::Char('x')), &kb), Action::Reject);
        assert_eq!(translate(&ev(KeyCode::Char('e')), &kb), Action::Escalate);
    }

    #[test]
    fn unknown_key_is_noop() {
        let kb = TuiKeybindings::defaults();
        assert_eq!(translate(&ev(KeyCode::Char('~')), &kb), Action::NoOp);
    }

    #[test]
    fn ctrl_c_translates_to_quit() {
        let kb = TuiKeybindings::defaults();
        let ev = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(translate(&ev, &kb), Action::Quit);
    }
}
