//! Knowledge explorer view for operator visibility into agent memory.

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};

use super::View;

/// Tier classification for knowledge entries.
#[derive(Debug, Clone, PartialEq)]
pub enum KnowledgeTier {
    ShortTerm,
    LongTerm,
    Shared,
}

impl KnowledgeTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            KnowledgeTier::ShortTerm => "Short",
            KnowledgeTier::LongTerm => "Long",
            KnowledgeTier::Shared => "Shared",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            KnowledgeTier::ShortTerm => Color::Yellow,
            KnowledgeTier::LongTerm => Color::Green,
            KnowledgeTier::Shared => Color::Magenta,
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "long_term" | "long" => KnowledgeTier::LongTerm,
            "shared" => KnowledgeTier::Shared,
            _ => KnowledgeTier::ShortTerm,
        }
    }
}

/// Sort field for knowledge entries.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::enum_variant_names)]
pub enum KnowledgeSortField {
    ByRecency,
    ByScore,
    ByRecallCount,
    BySize,
}

impl KnowledgeSortField {
    pub fn as_str(&self) -> &'static str {
        match self {
            KnowledgeSortField::ByRecency => "Recency",
            KnowledgeSortField::ByScore => "Score",
            KnowledgeSortField::ByRecallCount => "Recalls",
            KnowledgeSortField::BySize => "Size",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            KnowledgeSortField::ByRecency => KnowledgeSortField::ByScore,
            KnowledgeSortField::ByScore => KnowledgeSortField::ByRecallCount,
            KnowledgeSortField::ByRecallCount => KnowledgeSortField::BySize,
            KnowledgeSortField::BySize => KnowledgeSortField::ByRecency,
        }
    }
}

/// A single knowledge entry.
#[derive(Debug, Clone)]
pub struct KnowledgeEntry {
    pub id: String,
    pub title: String,
    pub tier: KnowledgeTier,
    pub tags: Vec<String>,
    pub size_bytes: u64,
    pub created_at: String,
    pub updated_at: String,
    pub recall_count: u64,
    pub score: f64,
}

impl KnowledgeEntry {
    pub fn from_json(v: &serde_json::Value) -> Self {
        let tags = v["tags"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        Self {
            id: v["id"].as_str().unwrap_or("").to_string(),
            title: v["title"].as_str().unwrap_or("(no title)").to_string(),
            tier: KnowledgeTier::from_str(v["tier"].as_str().unwrap_or("")),
            tags,
            size_bytes: v["size_bytes"].as_u64().unwrap_or(0),
            created_at: v["created_at"].as_str().unwrap_or("").to_string(),
            updated_at: v["updated_at"].as_str().unwrap_or("").to_string(),
            recall_count: v["recall_count"].as_u64().unwrap_or(0),
            score: v["score"].as_f64().unwrap_or(0.0),
        }
    }
}

/// View for exploring knowledge/memory entries.
pub struct KnowledgeView {
    entries: Vec<KnowledgeEntry>,
    selected: usize,
    filter_text: String,
    sort_by: KnowledgeSortField,
    detail_mode: bool,
}

impl KnowledgeView {
    /// Create a new knowledge view.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            selected: 0,
            filter_text: String::new(),
            sort_by: KnowledgeSortField::ByRecency,
            detail_mode: false,
        }
    }

    /// Set entries to display.
    pub fn set_entries(&mut self, entries: Vec<KnowledgeEntry>) {
        self.entries = entries;
        self.selected = 0;
        self.apply_sort();
    }

    /// Move selection up.
    pub fn previous(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn next(&mut self) {
        let count = self.filtered_entries().len();
        if self.selected < count.saturating_sub(1) {
            self.selected += 1;
        }
    }

    /// Toggle detail panel.
    pub fn toggle_detail(&mut self) {
        self.detail_mode = !self.detail_mode;
    }

    /// Cycle to the next sort field.
    pub fn cycle_sort(&mut self) {
        self.sort_by = self.sort_by.next();
        self.apply_sort();
    }

    /// Set the filter text.
    pub fn set_filter(&mut self, text: String) {
        self.filter_text = text;
        self.selected = 0;
    }

    /// Get entries matching the current filter.
    pub fn filtered_entries(&self) -> Vec<&KnowledgeEntry> {
        if self.filter_text.is_empty() {
            self.entries.iter().collect()
        } else {
            let lower = self.filter_text.to_lowercase();
            self.entries
                .iter()
                .filter(|e| {
                    e.title.to_lowercase().contains(&lower)
                        || e.tags.iter().any(|t| t.to_lowercase().contains(&lower))
                })
                .collect()
        }
    }

    /// Returns true if a filter is currently active.
    pub fn has_filter(&self) -> bool {
        !self.filter_text.is_empty()
    }

    /// Get the currently selected entry.
    pub fn selected_entry(&self) -> Option<&KnowledgeEntry> {
        self.filtered_entries().get(self.selected).copied()
    }

    fn apply_sort(&mut self) {
        match self.sort_by {
            KnowledgeSortField::ByRecency => {
                self.entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            }
            KnowledgeSortField::ByScore => {
                self.entries
                    .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
            }
            KnowledgeSortField::ByRecallCount => {
                self.entries
                    .sort_by_key(|e| std::cmp::Reverse(e.recall_count));
            }
            KnowledgeSortField::BySize => {
                self.entries
                    .sort_by_key(|e| std::cmp::Reverse(e.size_bytes));
            }
        }
    }

    fn render_detail(&self, frame: &mut Frame, area: Rect, entry: &KnowledgeEntry) {
        let detail_text = format!(
            "ID:       {}\nTitle:    {}\nTier:     {}\nTags:     {}\nScore:    {:.2}\nRecalls:  {}\nSize:     {} bytes\nCreated:  {}\nUpdated:  {}",
            entry.id,
            entry.title,
            entry.tier.as_str(),
            entry.tags.join(", "),
            entry.score,
            entry.recall_count,
            entry.size_bytes,
            entry.created_at,
            entry.updated_at,
        );

        let para = Paragraph::new(detail_text)
            .block(
                Block::default()
                    .title(" Entry Detail ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .wrap(ratatui::widgets::Wrap { trim: false });

        frame.render_widget(para, area);
    }
}

impl Default for KnowledgeView {
    fn default() -> Self {
        Self::new()
    }
}

impl View for KnowledgeView {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let filtered = self.filtered_entries();

        // Split vertically: optional filter bar at top, then content
        let (filter_area, content_area) = if !self.filter_text.is_empty() {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(area);
            (Some(chunks[0]), chunks[1])
        } else {
            (None, area)
        };

        // Render filter bar if active
        if let Some(fa) = filter_area {
            let filter_para = Paragraph::new(format!("Filter: {}", self.filter_text))
                .style(Style::default().fg(Color::Green));
            frame.render_widget(filter_para, fa);
        }

        // Split horizontally for detail mode
        let (list_area, detail_area) = if self.detail_mode {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(content_area);
            (chunks[0], Some(chunks[1]))
        } else {
            (content_area, None)
        };

        if filtered.is_empty() {
            let msg = if self.filter_text.is_empty() {
                "No knowledge entries found. Press 'r' to refresh."
            } else {
                "No entries match filter."
            };
            let text = Paragraph::new(msg).style(Style::default().fg(Color::Yellow));
            frame.render_widget(text, list_area);
        } else {
            // Build table rows
            let rows = filtered.iter().enumerate().map(|(idx, entry)| {
                let style = if idx == self.selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };

                let tier_cell = Cell::from(entry.tier.as_str())
                    .style(Style::default().fg(entry.tier.color()));
                let tags_str = entry.tags.join(", ");
                let score_str = format!("{:.2}", entry.score);
                // Truncate updated_at to date portion
                let updated = entry.updated_at.get(..10).unwrap_or(&entry.updated_at);

                Row::new(vec![
                    Cell::from(entry.title.clone()),
                    tier_cell,
                    Cell::from(tags_str),
                    Cell::from(entry.recall_count.to_string()),
                    Cell::from(score_str),
                    Cell::from(updated.to_string()),
                ])
                .style(style)
            });

            let sort_indicator = format!(" Knowledge [sort: {}] ", self.sort_by.as_str());
            let table = Table::new(
                rows,
                [
                    Constraint::Percentage(30),
                    Constraint::Percentage(10),
                    Constraint::Percentage(25),
                    Constraint::Percentage(10),
                    Constraint::Percentage(10),
                    Constraint::Percentage(15),
                ],
            )
            .header(
                Row::new(vec!["Title", "Tier", "Tags", "Recalls", "Score", "Updated"])
                    .style(Style::default().bold().fg(Color::Cyan)),
            )
            .block(
                Block::default()
                    .title(sort_indicator)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            frame.render_widget(table, list_area);
        }

        // Render detail panel if active
        if let Some(da) = detail_area {
            if let Some(entry) = self.selected_entry() {
                self.render_detail(frame, da, entry);
            } else {
                let placeholder = Paragraph::new("Select an entry to view details.")
                    .block(
                        Block::default()
                            .title(" Entry Detail ")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Yellow)),
                    );
                frame.render_widget(placeholder, da);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── KnowledgeTier ────────────────────────────────────────────────────────

    #[test]
    fn tier_from_str_long_term_variants() {
        assert_eq!(KnowledgeTier::from_str("long_term"), KnowledgeTier::LongTerm);
        assert_eq!(KnowledgeTier::from_str("long"), KnowledgeTier::LongTerm);
    }

    #[test]
    fn tier_from_str_shared() {
        assert_eq!(KnowledgeTier::from_str("shared"), KnowledgeTier::Shared);
    }

    #[test]
    fn tier_from_str_unknown_defaults_to_short_term() {
        assert_eq!(KnowledgeTier::from_str(""), KnowledgeTier::ShortTerm);
        assert_eq!(KnowledgeTier::from_str("short_term"), KnowledgeTier::ShortTerm);
        assert_eq!(KnowledgeTier::from_str("bogus"), KnowledgeTier::ShortTerm);
    }

    #[test]
    fn tier_as_str_round_trips() {
        assert_eq!(KnowledgeTier::ShortTerm.as_str(), "Short");
        assert_eq!(KnowledgeTier::LongTerm.as_str(), "Long");
        assert_eq!(KnowledgeTier::Shared.as_str(), "Shared");
    }

    #[test]
    fn tier_color_is_distinct() {
        // Each tier maps to a different color — guards against accidental merges.
        let colors = [
            KnowledgeTier::ShortTerm.color(),
            KnowledgeTier::LongTerm.color(),
            KnowledgeTier::Shared.color(),
        ];
        assert!(colors[0] != colors[1]);
        assert!(colors[1] != colors[2]);
        assert!(colors[0] != colors[2]);
    }

    // ── KnowledgeSortField ───────────────────────────────────────────────────

    #[test]
    fn sort_field_cycle_wraps_around() {
        let start = KnowledgeSortField::ByRecency;
        let after_one = start.next();
        assert_eq!(after_one, KnowledgeSortField::ByScore);
        let after_two = after_one.next();
        assert_eq!(after_two, KnowledgeSortField::ByRecallCount);
        let after_three = after_two.next();
        assert_eq!(after_three, KnowledgeSortField::BySize);
        let wrapped = after_three.next();
        assert_eq!(wrapped, KnowledgeSortField::ByRecency);
    }

    // ── KnowledgeEntry::from_json ────────────────────────────────────────────

    #[test]
    fn from_json_parses_all_fields() {
        let v = json!({
            "id": "k1",
            "title": "Arch notes",
            "tier": "long_term",
            "tags": ["rust", "design"],
            "size_bytes": 1024,
            "created_at": "2026-04-10T10:00:00Z",
            "updated_at": "2026-04-15T14:30:00Z",
            "recall_count": 7,
            "score": 0.95,
        });
        let entry = KnowledgeEntry::from_json(&v);
        assert_eq!(entry.id, "k1");
        assert_eq!(entry.title, "Arch notes");
        assert_eq!(entry.tier, KnowledgeTier::LongTerm);
        assert_eq!(entry.tags, vec!["rust", "design"]);
        assert_eq!(entry.size_bytes, 1024);
        assert_eq!(entry.recall_count, 7);
        assert!((entry.score - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn from_json_missing_fields_use_defaults() {
        let entry = KnowledgeEntry::from_json(&json!({}));
        assert_eq!(entry.id, "");
        assert_eq!(entry.title, "(no title)");
        assert_eq!(entry.tier, KnowledgeTier::ShortTerm);
        assert!(entry.tags.is_empty());
        assert_eq!(entry.size_bytes, 0);
        assert_eq!(entry.recall_count, 0);
        assert_eq!(entry.score, 0.0);
    }

    // ── KnowledgeView navigation & state ────────────────────────────────────

    fn make_entries(n: usize) -> Vec<KnowledgeEntry> {
        (0..n)
            .map(|i| KnowledgeEntry::from_json(&json!({
                "id": format!("k{i}"),
                "title": format!("Entry {i}"),
                "tier": "long_term",
                "tags": [],
                "size_bytes": (i as u64) * 100,
                "created_at": "2026-04-10T00:00:00Z",
                "updated_at": format!("2026-04-{:02}T00:00:00Z", 10 + i),
                "recall_count": i as u64,
                "score": i as f64 * 0.1,
            })))
            .collect()
    }

    #[test]
    fn navigation_clamps_at_boundaries() {
        let mut view = KnowledgeView::new();
        view.set_entries(make_entries(3));

        // After set_entries, ByRecency sort puts the newest updated_at first.
        // make_entries produces updated_at "2026-04-12", "2026-04-11", "2026-04-10"
        // so sorted order is k2, k1, k0.
        let first_id = view.filtered_entries()[0].id.clone();
        let last_id  = view.filtered_entries()[2].id.clone();

        // previous at zero does nothing
        view.previous();
        assert_eq!(view.selected_entry().map(|e| e.id.clone()), Some(first_id.clone()));

        // move to last
        view.next();
        view.next();
        assert_eq!(view.selected_entry().map(|e| e.id.clone()), Some(last_id.clone()));

        // next at last does nothing
        view.next();
        assert_eq!(view.selected_entry().map(|e| e.id.clone()), Some(last_id));
    }

    #[test]
    fn filter_text_narrows_results() {
        let mut view = KnowledgeView::new();
        view.set_entries(make_entries(3));

        assert!(!view.has_filter());
        assert_eq!(view.filtered_entries().len(), 3);

        view.set_filter("Entry 1".to_string());
        assert!(view.has_filter());
        assert_eq!(view.filtered_entries().len(), 1);
        assert_eq!(view.filtered_entries()[0].id, "k1");

        view.set_filter(String::new());
        assert!(!view.has_filter());
        assert_eq!(view.filtered_entries().len(), 3);
    }

    #[test]
    fn cycle_sort_changes_field_and_reorders() {
        let mut view = KnowledgeView::new();
        // entries with varying recall_count — set_entries sorts by ByRecency by default
        let mut entries = make_entries(3);
        // manually set differing scores so sort order is predictable
        entries[0].score = 0.9;
        entries[1].score = 0.5;
        entries[2].score = 0.1;
        view.set_entries(entries);

        // initial sort: ByRecency — latest updated_at first → k2 > k1 > k0
        assert_eq!(view.filtered_entries()[0].id, "k2");

        view.cycle_sort(); // → ByScore
        // ByScore descending: k0(0.9) > k1(0.5) > k2(0.1)
        assert_eq!(view.filtered_entries()[0].id, "k0");
    }

    #[test]
    fn toggle_detail_flips_state() {
        let mut view = KnowledgeView::new();
        view.set_entries(make_entries(1));

        assert!(!view.detail_mode);
        view.toggle_detail();
        assert!(view.detail_mode);
        view.toggle_detail();
        assert!(!view.detail_mode);
    }

    #[test]
    fn selected_entry_returns_none_on_empty() {
        let view = KnowledgeView::new();
        assert!(view.selected_entry().is_none());
    }
}

