//! Improvement validation and automatic rollback for prompt version activations.
//!
//! After a prompt version is activated a 48-hour validation window opens.
//! During that window evaluator scoring is mandatory. The window is checked
//! every 6 hours (modelled here as a pure function — the pg-boss job is in the
//! full system). If scores drop >10% below the 30-day baseline the version is
//! marked as `Regressed` and the caller should trigger a rollback.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::PromptSection;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Outcome of a validation check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationOutcome {
    /// Scores improved or stayed stable.
    Confirmed,
    /// Scores within noise range — flag for human review.
    Neutral,
    /// Scores dropped >10% — auto-rollback triggered.
    Regressed,
    /// Validation still in progress (within 48h window).
    Pending,
}

/// Configuration for the validation system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    /// Duration of validation window in hours (default: 48).
    pub window_hours: u64,
    /// Check interval in hours (default: 6).
    pub check_interval_hours: u64,
    /// Baseline period in days (default: 30).
    pub baseline_days: u32,
    /// Regression threshold as fraction (default: 0.10 = 10%).
    pub regression_threshold: f64,
    /// Drift alert threshold for self-vs-evaluator delta (default: 0.5).
    pub drift_threshold: f64,
    /// Rolling window for drift detection in days (default: 7).
    pub drift_window_days: u32,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            window_hours: 48,
            check_interval_hours: 6,
            baseline_days: 30,
            regression_threshold: 0.10,
            drift_threshold: 0.5,
            drift_window_days: 7,
        }
    }
}

/// A record of an active validation window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWindow {
    pub id: String,
    pub agent_id: String,
    pub section: PromptSection,
    pub version: u32,
    pub previous_version: Option<u32>,
    pub baseline_score: f64,
    pub current_scores: Vec<f64>,
    pub outcome: ValidationOutcome,
    pub started_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_checked_at: Option<DateTime<Utc>>,
    /// Evaluator scoring is mandatory during the validation window.
    pub evaluator_mandatory: bool,
}

/// Drift detection alert emitted when self-score and evaluator-score diverge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftAlert {
    pub agent_id: String,
    pub self_score_avg: f64,
    pub evaluator_score_avg: f64,
    pub delta: f64,
    pub window_days: u32,
    pub detected_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("validation window not found: {0}")]
    WindowNotFound(String),
    #[error("validation window expired")]
    WindowExpired,
    #[error("invalid score: {0}")]
    InvalidScore(f64),
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// Manages validation windows and drift detection for prompt version activations.
pub struct ValidationManager {
    config: ValidationConfig,
    windows: Vec<ValidationWindow>,
}

impl ValidationManager {
    pub fn new(config: ValidationConfig) -> Self {
        Self {
            config,
            windows: Vec::new(),
        }
    }

    /// Start a validation window for a newly activated prompt version.
    ///
    /// Returns a reference to the created window.
    pub fn start_validation(
        &mut self,
        agent_id: &str,
        section: PromptSection,
        version: u32,
        previous_version: Option<u32>,
        baseline_score: f64,
    ) -> &ValidationWindow {
        let now = Utc::now();
        let window = ValidationWindow {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.to_owned(),
            section,
            version,
            previous_version,
            baseline_score,
            current_scores: Vec::new(),
            outcome: ValidationOutcome::Pending,
            started_at: now,
            expires_at: now + Duration::hours(self.config.window_hours as i64),
            last_checked_at: None,
            evaluator_mandatory: true,
        };
        self.windows.push(window);
        self.windows.last().expect("just pushed")
    }

    /// Record a new score during the validation window.
    pub fn record_score(&mut self, window_id: &str, score: f64) -> Result<(), ValidationError> {
        if !score.is_finite() {
            return Err(ValidationError::InvalidScore(score));
        }
        let window = self
            .windows
            .iter_mut()
            .find(|w| w.id == window_id)
            .ok_or_else(|| ValidationError::WindowNotFound(window_id.to_owned()))?;

        if Utc::now() > window.expires_at {
            return Err(ValidationError::WindowExpired);
        }

        window.current_scores.push(score);
        Ok(())
    }

    /// Check a validation window and determine its outcome.
    ///
    /// Called periodically (every 6 hours in production).
    pub fn check_validation(
        &mut self,
        window_id: &str,
    ) -> Result<ValidationOutcome, ValidationError> {
        let now = Utc::now();

        let window = self
            .windows
            .iter_mut()
            .find(|w| w.id == window_id)
            .ok_or_else(|| ValidationError::WindowNotFound(window_id.to_owned()))?;

        window.last_checked_at = Some(now);

        // Still within the validation window — no verdict yet.
        if now < window.expires_at {
            return Ok(ValidationOutcome::Pending);
        }

        // No scores recorded — flag for review.
        if window.current_scores.is_empty() {
            window.outcome = ValidationOutcome::Neutral;
            return Ok(ValidationOutcome::Neutral);
        }

        let avg: f64 =
            window.current_scores.iter().sum::<f64>() / window.current_scores.len() as f64;

        let baseline = window.baseline_score;
        let threshold = self.config.regression_threshold;

        // Within 2% of baseline → neutral (noise range).
        let noise_band = baseline * 0.02;
        let outcome = if (avg - baseline).abs() <= noise_band {
            ValidationOutcome::Neutral
        } else if avg < baseline * (1.0 - threshold) {
            ValidationOutcome::Regressed
        } else {
            ValidationOutcome::Confirmed
        };

        window.outcome = outcome;
        Ok(outcome)
    }

    /// Get all active (`Pending`) validation windows.
    pub fn active_windows(&self) -> Vec<&ValidationWindow> {
        self.windows
            .iter()
            .filter(|w| w.outcome == ValidationOutcome::Pending)
            .collect()
    }

    /// Get a validation window by ID.
    pub fn get_window(&self, window_id: &str) -> Option<&ValidationWindow> {
        self.windows.iter().find(|w| w.id == window_id)
    }

    /// Check for drift between self-scores and evaluator scores.
    ///
    /// Returns `Some(DriftAlert)` when `|self_avg - evaluator_avg| > drift_threshold`.
    pub fn check_drift(
        &self,
        agent_id: &str,
        self_scores: &[f64],
        evaluator_scores: &[f64],
    ) -> Option<DriftAlert> {
        if self_scores.is_empty() || evaluator_scores.is_empty() {
            return None;
        }

        let self_avg = self_scores.iter().sum::<f64>() / self_scores.len() as f64;
        let evaluator_avg =
            evaluator_scores.iter().sum::<f64>() / evaluator_scores.len() as f64;
        let delta = (self_avg - evaluator_avg).abs();

        if delta > self.config.drift_threshold {
            Some(DriftAlert {
                agent_id: agent_id.to_owned(),
                self_score_avg: self_avg,
                evaluator_score_avg: evaluator_avg,
                delta,
                window_days: self.config.drift_window_days,
                detected_at: Utc::now(),
            })
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> ValidationManager {
        ValidationManager::new(ValidationConfig::default())
    }

    /// Advance a window's expiry into the past so check_validation sees it as expired.
    fn expire_window(mgr: &mut ValidationManager, window_id: &str) {
        let w = mgr
            .windows
            .iter_mut()
            .find(|w| w.id == window_id)
            .unwrap();
        w.expires_at = Utc::now() - Duration::seconds(1);
    }

    #[test]
    fn start_validation_creates_pending_window_with_correct_expiry() {
        let mut mgr = manager();
        let w = mgr.start_validation(
            "agent-1",
            PromptSection::Role,
            2,
            Some(1),
            0.85,
        );

        assert_eq!(w.outcome, ValidationOutcome::Pending);
        assert!(w.evaluator_mandatory);
        assert_eq!(w.version, 2);
        assert_eq!(w.previous_version, Some(1));
        assert_eq!(w.baseline_score, 0.85);

        let expected_expiry = w.started_at + Duration::hours(48);
        // Allow a 2-second tolerance for test execution time.
        let diff = (w.expires_at - expected_expiry).num_seconds().abs();
        assert!(diff <= 2, "expiry should be ~48h from start");
    }

    #[test]
    fn record_score_updates_window() {
        let mut mgr = manager();
        let id = {
            let w = mgr.start_validation("agent-1", PromptSection::Role, 1, None, 0.80);
            w.id.clone()
        };
        mgr.record_score(&id, 0.82).unwrap();
        mgr.record_score(&id, 0.84).unwrap();
        let w = mgr.get_window(&id).unwrap();
        assert_eq!(w.current_scores, vec![0.82, 0.84]);
    }

    #[test]
    fn window_within_48h_returns_pending() {
        let mut mgr = manager();
        let id = {
            let w = mgr.start_validation("agent-1", PromptSection::Role, 1, None, 0.80);
            w.id.clone()
        };
        mgr.record_score(&id, 0.82).unwrap();
        // Window has not expired yet.
        let outcome = mgr.check_validation(&id).unwrap();
        assert_eq!(outcome, ValidationOutcome::Pending);
    }

    #[test]
    fn expired_window_with_good_scores_returns_confirmed() {
        let mut mgr = manager();
        let id = {
            let w = mgr.start_validation("agent-1", PromptSection::Role, 1, None, 0.80);
            w.id.clone()
        };
        // Scores clearly above baseline.
        mgr.record_score(&id, 0.90).unwrap();
        mgr.record_score(&id, 0.88).unwrap();
        expire_window(&mut mgr, &id);
        let outcome = mgr.check_validation(&id).unwrap();
        assert_eq!(outcome, ValidationOutcome::Confirmed);
    }

    #[test]
    fn expired_window_with_regressed_scores_returns_regressed() {
        let mut mgr = manager();
        let id = {
            let w = mgr.start_validation("agent-1", PromptSection::Role, 1, None, 0.80);
            w.id.clone()
        };
        // >10% drop from 0.80 baseline.
        mgr.record_score(&id, 0.60).unwrap();
        mgr.record_score(&id, 0.62).unwrap();
        expire_window(&mut mgr, &id);
        let outcome = mgr.check_validation(&id).unwrap();
        assert_eq!(outcome, ValidationOutcome::Regressed);
    }

    #[test]
    fn expired_window_with_marginal_scores_returns_neutral() {
        let mut mgr = manager();
        let id = {
            let w = mgr.start_validation("agent-1", PromptSection::Role, 1, None, 0.80);
            w.id.clone()
        };
        // Within 2% of 0.80 → neutral.
        mgr.record_score(&id, 0.801).unwrap();
        expire_window(&mut mgr, &id);
        let outcome = mgr.check_validation(&id).unwrap();
        assert_eq!(outcome, ValidationOutcome::Neutral);
    }

    #[test]
    fn expired_window_no_scores_returns_neutral() {
        let mut mgr = manager();
        let id = {
            let w = mgr.start_validation("agent-1", PromptSection::Role, 1, None, 0.80);
            w.id.clone()
        };
        expire_window(&mut mgr, &id);
        let outcome = mgr.check_validation(&id).unwrap();
        assert_eq!(outcome, ValidationOutcome::Neutral);
    }

    #[test]
    fn drift_detection_triggers_when_delta_exceeds_threshold() {
        let mgr = manager();
        let self_scores = vec![0.9, 0.9, 0.9];
        let eval_scores = vec![0.3, 0.3, 0.3];
        let alert = mgr.check_drift("agent-1", &self_scores, &eval_scores);
        assert!(alert.is_some());
        let alert = alert.unwrap();
        assert!(alert.delta > 0.5);
        assert_eq!(alert.agent_id, "agent-1");
    }

    #[test]
    fn drift_detection_returns_none_when_delta_within_threshold() {
        let mgr = manager();
        let self_scores = vec![0.80, 0.82];
        let eval_scores = vec![0.81, 0.83];
        let alert = mgr.check_drift("agent-1", &self_scores, &eval_scores);
        assert!(alert.is_none());
    }

    #[test]
    fn drift_detection_returns_none_for_empty_scores() {
        let mgr = manager();
        assert!(mgr.check_drift("agent-1", &[], &[0.5]).is_none());
        assert!(mgr.check_drift("agent-1", &[0.5], &[]).is_none());
        assert!(mgr.check_drift("agent-1", &[], &[]).is_none());
    }

    #[test]
    fn active_windows_filters_only_pending() {
        let mut mgr = manager();
        let id1 = {
            let w = mgr.start_validation("agent-1", PromptSection::Role, 1, None, 0.80);
            w.id.clone()
        };
        let id2 = {
            let w = mgr.start_validation("agent-2", PromptSection::Principles, 1, None, 0.75);
            w.id.clone()
        };

        // Record a score then expire and resolve the second window.
        mgr.record_score(&id2, 0.76).unwrap();
        expire_window(&mut mgr, &id2);
        mgr.check_validation(&id2).unwrap();

        let active = mgr.active_windows();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, id1);
    }

    #[test]
    fn record_score_on_missing_window_returns_error() {
        let mut mgr = manager();
        let err = mgr.record_score("nonexistent", 0.5).unwrap_err();
        assert!(matches!(err, ValidationError::WindowNotFound(_)));
    }

    #[test]
    fn invalid_score_returns_error() {
        let mut mgr = manager();
        let id = {
            let w = mgr.start_validation("agent-1", PromptSection::Role, 1, None, 0.80);
            w.id.clone()
        };
        let err = mgr.record_score(&id, f64::NAN).unwrap_err();
        assert!(matches!(err, ValidationError::InvalidScore(_)));
    }
}
