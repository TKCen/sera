//! Training-data export pipeline types — schema for `training_exports` table.
//!
//! Defines the request, record, and supporting enums for the fine-tuning
//! data export pipeline. Implementation (job, repository, endpoint) is
//! tracked separately.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── TrainingExportFormat ─────────────────────────────────────────────────────

/// Output format for a training-data export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingExportFormat {
    OpenaiJsonl,
    Alpaca,
    ShareGpt,
}

// ── TrainingExportStatus ─────────────────────────────────────────────────────

/// Lifecycle status of a training-data export job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingExportStatus {
    Queued,
    Running,
    Complete,
    Failed,
}

// ── TrainingExportFilter ─────────────────────────────────────────────────────

/// Optional filters applied when selecting sessions for export.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrainingExportFilter {
    /// Only include sessions with an evaluation score >= this value.
    pub min_score: Option<f64>,
    /// Only include sessions created on or after this timestamp.
    pub date_from: Option<DateTime<Utc>>,
    /// Only include sessions created on or before this timestamp.
    pub date_to: Option<DateTime<Utc>>,
    /// Only include sessions triggered by this trigger type.
    pub trigger_type: Option<String>,
}

// ── TrainingExportRequest ────────────────────────────────────────────────────

/// API request body for creating a new training-data export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingExportRequest {
    /// Target output format.
    pub format: TrainingExportFormat,
    /// Filters controlling which sessions are included.
    pub filter: TrainingExportFilter,
    /// Whether PII should be redacted from the exported data.
    pub pii_redaction: bool,
}

// ── TrainingExportRecord ─────────────────────────────────────────────────────

/// A persisted training-data export record, as stored in `training_exports`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingExportRecord {
    pub id: Uuid,
    pub format: TrainingExportFormat,
    pub filter: TrainingExportFilter,
    pub pii_redaction: bool,
    pub status: TrainingExportStatus,
    pub total_records: Option<i32>,
    pub output_path: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Serde round-trip of TrainingExportRequest (JSON → struct → JSON).
    #[test]
    fn request_serde_roundtrip() {
        let req = TrainingExportRequest {
            format: TrainingExportFormat::OpenaiJsonl,
            filter: TrainingExportFilter {
                min_score: Some(0.8),
                date_from: None,
                date_to: None,
                trigger_type: Some("user".to_string()),
            },
            pii_redaction: true,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: TrainingExportRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.format, req.format);
        assert_eq!(parsed.pii_redaction, req.pii_redaction);
        assert_eq!(parsed.filter.min_score, req.filter.min_score);
        assert_eq!(parsed.filter.trigger_type, req.filter.trigger_type);
    }

    // Enum variants serialize as expected string values.
    #[test]
    fn format_enum_serialization() {
        assert_eq!(
            serde_json::to_string(&TrainingExportFormat::OpenaiJsonl).unwrap(),
            "\"openai_jsonl\""
        );
        assert_eq!(
            serde_json::to_string(&TrainingExportFormat::Alpaca).unwrap(),
            "\"alpaca\""
        );
        assert_eq!(
            serde_json::to_string(&TrainingExportFormat::ShareGpt).unwrap(),
            "\"share_gpt\""
        );
    }

    // Empty filter round-trips cleanly.
    #[test]
    fn empty_filter_roundtrip() {
        let filter = TrainingExportFilter::default();
        let json = serde_json::to_string(&filter).unwrap();
        let parsed: TrainingExportFilter = serde_json::from_str(&json).unwrap();
        assert!(parsed.min_score.is_none());
        assert!(parsed.date_from.is_none());
        assert!(parsed.date_to.is_none());
        assert!(parsed.trigger_type.is_none());
    }

    // Missing optional fields in JSON → struct uses None defaults.
    #[test]
    fn request_missing_optional_fields() {
        let json = r#"{"format":"alpaca","filter":{},"pii_redaction":false}"#;
        let req: TrainingExportRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.format, TrainingExportFormat::Alpaca);
        assert!(!req.pii_redaction);
        assert!(req.filter.min_score.is_none());
        assert!(req.filter.date_from.is_none());
        assert!(req.filter.date_to.is_none());
        assert!(req.filter.trigger_type.is_none());
    }
}
