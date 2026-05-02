//! Autocomplete popup state for the TUI composer (J.0.5, sera-ywdr).
//!
//! Two trigger modes:
//!
//! * **Slash** (`/` at the start of line): shows registered slash commands.
//! * **At-file** (`@` mid-line, no trailing whitespace): shows files in cwd,
//!   filtered by what the user has typed after `@`.  The `ignore` crate is
//!   used so `.gitignore` rules are respected automatically.

use std::path::Path;

use ignore::WalkBuilder;

/// Maximum file matches returned by the `@` completer to bound latency.
const MAX_FILE_MATCHES: usize = 100;

/// The two popup modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupMode {
    /// Triggered by `/` at start of line.
    Slash,
    /// Triggered by `@` mid-line (no whitespace between `@` and cursor).
    AtFile,
}

/// A single item in the popup list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupItem {
    /// Text inserted into the composer on selection.
    pub insert: String,
    /// Short description shown next to the item.
    pub description: String,
}

/// Active popup state.  `None` on `App` means the popup is closed.
#[derive(Debug, Clone)]
pub struct AutocompletePopup {
    pub mode: PopupMode,
    /// Filtered candidate list.
    pub items: Vec<PopupItem>,
    /// Currently highlighted row (0-based).
    pub selected: usize,
    /// Text typed after the trigger character.
    pub filter: String,
}

impl AutocompletePopup {
    /// Build a slash-command popup filtered by `typed_prefix`.
    pub fn for_slash(typed_prefix: &str) -> Self {
        let all = slash_command_items();
        let items = filter_items(&all, typed_prefix);
        Self { mode: PopupMode::Slash, items, selected: 0, filter: typed_prefix.to_owned() }
    }

    /// Build an at-file popup filtered by `typed_prefix`, walking `cwd`.
    pub fn for_at_file(typed_prefix: &str, cwd: &Path) -> Self {
        let candidates = collect_files(cwd, typed_prefix);
        let items = candidates
            .into_iter()
            .map(|p| PopupItem { insert: p, description: String::new() })
            .collect();
        Self { mode: PopupMode::AtFile, items, selected: 0, filter: typed_prefix.to_owned() }
    }

    /// Move selection up; wraps at the top.
    pub fn move_up(&mut self) {
        if self.items.is_empty() { return; }
        if self.selected == 0 {
            self.selected = self.items.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    /// Move selection down; wraps at the bottom.
    pub fn move_down(&mut self) {
        if self.items.is_empty() { return; }
        self.selected = (self.selected + 1) % self.items.len();
    }

    /// Return the currently highlighted item, if any.
    pub fn selected_item(&self) -> Option<&PopupItem> {
        self.items.get(self.selected)
    }

    /// True when the candidate list is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

// ── slash catalogue ───────────────────────────────────────────────────────────

/// All slash commands surfaced in the autocomplete popup.
/// Keep in sync with `app::slash::parse` and the spec command table.
pub fn slash_command_items() -> Vec<PopupItem> {
    vec![
        PopupItem { insert: "/new".into(),   description: "clear transcript, start fresh turn".into() },
        PopupItem { insert: "/clear".into(), description: "alias for /new".into() },
        PopupItem { insert: "/agent".into(), description: "switch active agent: /agent <name>".into() },
        PopupItem { insert: "/help".into(),  description: "toggle help modal".into() },
        PopupItem { insert: "/quit".into(),  description: "exit the TUI".into() },
    ]
}

/// Keep only items whose command token starts with `prefix` (case-insensitive).
fn filter_items(items: &[PopupItem], prefix: &str) -> Vec<PopupItem> {
    if prefix.is_empty() { return items.to_vec(); }
    let lower = prefix.to_lowercase();
    items
        .iter()
        .filter(|i| i.insert.trim_start_matches('/').to_lowercase().starts_with(&lower))
        .cloned()
        .collect()
}

// ── file discovery ────────────────────────────────────────────────────────────

/// Walk `root` (respecting `.gitignore`) and return up to [`MAX_FILE_MATCHES`]
/// relative paths that contain `prefix`.  Directories are excluded.
fn collect_files(root: &Path, prefix: &str) -> Vec<String> {
    let lower = prefix.to_lowercase();
    let mut results = Vec::new();

    for entry in WalkBuilder::new(root).max_depth(Some(6)).build().flatten() {
        if results.len() >= MAX_FILE_MATCHES { break; }
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if lower.is_empty() || rel.to_lowercase().contains(&lower) {
            results.push(rel.to_owned());
        }
    }
    results.sort();
    results
}

// ── trigger detection ─────────────────────────────────────────────────────────

/// Analyse the current composer line and return the active trigger, or `None`.
///
/// * Slash: first non-whitespace char is `/`.  Filter = text after `/`.
/// * At-file: last `@` with no whitespace between it and the end of line.
///   Filter = text after the `@`.
pub fn detect_trigger(line: &str) -> Option<(PopupMode, String)> {
    if line.trim_start().starts_with('/') {
        let after = line.trim_start().trim_start_matches('/');
        return Some((PopupMode::Slash, after.to_owned()));
    }
    if let Some(at_pos) = line.rfind('@') {
        let after = &line[at_pos + 1..];
        if !after.contains(char::is_whitespace) {
            return Some((PopupMode::AtFile, after.to_owned()));
        }
    }
    None
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_triggers_slash_mode() {
        let (mode, filter) = detect_trigger("/new").unwrap();
        assert_eq!(mode, PopupMode::Slash);
        assert_eq!(filter, "new");
    }

    #[test]
    fn slash_alone_gives_empty_filter() {
        let (mode, filter) = detect_trigger("/").unwrap();
        assert_eq!(mode, PopupMode::Slash);
        assert_eq!(filter, "");
    }

    #[test]
    fn at_mid_line_triggers_at_file_mode() {
        let (mode, filter) = detect_trigger("see @src/lib").unwrap();
        assert_eq!(mode, PopupMode::AtFile);
        assert_eq!(filter, "src/lib");
    }

    #[test]
    fn at_with_trailing_space_does_not_trigger() {
        assert!(detect_trigger("@path ").is_none());
    }

    #[test]
    fn plain_text_returns_none() {
        assert!(detect_trigger("hello world").is_none());
    }

    #[test]
    fn empty_line_returns_none() {
        assert!(detect_trigger("").is_none());
    }

    #[test]
    fn filter_empty_prefix_returns_all() {
        let items = slash_command_items();
        assert_eq!(filter_items(&items, "").len(), items.len());
    }

    #[test]
    fn filter_ne_matches_new() {
        let items = slash_command_items();
        let f = filter_items(&items, "ne");
        assert!(f.iter().any(|i| i.insert == "/new"));
        assert!(!f.iter().any(|i| i.insert == "/quit"));
    }

    #[test]
    fn filter_case_insensitive() {
        let items = slash_command_items();
        let f = filter_items(&items, "NE");
        assert!(f.iter().any(|i| i.insert == "/new"));
    }

    #[test]
    fn for_slash_empty_prefix_lists_all() {
        let popup = AutocompletePopup::for_slash("");
        assert_eq!(popup.items.len(), slash_command_items().len());
        assert_eq!(popup.selected, 0);
    }

    #[test]
    fn for_slash_filtered_prefix_limits_items() {
        let popup = AutocompletePopup::for_slash("qui");
        assert_eq!(popup.items.len(), 1);
        assert_eq!(popup.items[0].insert, "/quit");
    }

    #[test]
    fn move_down_wraps_at_end() {
        let mut popup = AutocompletePopup::for_slash("");
        let n = popup.items.len();
        popup.selected = n - 1;
        popup.move_down();
        assert_eq!(popup.selected, 0);
    }

    #[test]
    fn move_up_wraps_at_start() {
        let mut popup = AutocompletePopup::for_slash("");
        let n = popup.items.len();
        popup.selected = 0;
        popup.move_up();
        assert_eq!(popup.selected, n - 1);
    }

    #[test]
    fn selected_item_returns_highlighted() {
        let popup = AutocompletePopup::for_slash("he");
        let item = popup.selected_item().unwrap();
        assert_eq!(item.insert, "/help");
    }

    #[test]
    fn empty_popup_navigation_is_noop() {
        let mut popup = AutocompletePopup::for_slash("zzz");
        assert!(popup.is_empty());
        popup.move_up();
        popup.move_down();
        assert_eq!(popup.selected, 0);
    }

    #[test]
    fn for_at_file_against_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("foo.txt"), b"").unwrap();
        std::fs::write(dir.path().join("bar.rs"), b"").unwrap();
        let popup = AutocompletePopup::for_at_file("foo", dir.path());
        assert_eq!(popup.items.len(), 1);
        assert!(popup.items[0].insert.contains("foo.txt"));
    }

    #[test]
    fn collect_files_respects_prefix() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("alpha.txt"), b"").unwrap();
        std::fs::write(dir.path().join("beta.txt"), b"").unwrap();
        let files = collect_files(dir.path(), "alp");
        assert_eq!(files.len(), 1);
        assert!(files[0].contains("alpha"));
    }

    #[test]
    fn collect_files_caps_at_max() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..=MAX_FILE_MATCHES {
            std::fs::write(dir.path().join(format!("file{i}.txt")), b"").unwrap();
        }
        let files = collect_files(dir.path(), "");
        assert!(files.len() <= MAX_FILE_MATCHES);
    }
}
