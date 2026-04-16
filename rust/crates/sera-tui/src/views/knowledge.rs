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
                self.entries.sort_by(|a, b| b.recall_count.cmp(&a.recall_count));
            }
            KnowledgeSortField::BySize => {
                self.entries.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
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
