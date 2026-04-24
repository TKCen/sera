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
    if matches_key(event, &kb.open_session_picker) {
        return Action::OpenSessionPicker;
    }
    if matches_key(event, &kb.open_agents_modal) {
        return Action::OpenAgentsModal;
    }
    if matches_key(event, &kb.open_hitl_modal) {
        return Action::OpenHitlModal;
    }
    if matches_key(event, &kb.open_evolve_modal) {
        return Action::OpenEvolveModal;
    }
    Action::NoOp
}

/// Session-view translator.  When `composer_focused` is true the composer
/// textarea intercepts most keys; only global exits (quit) and the two
/// session-specific bindings are checked first.
///
/// When `composer_focused` is false the transcript pane is active and we
/// fall back to the standard `translate` so scroll keys work normally.
pub fn translate_session(
    event: &KeyEvent,
    kb: &TuiKeybindings,
    composer_focused: bool,
) -> Action {
    // Global exits always win regardless of composer state.
    if matches_key(event, &kb.quit) {
        return Action::Quit;
    }

    // Session-specific bindings checked before the global table.
    if matches_key(event, &kb.submit_message) {
        return Action::SubmitComposer;
    }
    if matches_key(event, &kb.toggle_composer_focus) {
        return Action::ToggleComposerFocus;
    }

    // J.0.1 modal-open shortcuts — handled even when composer has focus so
    // Ctrl+A/H/E open their modal instead of reaching the textarea.
    if matches_key(event, &kb.open_agents_modal) {
        return Action::OpenAgentsModal;
    }
    if matches_key(event, &kb.open_hitl_modal) {
        return Action::OpenHitlModal;
    }
    if matches_key(event, &kb.open_evolve_modal) {
        return Action::OpenEvolveModal;
    }
    if matches_key(event, &kb.open_session_picker) {
        return Action::OpenSessionPicker;
    }

    if composer_focused {
        // All other keys go straight to the textarea widget.
        Action::ComposerInput(*event)
    } else {
        // Transcript is focused — standard navigation.
        translate(event, kb)
    }
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

    #[test]
    fn session_tab_toggles_composer_focus() {
        let kb = TuiKeybindings::defaults();
        assert_eq!(
            translate_session(&ev(KeyCode::Tab), &kb, true),
            Action::ToggleComposerFocus,
        );
        assert_eq!(
            translate_session(&ev(KeyCode::Tab), &kb, false),
            Action::ToggleComposerFocus,
        );
    }

    #[test]
    fn session_ctrl_enter_submits() {
        let kb = TuiKeybindings::defaults();
        let ctrl_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(
            translate_session(&ctrl_enter, &kb, true),
            Action::SubmitComposer,
        );
        assert_eq!(
            translate_session(&ctrl_enter, &kb, false),
            Action::SubmitComposer,
        );
    }

    #[test]
    fn session_plain_key_goes_to_composer_when_focused() {
        let kb = TuiKeybindings::defaults();
        let key_h = ev(KeyCode::Char('h'));
        assert_eq!(
            translate_session(&key_h, &kb, true),
            Action::ComposerInput(key_h),
        );
    }

    #[test]
    fn session_plain_key_scrolls_when_transcript_focused() {
        let kb = TuiKeybindings::defaults();
        assert_eq!(
            translate_session(&ev(KeyCode::Up), &kb, false),
            Action::Up,
        );
    }

    #[test]
    fn session_quit_always_wins() {
        let kb = TuiKeybindings::defaults();
        assert_eq!(
            translate_session(&ev(KeyCode::Char('q')), &kb, true),
            Action::Quit,
        );
    }
}
