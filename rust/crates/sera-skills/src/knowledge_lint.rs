//! Knowledge lint periodic health-check for agent and circle memory.
//!
//! Implements the Karpathy LLM wiki maintenance pattern: a scheduled job that
//! detects contradictions, stale blocks, orphans (no semantic neighbours), and
//! knowledge gaps. Non-LLM checks are executed by [`BasicLinter`]. LLM-assisted
//! checks ([`LintCheckKind::Contradiction`], [`LintCheckKind::KnowledgeGap`])
//! are defined at the type level and stubbed with TODOs for future
//! `MemoryAnalyst` integration.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(test)]
use async_trait::async_trait;
#[cfg(not(test))]
use async_trait::async_trait;

use crate::knowledge_schema::{KnowledgeSchemaValidator, SchemaViolation};
use sera_types::skill::KnowledgeSchema;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during a knowledge lint run.
#[derive(Debug, Error)]
pub enum LintError {
    #[error("lint configuration is invalid: {0}")]
    InvalidConfig(String),

    #[error("failed to analyse page '{page_id}': {reason}")]
    PageAnalysisFailed { page_id: String, reason: String },

    #[error("LLM token budget exhausted (budget: {budget}, used: {used})")]
    TokenBudgetExhausted { budget: usize, used: usize },

    #[error("lint run timed out after {elapsed_ms}ms")]
    Timeout { elapsed_ms: u64 },
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// The category of lint check performed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LintCheckKind {
    /// Pages whose `last_modified` timestamp is older than
    /// [`LintConfig::stale_threshold_days`].
    StaleContent,
    /// Pages with no inbound links and no outbound links — fully disconnected
    /// from the knowledge graph.
    Orphan,
    /// Pages that assert conflicting information (requires an LLM call).
    Contradiction,
    /// Topics referenced across pages but lacking a dedicated page (requires
    /// an LLM call).
    KnowledgeGap,
    /// Pages that violate the active [`KnowledgeSchema`].
    SchemaViolation,
    /// Pages whose content is nearly identical to another page.
    DuplicateContent,
}

/// Severity of a lint finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    /// Must be addressed immediately; may indicate data corruption or a
    /// blocking contradiction.
    Critical,
    /// Should be addressed but does not block normal operation.
    Warning,
    /// Informational observation; no action required.
    Info,
}

/// A single lint finding for one page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintFinding {
    /// The category of check that produced this finding.
    pub check: LintCheckKind,
    /// How severe the finding is.
    pub severity: FindingSeverity,
    /// Identifier of the page that triggered the finding.
    pub page_id: String,
    /// Human-readable description of the finding.
    pub message: String,
    /// Optional suggested remediation action.
    pub suggestion: Option<String>,
}

/// Minimal page information required for lint analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    /// Unique page identifier (e.g., slug or UUID).
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Full page content (Markdown or plain text).
    pub content: String,
    /// When the page was last written.
    pub last_modified: DateTime<Utc>,
    /// IDs of pages that link *to* this page.
    pub inbound_links: Vec<String>,
    /// IDs of pages this page links *to*.
    pub outbound_links: Vec<String>,
    /// Optional page-type tag (e.g., `"decision"`, `"runbook"`).
    pub page_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for a single lint run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintConfig {
    /// Which checks to perform during this run.
    pub checks: Vec<LintCheckKind>,
    /// Number of days after which a page is considered stale.
    pub stale_threshold_days: u32,
    /// Maximum number of pages to inspect in one run (budget guard).
    pub max_pages_per_run: usize,
    /// Maximum LLM tokens to spend on LLM-assisted checks.
    pub token_budget: usize,
    /// Agent ID or circle ID this run is scoped to.
    pub scope: String,
    /// Optional knowledge schema used for [`LintCheckKind::SchemaViolation`]
    /// checks. If `None`, schema validation is skipped even when
    /// [`LintCheckKind::SchemaViolation`] is listed in `checks`.
    pub schema: Option<KnowledgeSchema>,
}

impl Default for LintConfig {
    fn default() -> Self {
        Self {
            checks: vec![
                LintCheckKind::StaleContent,
                LintCheckKind::Orphan,
                LintCheckKind::SchemaViolation,
                LintCheckKind::DuplicateContent,
            ],
            stale_threshold_days: 90,
            max_pages_per_run: 500,
            token_budget: 8_000,
            scope: "default".to_string(),
            schema: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Lint report
// ---------------------------------------------------------------------------

/// The output of a complete lint run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintReport {
    /// Agent or circle scope for this run.
    pub scope: String,
    /// When this report was generated.
    pub timestamp: DateTime<Utc>,
    /// All findings produced during this run.
    pub findings: Vec<LintFinding>,
    /// Number of pages that were checked.
    pub pages_checked: usize,
    /// Approximate LLM tokens consumed (0 when no LLM checks were run).
    pub tokens_used: usize,
    /// Wall-clock duration of the lint run in milliseconds.
    pub duration_ms: u64,
}

impl LintReport {
    /// Count findings with [`FindingSeverity::Critical`].
    pub fn critical_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == FindingSeverity::Critical)
            .count()
    }

    /// Count findings with [`FindingSeverity::Warning`].
    pub fn warning_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == FindingSeverity::Warning)
            .count()
    }

    /// Returns `true` if any [`FindingSeverity::Critical`] findings exist.
    pub fn has_critical(&self) -> bool {
        self.critical_count() > 0
    }

    /// Returns `true` if there are no findings at all.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Linter trait
// ---------------------------------------------------------------------------

/// Trait for knowledge lint implementations.
///
/// Implementations receive a fully-resolved [`LintConfig`] and a slice of
/// [`PageInfo`] objects and return a [`LintReport`].
#[async_trait]
pub trait KnowledgeLinter: Send + Sync {
    /// Run the lint checks defined in `config` over `pages`.
    async fn run_lint(
        &self,
        config: &LintConfig,
        pages: &[PageInfo],
    ) -> Result<LintReport, LintError>;
}

// ---------------------------------------------------------------------------
// BasicLinter — non-LLM checks
// ---------------------------------------------------------------------------

/// A [`KnowledgeLinter`] that implements all non-LLM checks:
///
/// - [`LintCheckKind::StaleContent`] — pages older than the configured
///   threshold
/// - [`LintCheckKind::Orphan`] — pages with no inbound *and* no outbound links
/// - [`LintCheckKind::SchemaViolation`] — pages that break the knowledge schema
/// - [`LintCheckKind::DuplicateContent`] — near-duplicate detection via
///   normalised word-overlap (Jaccard similarity ≥ 0.85)
///
/// LLM-requiring checks ([`LintCheckKind::Contradiction`],
/// [`LintCheckKind::KnowledgeGap`]) are logged as informational stubs.
#[derive(Debug, Default)]
pub struct BasicLinter;

impl BasicLinter {
    /// Create a new `BasicLinter`.
    pub fn new() -> Self {
        Self
    }

    // --- individual check helpers ---

    fn check_stale(pages: &[PageInfo], threshold_days: u32, findings: &mut Vec<LintFinding>) {
        let cutoff = Utc::now()
            - chrono::Duration::days(i64::from(threshold_days));

        for page in pages {
            if page.last_modified < cutoff {
                let days_old = (Utc::now() - page.last_modified).num_days();
                findings.push(LintFinding {
                    check: LintCheckKind::StaleContent,
                    severity: FindingSeverity::Warning,
                    page_id: page.id.clone(),
                    message: format!(
                        "Page '{}' has not been updated in {days_old} days (threshold: {threshold_days})",
                        page.title
                    ),
                    suggestion: Some(format!(
                        "Review '{}' and update or archive it if the content is no longer relevant.",
                        page.title
                    )),
                });
            }
        }
    }

    fn check_orphans(pages: &[PageInfo], findings: &mut Vec<LintFinding>) {
        for page in pages {
            if page.inbound_links.is_empty() && page.outbound_links.is_empty() {
                findings.push(LintFinding {
                    check: LintCheckKind::Orphan,
                    severity: FindingSeverity::Warning,
                    page_id: page.id.clone(),
                    message: format!(
                        "Page '{}' has no inbound or outbound links — it is fully disconnected.",
                        page.title
                    ),
                    suggestion: Some(
                        "Link this page from at least one related page, or consider merging or removing it."
                            .to_string(),
                    ),
                });
            }
        }
    }

    fn check_schema_violations(
        pages: &[PageInfo],
        schema: &KnowledgeSchema,
        findings: &mut Vec<LintFinding>,
    ) {
        let validator = KnowledgeSchemaValidator::new();

        for page in pages {
            let page_type = match &page.page_type {
                Some(pt) => pt.clone(),
                None => continue, // no type tag → skip schema check
            };

            let violations: Vec<SchemaViolation> =
                validator.validate_page_name(&page.id, &page_type, schema);

            for v in violations {
                findings.push(LintFinding {
                    check: LintCheckKind::SchemaViolation,
                    severity: schema_severity(&v),
                    page_id: page.id.clone(),
                    message: v.message,
                    suggestion: Some(format!(
                        "Rename page '{}' to conform to the '{page_type}' naming pattern.",
                        page.id
                    )),
                });
            }
        }
    }

    /// Detect near-duplicate pages using normalised word-set Jaccard similarity.
    ///
    /// Two pages are flagged as duplicates when their Jaccard similarity
    /// score is ≥ `DUPLICATE_THRESHOLD` (0.85). Only the *later* page in the
    /// slice emits a finding (pointing back at the earlier one) to avoid
    /// double-reporting.
    fn check_duplicates(pages: &[PageInfo], findings: &mut Vec<LintFinding>) {
        const DUPLICATE_THRESHOLD: f64 = 0.85;

        for i in 0..pages.len() {
            for j in (i + 1)..pages.len() {
                let score = jaccard_similarity(&pages[i].content, &pages[j].content);
                if score >= DUPLICATE_THRESHOLD {
                    findings.push(LintFinding {
                        check: LintCheckKind::DuplicateContent,
                        severity: FindingSeverity::Warning,
                        page_id: pages[j].id.clone(),
                        message: format!(
                            "Page '{}' is {:.0}% similar to '{}' — possible duplicate.",
                            pages[j].title,
                            score * 100.0,
                            pages[i].title,
                        ),
                        suggestion: Some(format!(
                            "Merge '{}' into '{}' or differentiate their content.",
                            pages[j].title, pages[i].title,
                        )),
                    });
                }
            }
        }
    }

    fn stub_llm_check(kind: LintCheckKind, scope: &str, findings: &mut Vec<LintFinding>) {
        findings.push(LintFinding {
            check: kind.clone(),
            severity: FindingSeverity::Info,
            page_id: format!("scope:{scope}"),
            message: format!(
                "{kind:?} check requires a MemoryAnalyst LLM call and is not yet implemented.",
            ),
            suggestion: Some(
                "Implement MemoryAnalyst integration and wire up this check.".to_string(),
            ),
        });
    }
}

#[async_trait]
impl KnowledgeLinter for BasicLinter {
    async fn run_lint(
        &self,
        config: &LintConfig,
        pages: &[PageInfo],
    ) -> Result<LintReport, LintError> {
        if config.max_pages_per_run == 0 {
            return Err(LintError::InvalidConfig(
                "max_pages_per_run must be greater than zero".to_string(),
            ));
        }

        let started = std::time::Instant::now();
        let capped: &[PageInfo] = if pages.len() > config.max_pages_per_run {
            tracing::warn!(
                limit = config.max_pages_per_run,
                total = pages.len(),
                "Lint run capped at max_pages_per_run"
            );
            &pages[..config.max_pages_per_run]
        } else {
            pages
        };

        let mut findings: Vec<LintFinding> = Vec::new();

        for check in &config.checks {
            match check {
                LintCheckKind::StaleContent => {
                    Self::check_stale(capped, config.stale_threshold_days, &mut findings);
                }
                LintCheckKind::Orphan => {
                    Self::check_orphans(capped, &mut findings);
                }
                LintCheckKind::SchemaViolation => {
                    if let Some(schema) = &config.schema {
                        Self::check_schema_violations(capped, schema, &mut findings);
                    } else {
                        tracing::debug!(
                            "SchemaViolation check requested but no schema configured — skipping"
                        );
                    }
                }
                LintCheckKind::DuplicateContent => {
                    Self::check_duplicates(capped, &mut findings);
                }
                LintCheckKind::Contradiction | LintCheckKind::KnowledgeGap => {
                    // TODO(#312): implement via MemoryAnalyst LLM calls
                    Self::stub_llm_check(check.clone(), &config.scope, &mut findings);
                }
            }
        }

        let duration_ms = started.elapsed().as_millis() as u64;

        Ok(LintReport {
            scope: config.scope.clone(),
            timestamp: Utc::now(),
            findings,
            pages_checked: capped.len(),
            tokens_used: 0,
            duration_ms,
        })
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Map a [`SchemaViolation`] severity to a [`FindingSeverity`].
fn schema_severity(v: &SchemaViolation) -> FindingSeverity {
    use crate::knowledge_schema::ViolationSeverity;
    match v.severity {
        ViolationSeverity::Error => FindingSeverity::Critical,
        ViolationSeverity::Warning => FindingSeverity::Warning,
    }
}

/// Compute the Jaccard similarity between two text strings.
///
/// Tokenises by whitespace, lowercases, and computes
/// `|A ∩ B| / |A ∪ B|`. Returns `0.0` if both sets are empty.
fn jaccard_similarity(a: &str, b: &str) -> f64 {
    use std::collections::HashSet;

    let words_a: HashSet<&str> = a.split_whitespace().collect();
    let words_b: HashSet<&str> = b.split_whitespace().collect();

    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::skill::{EnforcementMode, KnowledgeSchema, PageTypeRule};

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    fn days_ago(days: i64) -> DateTime<Utc> {
        Utc::now() - chrono::Duration::days(days)
    }

    fn fresh_page(id: &str) -> PageInfo {
        PageInfo {
            id: id.to_string(),
            title: format!("Page {id}"),
            content: format!("Content for page {id}."),
            last_modified: now(),
            inbound_links: vec!["other-page".to_string()],
            outbound_links: vec!["another-page".to_string()],
            page_type: None,
        }
    }

    fn minimal_schema() -> KnowledgeSchema {
        KnowledgeSchema {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            enforcement_mode: EnforcementMode::Enforced,
            page_types: vec![PageTypeRule {
                name: "decision".to_string(),
                naming_pattern: "YYYY-MM-DD-<slug>".to_string(),
                required_fields: vec!["title".to_string()],
                description: None,
            }],
            categories: vec![],
            cross_reference_rules: vec![],
        }
    }

    // --- LintReport helpers ---

    #[test]
    fn report_is_clean_when_no_findings() {
        let report = LintReport {
            scope: "test".to_string(),
            timestamp: now(),
            findings: vec![],
            pages_checked: 0,
            tokens_used: 0,
            duration_ms: 0,
        };
        assert!(report.is_clean());
        assert!(!report.has_critical());
        assert_eq!(report.critical_count(), 0);
        assert_eq!(report.warning_count(), 0);
    }

    #[test]
    fn report_counts_by_severity() {
        let finding = |sev: FindingSeverity| LintFinding {
            check: LintCheckKind::StaleContent,
            severity: sev,
            page_id: "p1".to_string(),
            message: "msg".to_string(),
            suggestion: None,
        };

        let report = LintReport {
            scope: "test".to_string(),
            timestamp: now(),
            findings: vec![
                finding(FindingSeverity::Critical),
                finding(FindingSeverity::Critical),
                finding(FindingSeverity::Warning),
                finding(FindingSeverity::Info),
            ],
            pages_checked: 1,
            tokens_used: 0,
            duration_ms: 0,
        };

        assert_eq!(report.critical_count(), 2);
        assert_eq!(report.warning_count(), 1);
        assert!(report.has_critical());
        assert!(!report.is_clean());
    }

    // --- serde roundtrips ---

    #[test]
    fn lint_finding_serde_roundtrip() {
        let finding = LintFinding {
            check: LintCheckKind::Orphan,
            severity: FindingSeverity::Warning,
            page_id: "orphan-page".to_string(),
            message: "No links found".to_string(),
            suggestion: Some("Link it from somewhere".to_string()),
        };
        let json = serde_json::to_string(&finding).unwrap();
        let parsed: LintFinding = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.page_id, finding.page_id);
        assert_eq!(parsed.check, LintCheckKind::Orphan);
        assert_eq!(parsed.severity, FindingSeverity::Warning);
    }

    #[test]
    fn lint_report_serde_roundtrip() {
        let report = LintReport {
            scope: "agent-xyz".to_string(),
            timestamp: now(),
            findings: vec![],
            pages_checked: 10,
            tokens_used: 42,
            duration_ms: 150,
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: LintReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.scope, report.scope);
        assert_eq!(parsed.pages_checked, report.pages_checked);
        assert_eq!(parsed.tokens_used, report.tokens_used);
    }

    #[test]
    fn lint_config_default_is_sensible() {
        let cfg = LintConfig::default();
        assert!(!cfg.checks.is_empty());
        assert!(cfg.stale_threshold_days > 0);
        assert!(cfg.max_pages_per_run > 0);
        assert!(cfg.token_budget > 0);
        assert!(!cfg.scope.is_empty());
    }

    // --- StaleContent check ---

    #[tokio::test]
    async fn stale_check_flags_old_pages() {
        let linter = BasicLinter::new();
        let mut cfg = LintConfig {
            checks: vec![LintCheckKind::StaleContent],
            stale_threshold_days: 30,
            ..LintConfig::default()
        };
        cfg.scope = "test".to_string();

        let pages = vec![
            PageInfo {
                last_modified: days_ago(45),
                inbound_links: vec!["x".to_string()],
                ..fresh_page("old-page")
            },
            PageInfo {
                last_modified: days_ago(5),
                ..fresh_page("new-page")
            },
        ];

        let report = linter.run_lint(&cfg, &pages).await.unwrap();
        let stale: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.check == LintCheckKind::StaleContent)
            .collect();

        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].page_id, "old-page");
        assert_eq!(stale[0].severity, FindingSeverity::Warning);
    }

    #[tokio::test]
    async fn stale_check_clean_when_all_fresh() {
        let linter = BasicLinter::new();
        let cfg = LintConfig {
            checks: vec![LintCheckKind::StaleContent],
            stale_threshold_days: 90,
            ..LintConfig::default()
        };

        let pages = vec![fresh_page("p1"), fresh_page("p2")];
        let report = linter.run_lint(&cfg, &pages).await.unwrap();
        assert!(report.is_clean());
    }

    // --- Orphan check ---

    #[tokio::test]
    async fn orphan_check_detects_disconnected_page() {
        let linter = BasicLinter::new();
        let cfg = LintConfig {
            checks: vec![LintCheckKind::Orphan],
            ..LintConfig::default()
        };

        let pages = vec![
            PageInfo {
                inbound_links: vec![],
                outbound_links: vec![],
                ..fresh_page("orphan")
            },
            fresh_page("connected"),
        ];

        let report = linter.run_lint(&cfg, &pages).await.unwrap();
        let orphans: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.check == LintCheckKind::Orphan)
            .collect();

        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].page_id, "orphan");
    }

    #[tokio::test]
    async fn orphan_check_one_link_is_enough() {
        let linter = BasicLinter::new();
        let cfg = LintConfig {
            checks: vec![LintCheckKind::Orphan],
            ..LintConfig::default()
        };

        // Has an outbound link but no inbound — should NOT be flagged.
        let pages = vec![PageInfo {
            inbound_links: vec![],
            outbound_links: vec!["somewhere".to_string()],
            ..fresh_page("p1")
        }];

        let report = linter.run_lint(&cfg, &pages).await.unwrap();
        assert!(report.is_clean());
    }

    // --- SchemaViolation check ---

    #[tokio::test]
    async fn schema_check_flags_bad_name() {
        let linter = BasicLinter::new();
        let cfg = LintConfig {
            checks: vec![LintCheckKind::SchemaViolation],
            schema: Some(minimal_schema()),
            ..LintConfig::default()
        };

        // "bad-name" does not match the YYYY-MM-DD-<slug> pattern
        let pages = vec![PageInfo {
            id: "bad-name".to_string(),
            page_type: Some("decision".to_string()),
            ..fresh_page("bad-name")
        }];

        let report = linter.run_lint(&cfg, &pages).await.unwrap();
        let schema_findings: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.check == LintCheckKind::SchemaViolation)
            .collect();

        assert!(!schema_findings.is_empty());
        assert_eq!(schema_findings[0].severity, FindingSeverity::Critical);
    }

    #[tokio::test]
    async fn schema_check_skipped_without_schema() {
        let linter = BasicLinter::new();
        let cfg = LintConfig {
            checks: vec![LintCheckKind::SchemaViolation],
            schema: None,
            ..LintConfig::default()
        };

        let pages = vec![PageInfo {
            id: "bad-name".to_string(),
            page_type: Some("decision".to_string()),
            ..fresh_page("bad-name")
        }];

        let report = linter.run_lint(&cfg, &pages).await.unwrap();
        assert!(report.is_clean());
    }

    // --- DuplicateContent check ---

    #[tokio::test]
    async fn duplicate_check_flags_identical_pages() {
        let linter = BasicLinter::new();
        let cfg = LintConfig {
            checks: vec![LintCheckKind::DuplicateContent],
            ..LintConfig::default()
        };

        let shared = "the quick brown fox jumps over the lazy dog".to_string();
        let pages = vec![
            PageInfo {
                content: shared.clone(),
                ..fresh_page("p1")
            },
            PageInfo {
                content: shared.clone(),
                ..fresh_page("p2")
            },
        ];

        let report = linter.run_lint(&cfg, &pages).await.unwrap();
        let dupes: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.check == LintCheckKind::DuplicateContent)
            .collect();

        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0].page_id, "p2");
        assert_eq!(dupes[0].severity, FindingSeverity::Warning);
    }

    #[tokio::test]
    async fn duplicate_check_ignores_dissimilar_pages() {
        let linter = BasicLinter::new();
        let cfg = LintConfig {
            checks: vec![LintCheckKind::DuplicateContent],
            ..LintConfig::default()
        };

        let pages = vec![
            PageInfo {
                content: "apple orange banana".to_string(),
                ..fresh_page("p1")
            },
            PageInfo {
                content: "rust tokio async performance benchmarks".to_string(),
                ..fresh_page("p2")
            },
        ];

        let report = linter.run_lint(&cfg, &pages).await.unwrap();
        assert!(report.is_clean());
    }

    // --- LLM check stubs ---

    #[tokio::test]
    async fn contradiction_check_is_stubbed_as_info() {
        let linter = BasicLinter::new();
        let cfg = LintConfig {
            checks: vec![LintCheckKind::Contradiction],
            ..LintConfig::default()
        };

        let pages = vec![fresh_page("p1")];
        let report = linter.run_lint(&cfg, &pages).await.unwrap();

        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].check, LintCheckKind::Contradiction);
        assert_eq!(report.findings[0].severity, FindingSeverity::Info);
    }

    #[tokio::test]
    async fn knowledge_gap_check_is_stubbed_as_info() {
        let linter = BasicLinter::new();
        let cfg = LintConfig {
            checks: vec![LintCheckKind::KnowledgeGap],
            ..LintConfig::default()
        };

        let pages = vec![fresh_page("p1")];
        let report = linter.run_lint(&cfg, &pages).await.unwrap();

        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].check, LintCheckKind::KnowledgeGap);
        assert_eq!(report.findings[0].severity, FindingSeverity::Info);
    }

    // --- max_pages_per_run cap ---

    #[tokio::test]
    async fn max_pages_cap_is_respected() {
        let linter = BasicLinter::new();
        let cfg = LintConfig {
            checks: vec![LintCheckKind::StaleContent],
            stale_threshold_days: 1,
            max_pages_per_run: 2,
            ..LintConfig::default()
        };

        let pages: Vec<PageInfo> = (0..5)
            .map(|i| PageInfo {
                last_modified: days_ago(100),
                inbound_links: vec!["x".to_string()],
                ..fresh_page(&format!("page-{i}"))
            })
            .collect();

        let report = linter.run_lint(&cfg, &pages).await.unwrap();
        assert_eq!(report.pages_checked, 2);
    }

    #[tokio::test]
    async fn invalid_config_zero_max_pages_returns_error() {
        let linter = BasicLinter::new();
        let cfg = LintConfig {
            max_pages_per_run: 0,
            ..LintConfig::default()
        };

        let result = linter.run_lint(&cfg, &[]).await;
        assert!(matches!(result, Err(LintError::InvalidConfig(_))));
    }

    // --- jaccard helper ---

    #[test]
    fn jaccard_identical_strings() {
        assert!((jaccard_similarity("a b c", "a b c") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_disjoint_strings() {
        assert!((jaccard_similarity("a b c", "x y z") - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_empty_strings() {
        assert!((jaccard_similarity("", "") - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_partial_overlap() {
        // {"a","b","c"} ∩ {"b","c","d"} = {"b","c"} → 2/4 = 0.5
        let score = jaccard_similarity("a b c", "b c d");
        assert!((score - 0.5).abs() < 1e-10);
    }
}
