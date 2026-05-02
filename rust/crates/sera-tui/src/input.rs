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
///
/// `turn_streaming` is true while a chat turn is actively in flight.  When
/// it is set, the `cancel_turn` binding (default: Esc) takes highest
/// precedence so the operator can interrupt the stream immediately.
pub fn translate_session(
    event: &KeyEvent,
    kb: &TuiKeybindings,
    composer_focused: bool,
    turn_streaming: bool,
    disconnected: bool,
    popup_open: bool,
) -> Action {
    // Global exits always win regardless of composer or streaming state.
    if matches_key(event, &kb.quit) {
        return Action::Quit;
    }

    // J.0.4: while a turn is streaming, ESC (cancel_turn binding) takes
    // precedence over all other bindings — including Back and modal-open
    // shortcuts.  The dispatch layer only acts on CancelTurn when
    // `App::turn_streaming` is true, so a false-positive here is a no-op.
    if turn_streaming && matches_key(event, &kb.cancel_turn) {
        return Action::CancelTurn;
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
    // Retry is only intercepted while the disconnect banner is shown, so
    // plain `r` still reaches the composer / standard refresh otherwise.
    if disconnected && matches_key(event, &kb.retry_connection) {
        return Action::RetryConnection;
    }


    // J.0.5: popup intercept — navigation drives the popup, not the textarea.
    if popup_open {
        if matches_key(event, &kb.popup_up) {
            return Action::PopupUp;
        }
        if matches_key(event, &kb.popup_down) {
            return Action::PopupDown;
        }
        if matches_key(event, &kb.popup_select) {
            return Action::PopupSelect;
        }
        if matches_key(event, &kb.popup_dismiss) {
            return Action::PopupDismiss;
        }
        // Any other key dismisses popup; fall through to forward key to textarea.
        return Action::PopupDismiss;
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
            translate_session(&ev(KeyCode::Tab), &kb, true, false, false),
            Action::ToggleComposerFocus,
        );
        assert_eq!(
            translate_session(&ev(KeyCode::Tab), &kb, false, false, false),
            Action::ToggleComposerFocus,
        );
    }

    #[test]
    fn session_ctrl_enter_submits() {
        let kb = TuiKeybindings::defaults();
        let ctrl_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(
            translate_session(&ctrl_enter, &kb, true, false, false),
            Action::SubmitComposer,
        );
        assert_eq!(
            translate_session(&ctrl_enter, &kb, false, false, false),
            Action::SubmitComposer,
        );
    }

    #[test]
    fn session_plain_key_goes_to_composer_when_focused() {
        let kb = TuiKeybindings::defaults();
        let key_h = ev(KeyCode::Char('h'));
        assert_eq!(
            translate_session(&key_h, &kb, true, false, false),
            Action::ComposerInput(key_h),
        );
    }

    #[test]
    fn session_plain_key_scrolls_when_transcript_focused() {
        let kb = TuiKeybindings::defaults();
        assert_eq!(
            translate_session(&ev(KeyCode::Up), &kb, false, false, false),
            Action::Up,
        );
    }

    #[test]
    fn session_quit_always_wins() {
        let kb = TuiKeybindings::defaults();
        assert_eq!(
            translate_session(&ev(KeyCode::Char('q')), &kb, true, false, false),
            Action::Quit,
        );
    }

    // --- J.0.4 cancel-turn tests ---

    #[test]
    fn esc_during_streaming_routes_to_cancel_turn() {
        let kb = TuiKeybindings::defaults();
        assert_eq!(
            translate_session(&ev(KeyCode::Esc), &kb, true, true, false),
            Action::CancelTurn,
        );
        assert_eq!(
            translate_session(&ev(KeyCode::Esc), &kb, false, true, false),
            Action::CancelTurn,
        );
    }

    #[test]
    fn esc_when_not_streaming_routes_to_back_via_composer_input() {
        // When not streaming, ESC in composer-focused mode goes to ComposerInput
        // (the textarea handles it).  In transcript-focused mode it hits Back
        // via the standard translate table.
        let kb = TuiKeybindings::defaults();
        let esc = ev(KeyCode::Esc);
        // Composer focused + not streaming → ComposerInput (textarea handles ESC)
        assert_eq!(
            translate_session(&esc, &kb, true, false, false),
            Action::ComposerInput(esc),
        );
        // Transcript focused + not streaming → Back (standard table)
        assert_eq!(
            translate_session(&esc, &kb, false, false, false),
            Action::Back,
        );
    }

    #[test]
    fn ctrl_c_quits_even_during_streaming() {
        let kb = TuiKeybindings::defaults();
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(
            translate_session(&ctrl_c, &kb, true, true, false),
            Action::Quit,
        );
    }

    #[test]
    fn session_r_only_retries_when_disconnected() {
        let kb = TuiKeybindings::defaults();
        let key_r = ev(KeyCode::Char('r'));
        // Banner not shown — `r` reaches the composer instead of retrying.
        assert_eq!(
            translate_session(&key_r, &kb, true, false, false),
            Action::ComposerInput(key_r),
        );
        // Banner shown — `r` triggers an immediate retry.
        assert_eq!(
            translate_session(&key_r, &kb, true, false, true),
            Action::RetryConnection,
        );
    }
}
