//! Knowledge activity log — chronological record of knowledge operations.
//!
//! Provides an append-only, rolling-window log of knowledge operations
//! (store/update/delete/synthesize/lint) scoped to an agent or circle.
//! Gives agents temporal awareness of knowledge growth.
//!
//! # Design
//!
//! - **Append-only with rolling window**: oldest entries are evicted when
//!   `max_entries` is exceeded, keeping memory bounded.
//! - **Queryable**: filter by op type, scope, time range, or page ID.
//! - **Serializable**: round-trips through JSON or YAML for persistence
//!   to `_log.yaml` or similar backing files.
//!
//! Inspired by Karpathy's LLM `wiki log.md` concept.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Op type
// ---------------------------------------------------------------------------

/// The kind of knowledge operation that was performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeOp {
    /// A new knowledge page was written.
    Store,
    /// An existing knowledge page was updated.
    Update,
    /// A knowledge page was deleted.
    Delete,
    /// One or more pages were synthesized into a new summary page.
    Synthesize,
    /// A lint/schema-validation pass was run against knowledge pages.
    Lint,
}

impl fmt::Display for KnowledgeOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Store => "store",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Synthesize => "synthesize",
            Self::Lint => "lint",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

/// A single entry in the knowledge activity log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeActivityEntry {
    /// When the operation occurred (UTC).
    pub timestamp: DateTime<Utc>,
    /// The type of knowledge operation.
    pub op: KnowledgeOp,
    /// Agent or circle scope that performed the operation.
    pub scope: String,
    /// The page that was affected, if applicable.
    pub page_id: Option<String>,
    /// Human-readable summary of what was done.
    pub summary: String,
    /// Optional structured metadata (e.g. diff stats, lint counts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl KnowledgeActivityEntry {
    /// Create a new entry with the current UTC timestamp.
    pub fn new(
        op: KnowledgeOp,
        scope: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            op,
            scope: scope.into(),
            page_id: None,
            summary: summary.into(),
            metadata: None,
        }
    }

    /// Attach a page ID to this entry.
    pub fn with_page_id(mut self, page_id: impl Into<String>) -> Self {
        self.page_id = Some(page_id.into());
        self
    }

    /// Attach structured metadata to this entry.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl fmt::Display for KnowledgeActivityEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} | scope={} | {}",
            self.timestamp.format("%Y-%m-%dT%H:%M:%SZ"),
            self.op,
            self.scope,
            self.summary
        )?;
        if let Some(page_id) = &self.page_id {
            write!(f, " (page: {page_id})")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Query filter
// ---------------------------------------------------------------------------

/// Filter criteria for [`KnowledgeActivityLog::query`].
///
/// All fields are optional; an entry matches only if it satisfies every
/// non-`None` criterion.
#[derive(Debug, Default, Clone)]
pub struct ActivityLogFilter {
    /// If set, only entries with this op type are returned.
    pub op: Option<KnowledgeOp>,
    /// If set, only entries whose `scope` exactly matches are returned.
    pub scope: Option<String>,
    /// If set, only entries whose `page_id` exactly matches are returned.
    pub page_id: Option<String>,
    /// If set, only entries at or after this timestamp are returned.
    pub since: Option<DateTime<Utc>>,
    /// If set, only entries before or at this timestamp are returned.
    pub until: Option<DateTime<Utc>>,
}

impl ActivityLogFilter {
    /// Create a new empty filter (matches everything).
    pub fn new() -> Self {
        Self::default()
    }

    /// Restrict to a specific operation type.
    pub fn with_op(mut self, op: KnowledgeOp) -> Self {
        self.op = Some(op);
        self
    }

    /// Restrict to a specific scope.
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    /// Restrict to a specific page ID.
    pub fn with_page_id(mut self, page_id: impl Into<String>) -> Self {
        self.page_id = Some(page_id.into());
        self
    }

    /// Restrict to entries at or after `since`.
    pub fn since(mut self, since: DateTime<Utc>) -> Self {
        self.since = Some(since);
        self
    }

    /// Restrict to entries before or at `until`.
    pub fn until(mut self, until: DateTime<Utc>) -> Self {
        self.until = Some(until);
        self
    }

    fn matches(&self, entry: &KnowledgeActivityEntry) -> bool {
        if let Some(op) = self.op
            && entry.op != op
        {
            return false;
        }
        if let Some(ref scope) = self.scope
            && &entry.scope != scope
        {
            return false;
        }
        if let Some(ref page_id) = self.page_id {
            match &entry.page_id {
                Some(eid) if eid == page_id => {}
                _ => return false,
            }
        }
        if let Some(since) = self.since
            && entry.timestamp < since
        {
            return false;
        }
        if let Some(until) = self.until
            && entry.timestamp > until
        {
            return false;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------

/// Default rolling-window size (1 000 entries).
pub const DEFAULT_MAX_ENTRIES: usize = 1_000;

/// Append-only knowledge activity log with a configurable rolling window.
///
/// When the number of entries exceeds `max_entries`, the oldest entry is
/// evicted to keep the log bounded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeActivityLog {
    entries: Vec<KnowledgeActivityEntry>,
    max_entries: usize,
}

impl Default for KnowledgeActivityLog {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ENTRIES)
    }
}

impl KnowledgeActivityLog {
    /// Create a new empty log with the given rolling-window size.
    ///
    /// # Panics
    ///
    /// Panics if `max_entries` is 0.
    pub fn new(max_entries: usize) -> Self {
        assert!(max_entries > 0, "max_entries must be > 0");
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }

    /// The configured rolling-window capacity.
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// The current number of entries in the log.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the log contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Append a new entry. If the log is at capacity the oldest entry is evicted.
    pub fn append(&mut self, entry: KnowledgeActivityEntry) {
        if self.entries.len() >= self.max_entries {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }

    /// Return the last `n` entries in chronological order (oldest first).
    ///
    /// If the log contains fewer than `n` entries, all entries are returned.
    pub fn recent(&self, n: usize) -> &[KnowledgeActivityEntry] {
        let start = self.entries.len().saturating_sub(n);
        &self.entries[start..]
    }

    /// Return all entries that match `filter`, in chronological order.
    pub fn query(&self, filter: &ActivityLogFilter) -> Vec<&KnowledgeActivityEntry> {
        self.entries.iter().filter(|e| filter.matches(e)).collect()
    }

    /// Iterate over all entries in chronological order (oldest first).
    pub fn iter(&self) -> impl Iterator<Item = &KnowledgeActivityEntry> {
        self.entries.iter()
    }
}

impl fmt::Display for KnowledgeActivityLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for entry in &self.entries {
            writeln!(f, "{entry}")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn make_entry(op: KnowledgeOp, scope: &str, summary: &str) -> KnowledgeActivityEntry {
        KnowledgeActivityEntry::new(op, scope, summary)
    }

    fn make_entry_at(
        op: KnowledgeOp,
        scope: &str,
        summary: &str,
        ts: DateTime<Utc>,
    ) -> KnowledgeActivityEntry {
        KnowledgeActivityEntry {
            timestamp: ts,
            op,
            scope: scope.to_string(),
            page_id: None,
            summary: summary.to_string(),
            metadata: None,
        }
    }

    // --- KnowledgeOp ---

    #[test]
    fn op_display() {
        assert_eq!(KnowledgeOp::Store.to_string(), "store");
        assert_eq!(KnowledgeOp::Update.to_string(), "update");
        assert_eq!(KnowledgeOp::Delete.to_string(), "delete");
        assert_eq!(KnowledgeOp::Synthesize.to_string(), "synthesize");
        assert_eq!(KnowledgeOp::Lint.to_string(), "lint");
    }

    #[test]
    fn op_serde_roundtrip() {
        for op in [
            KnowledgeOp::Store,
            KnowledgeOp::Update,
            KnowledgeOp::Delete,
            KnowledgeOp::Synthesize,
            KnowledgeOp::Lint,
        ] {
            let json = serde_json::to_string(&op).unwrap();
            let parsed: KnowledgeOp = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, op);
        }
    }

    #[test]
    fn op_serde_snake_case() {
        let json = serde_json::to_string(&KnowledgeOp::Store).unwrap();
        assert_eq!(json, "\"store\"");
        let json = serde_json::to_string(&KnowledgeOp::Synthesize).unwrap();
        assert_eq!(json, "\"synthesize\"");
    }

    // --- KnowledgeActivityEntry ---

    #[test]
    fn entry_builder_defaults() {
        let e = make_entry(KnowledgeOp::Store, "agent-1", "stored page");
        assert_eq!(e.op, KnowledgeOp::Store);
        assert_eq!(e.scope, "agent-1");
        assert_eq!(e.summary, "stored page");
        assert!(e.page_id.is_none());
        assert!(e.metadata.is_none());
    }

    #[test]
    fn entry_with_page_id() {
        let e = make_entry(KnowledgeOp::Update, "agent-1", "updated")
            .with_page_id("page-42");
        assert_eq!(e.page_id.as_deref(), Some("page-42"));
    }

    #[test]
    fn entry_with_metadata() {
        let meta = serde_json::json!({"lines_changed": 10});
        let e = make_entry(KnowledgeOp::Update, "agent-1", "updated")
            .with_metadata(meta.clone());
        assert_eq!(e.metadata, Some(meta));
    }

    #[test]
    fn entry_display_no_page() {
        let e = KnowledgeActivityEntry {
            timestamp: chrono::DateTime::parse_from_rfc3339("2024-01-15T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            op: KnowledgeOp::Store,
            scope: "circle-a".to_string(),
            page_id: None,
            summary: "stored decision".to_string(),
            metadata: None,
        };
        let s = e.to_string();
        assert!(s.contains("store"));
        assert!(s.contains("circle-a"));
        assert!(s.contains("stored decision"));
        assert!(!s.contains("page:"));
    }

    #[test]
    fn entry_display_with_page() {
        let e = KnowledgeActivityEntry {
            timestamp: chrono::DateTime::parse_from_rfc3339("2024-01-15T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            op: KnowledgeOp::Delete,
            scope: "agent-x".to_string(),
            page_id: Some("old-page".to_string()),
            summary: "pruned stale page".to_string(),
            metadata: None,
        };
        let s = e.to_string();
        assert!(s.contains("old-page"));
    }

    #[test]
    fn entry_serde_roundtrip_json() {
        let e = make_entry(KnowledgeOp::Lint, "circle-z", "lint pass complete")
            .with_page_id("page-99")
            .with_metadata(serde_json::json!({"violations": 3}));
        let json = serde_json::to_string(&e).unwrap();
        let parsed: KnowledgeActivityEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.op, e.op);
        assert_eq!(parsed.scope, e.scope);
        assert_eq!(parsed.page_id, e.page_id);
        assert_eq!(parsed.summary, e.summary);
        assert_eq!(parsed.metadata, e.metadata);
    }

    #[test]
    fn entry_metadata_skipped_when_none() {
        let e = make_entry(KnowledgeOp::Store, "s", "sum");
        let json = serde_json::to_string(&e).unwrap();
        assert!(!json.contains("metadata"));
    }

    // --- KnowledgeActivityLog ---

    #[test]
    fn log_starts_empty() {
        let log = KnowledgeActivityLog::new(10);
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn log_append_and_len() {
        let mut log = KnowledgeActivityLog::new(10);
        log.append(make_entry(KnowledgeOp::Store, "s", "first"));
        log.append(make_entry(KnowledgeOp::Update, "s", "second"));
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn log_rolling_window_evicts_oldest() {
        let mut log = KnowledgeActivityLog::new(3);
        log.append(make_entry(KnowledgeOp::Store, "s", "first"));
        log.append(make_entry(KnowledgeOp::Store, "s", "second"));
        log.append(make_entry(KnowledgeOp::Store, "s", "third"));
        // At capacity — next append evicts "first"
        log.append(make_entry(KnowledgeOp::Store, "s", "fourth"));
        assert_eq!(log.len(), 3);
        let all: Vec<_> = log.iter().collect();
        assert_eq!(all[0].summary, "second");
        assert_eq!(all[2].summary, "fourth");
    }

    #[test]
    fn log_max_entries_accessor() {
        let log = KnowledgeActivityLog::new(42);
        assert_eq!(log.max_entries(), 42);
    }

    #[test]
    #[should_panic(expected = "max_entries must be > 0")]
    fn log_panics_on_zero_max() {
        let _ = KnowledgeActivityLog::new(0);
    }

    // --- recent ---

    #[test]
    fn recent_returns_last_n() {
        let mut log = KnowledgeActivityLog::new(10);
        for i in 0..5 {
            log.append(make_entry(KnowledgeOp::Store, "s", &format!("entry-{i}")));
        }
        let r = log.recent(3);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].summary, "entry-2");
        assert_eq!(r[2].summary, "entry-4");
    }

    #[test]
    fn recent_clamps_to_available() {
        let mut log = KnowledgeActivityLog::new(10);
        log.append(make_entry(KnowledgeOp::Store, "s", "only"));
        let r = log.recent(100);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn recent_empty_log() {
        let log = KnowledgeActivityLog::new(10);
        assert!(log.recent(5).is_empty());
    }

    // --- query ---

    #[test]
    fn query_by_op() {
        let mut log = KnowledgeActivityLog::new(10);
        log.append(make_entry(KnowledgeOp::Store, "s", "a"));
        log.append(make_entry(KnowledgeOp::Delete, "s", "b"));
        log.append(make_entry(KnowledgeOp::Store, "s", "c"));

        let results = log.query(&ActivityLogFilter::new().with_op(KnowledgeOp::Store));
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.op == KnowledgeOp::Store));
    }

    #[test]
    fn query_by_scope() {
        let mut log = KnowledgeActivityLog::new(10);
        log.append(make_entry(KnowledgeOp::Store, "agent-a", "x"));
        log.append(make_entry(KnowledgeOp::Store, "agent-b", "y"));

        let results = log.query(&ActivityLogFilter::new().with_scope("agent-a"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].scope, "agent-a");
    }

    #[test]
    fn query_by_page_id() {
        let mut log = KnowledgeActivityLog::new(10);
        log.append(
            make_entry(KnowledgeOp::Update, "s", "a").with_page_id("page-1"),
        );
        log.append(
            make_entry(KnowledgeOp::Update, "s", "b").with_page_id("page-2"),
        );
        log.append(make_entry(KnowledgeOp::Update, "s", "c")); // no page_id

        let results = log.query(&ActivityLogFilter::new().with_page_id("page-1"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].page_id.as_deref(), Some("page-1"));
    }

    #[test]
    fn query_by_time_range() {
        let base = chrono::DateTime::parse_from_rfc3339("2024-06-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut log = KnowledgeActivityLog::new(10);
        log.append(make_entry_at(KnowledgeOp::Store, "s", "early", base));
        log.append(make_entry_at(
            KnowledgeOp::Store,
            "s",
            "mid",
            base + Duration::hours(1),
        ));
        log.append(make_entry_at(
            KnowledgeOp::Store,
            "s",
            "late",
            base + Duration::hours(2),
        ));

        let filter = ActivityLogFilter::new()
            .since(base + Duration::minutes(30))
            .until(base + Duration::minutes(90));
        let results = log.query(&filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].summary, "mid");
    }

    #[test]
    fn query_combined_filters() {
        let mut log = KnowledgeActivityLog::new(20);
        log.append(make_entry(KnowledgeOp::Store, "agent-a", "x").with_page_id("p1"));
        log.append(make_entry(KnowledgeOp::Store, "agent-b", "y").with_page_id("p1"));
        log.append(make_entry(KnowledgeOp::Delete, "agent-a", "z").with_page_id("p1"));

        let filter = ActivityLogFilter::new()
            .with_op(KnowledgeOp::Store)
            .with_scope("agent-a")
            .with_page_id("p1");
        let results = log.query(&filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].summary, "x");
    }

    #[test]
    fn query_no_matches_returns_empty() {
        let mut log = KnowledgeActivityLog::new(10);
        log.append(make_entry(KnowledgeOp::Store, "s", "x"));
        let results = log.query(&ActivityLogFilter::new().with_op(KnowledgeOp::Synthesize));
        assert!(results.is_empty());
    }

    #[test]
    fn query_empty_filter_returns_all() {
        let mut log = KnowledgeActivityLog::new(10);
        log.append(make_entry(KnowledgeOp::Store, "s", "a"));
        log.append(make_entry(KnowledgeOp::Delete, "s", "b"));
        let results = log.query(&ActivityLogFilter::new());
        assert_eq!(results.len(), 2);
    }

    // --- serde (log level) ---

    #[test]
    fn log_serde_roundtrip_json() {
        let mut log = KnowledgeActivityLog::new(5);
        log.append(
            make_entry(KnowledgeOp::Store, "agent-1", "hello").with_page_id("p1"),
        );
        log.append(make_entry(KnowledgeOp::Lint, "agent-1", "lint ok"));
        let json = serde_json::to_string(&log).unwrap();
        let parsed: KnowledgeActivityLog = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed.max_entries(), 5);
        assert_eq!(parsed.iter().next().unwrap().summary, "hello");
    }

    #[test]
    fn log_display_formats_all_entries() {
        let mut log = KnowledgeActivityLog::new(10);
        log.append(make_entry(KnowledgeOp::Store, "s", "first"));
        log.append(make_entry(KnowledgeOp::Update, "s", "second"));
        let display = log.to_string();
        assert!(display.contains("first"));
        assert!(display.contains("second"));
    }

    // --- default ---

    #[test]
    fn log_default_uses_default_max() {
        let log = KnowledgeActivityLog::default();
        assert_eq!(log.max_entries(), DEFAULT_MAX_ENTRIES);
        assert!(log.is_empty());
    }
}
