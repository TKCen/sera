//! Configurable keybindings for the SERA TUI.
//!
//! Per project rule (CLAUDE.md): keybindings MUST NOT be hardcoded in
//! dispatch code.  All input checks route through [`matches_key`] against
//! a [`TuiKeybindings`] struct that defaults to the values in
//! [`default_tui_keybindings`] but can be overridden by config.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A single bindable key: base [`KeyCode`] plus an exact modifier match.
///
/// We match modifiers with `==` rather than "contains" because it avoids
/// surprising the user when, e.g., Ctrl+Q should *not* trigger the plain
/// `q` quit binding.  A binding that needs "Q or q" adds two entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyBinding {
    /// Unmodified key (no Ctrl/Shift/Alt).
    pub const fn plain(code: KeyCode) -> Self {
        Self { code, modifiers: KeyModifiers::NONE }
    }

    /// Key with explicit modifier set.
    pub const fn with_mods(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    /// Human-readable short form, used by the footer hint row.
    pub fn display(&self) -> String {
        let prefix = if self.modifiers.contains(KeyModifiers::CONTROL) {
            "C-"
        } else if self.modifiers.contains(KeyModifiers::ALT) {
            "M-"
        } else if self.modifiers.contains(KeyModifiers::SHIFT) {
            "S-"
        } else {
            ""
        };
        let key = match self.code {
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Enter => "enter".into(),
            KeyCode::Tab => "tab".into(),
            KeyCode::BackTab => "S-tab".into(),
            KeyCode::Esc => "esc".into(),
            KeyCode::Backspace => "bksp".into(),
            KeyCode::Up => "up".into(),
            KeyCode::Down => "down".into(),
            KeyCode::Left => "left".into(),
            KeyCode::Right => "right".into(),
            KeyCode::Home => "home".into(),
            KeyCode::End => "end".into(),
            KeyCode::PageUp => "pgup".into(),
            KeyCode::PageDown => "pgdn".into(),
            KeyCode::F(n) => format!("F{n}"),
            _ => "?".into(),
        };
        format!("{prefix}{key}")
    }
}

/// Full keybinding surface for the TUI.  Each field is a `Vec<KeyBinding>`
/// so multiple aliases (e.g. `q` and `Ctrl+C`) collapse to one action.
#[derive(Debug, Clone)]
pub struct TuiKeybindings {
    pub quit: Vec<KeyBinding>,
    pub refresh: Vec<KeyBinding>,
    pub next_view: Vec<KeyBinding>,
    pub prev_view: Vec<KeyBinding>,
    pub approve: Vec<KeyBinding>,
    pub reject: Vec<KeyBinding>,
    pub escalate: Vec<KeyBinding>,
    pub up: Vec<KeyBinding>,
    pub down: Vec<KeyBinding>,
    pub select: Vec<KeyBinding>,
    pub back: Vec<KeyBinding>,
    pub end_of_buffer: Vec<KeyBinding>,
    pub page_up: Vec<KeyBinding>,
    pub page_down: Vec<KeyBinding>,
    /// Toggle focus between the composer and transcript inside the Session view.
    pub toggle_composer_focus: Vec<KeyBinding>,
    /// Submit the composer buffer as a pending message (Session view only).
    pub submit_message: Vec<KeyBinding>,
}

impl TuiKeybindings {
    /// Canonical defaults called out in the bead spec.
    pub fn defaults() -> Self {
        Self {
            quit: vec![
                KeyBinding::plain(KeyCode::Char('q')),
                KeyBinding::with_mods(KeyCode::Char('c'), KeyModifiers::CONTROL),
            ],
            refresh: vec![KeyBinding::plain(KeyCode::Char('r'))],
            next_view: vec![KeyBinding::plain(KeyCode::Tab)],
            prev_view: vec![KeyBinding::plain(KeyCode::BackTab)],
            approve: vec![KeyBinding::plain(KeyCode::Char('a'))],
            reject: vec![KeyBinding::plain(KeyCode::Char('x'))],
            escalate: vec![KeyBinding::plain(KeyCode::Char('e'))],
            up: vec![
                KeyBinding::plain(KeyCode::Up),
                KeyBinding::plain(KeyCode::Char('k')),
            ],
            down: vec![
                KeyBinding::plain(KeyCode::Down),
                KeyBinding::plain(KeyCode::Char('j')),
            ],
            select: vec![KeyBinding::plain(KeyCode::Enter)],
            back: vec![
                KeyBinding::plain(KeyCode::Esc),
                KeyBinding::plain(KeyCode::Backspace),
            ],
            end_of_buffer: vec![KeyBinding::plain(KeyCode::End)],
            page_up: vec![KeyBinding::plain(KeyCode::PageUp)],
            page_down: vec![KeyBinding::plain(KeyCode::PageDown)],
            toggle_composer_focus: vec![KeyBinding::plain(KeyCode::Tab)],
            submit_message: vec![
                KeyBinding::with_mods(KeyCode::Enter, KeyModifiers::CONTROL),
                KeyBinding::with_mods(KeyCode::Enter, KeyModifiers::ALT),
            ],
        }
    }
}

impl Default for TuiKeybindings {
    fn default() -> Self {
        Self::defaults()
    }
}

/// Default binding set.  Re-exported as a const-like singleton for callers
/// who prefer a named reference (e.g. config-merge code that wants to
/// start from the default and overlay user overrides).
#[allow(dead_code)]
pub static DEFAULT_TUI_KEYBINDINGS: once_cell::sync::Lazy<TuiKeybindings> =
    once_cell::sync::Lazy::new(TuiKeybindings::defaults);

/// Returns true when `event` matches any binding in `bindings`.
///
/// Key-release events are **not** matched — crossterm emits both Press and
/// Release on many terminals, and our dispatcher ignores Release upstream.
/// Still, this helper is defensive about modifier matching so callers can
/// pass raw events from inline tests.
pub fn matches_key(event: &KeyEvent, bindings: &[KeyBinding]) -> bool {
    bindings
        .iter()
        .any(|b| b.code == event.code && b.modifiers == event.modifiers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evt(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn defaults_cover_all_listed_actions() {
        let kb = TuiKeybindings::defaults();
        assert!(!kb.quit.is_empty());
        assert!(!kb.refresh.is_empty());
        assert!(!kb.next_view.is_empty());
        assert!(!kb.prev_view.is_empty());
        assert!(!kb.approve.is_empty());
        assert!(!kb.reject.is_empty());
        assert!(!kb.escalate.is_empty());
        assert!(!kb.up.is_empty());
        assert!(!kb.down.is_empty());
        assert!(!kb.select.is_empty());
        assert!(!kb.back.is_empty());
        assert!(!kb.toggle_composer_focus.is_empty());
        assert!(!kb.submit_message.is_empty());
    }

    #[test]
    fn matches_plain_q_against_quit() {
        let kb = TuiKeybindings::defaults();
        assert!(matches_key(&evt(KeyCode::Char('q')), &kb.quit));
    }

    #[test]
    fn matches_ctrl_c_against_quit() {
        let kb = TuiKeybindings::defaults();
        let ev = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(matches_key(&ev, &kb.quit));
    }

    #[test]
    fn does_not_match_ctrl_q_against_plain_quit() {
        let kb = TuiKeybindings::defaults();
        let ev = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert!(!matches_key(&ev, &kb.quit));
    }

    #[test]
    fn up_and_k_both_move_up() {
        let kb = TuiKeybindings::defaults();
        assert!(matches_key(&evt(KeyCode::Up), &kb.up));
        assert!(matches_key(&evt(KeyCode::Char('k')), &kb.up));
    }

    #[test]
    fn overridden_binding_replaces_default() {
        let mut kb = TuiKeybindings::defaults();
        kb.refresh = vec![KeyBinding::plain(KeyCode::F(5))];
        assert!(!matches_key(&evt(KeyCode::Char('r')), &kb.refresh));
        assert!(matches_key(&evt(KeyCode::F(5)), &kb.refresh));
    }

    #[test]
    fn display_renders_modifiers() {
        let b = KeyBinding::with_mods(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(b.display(), "C-c");
        let b2 = KeyBinding::plain(KeyCode::Enter);
        assert_eq!(b2.display(), "enter");
        let b3 = KeyBinding::plain(KeyCode::Char('q'));
        assert_eq!(b3.display(), "q");
    }
}
