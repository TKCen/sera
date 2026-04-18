//! Metering and budget types — token usage tracking and quota enforcement.

use serde::{Deserialize, Serialize};

/// A single token usage record.
/// Maps from TS: UsageRecord in metering/MeteringService.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub circle_id: Option<String>,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<UsageStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsageStatus {
    Success,
    Error,
}

/// Budget enforcement result.
/// Maps from TS: BudgetStatus in metering/MeteringService.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetStatus {
    pub allowed: bool,
    pub hourly_used: u64,
    pub hourly_quota: u64,
    pub daily_used: u64,
    pub daily_quota: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_status_serialize() {
        let status = BudgetStatus {
            allowed: true,
            hourly_used: 5000,
            hourly_quota: 100_000,
            daily_used: 50_000,
            daily_quota: 1_000_000,
        };
        let json = serde_json::to_string(&status).unwrap();
        let parsed: BudgetStatus = serde_json::from_str(&json).unwrap();
        assert!(parsed.allowed);
        assert_eq!(parsed.hourly_quota, 100_000);
    }

    #[test]
    fn usage_status_roundtrip() {
        for status in [UsageStatus::Success, UsageStatus::Error] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: UsageStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn usage_record_with_all_fields() {
        let record = UsageRecord {
            agent_id: "agent-123".to_string(),
            circle_id: Some("circle-456".to_string()),
            model: "claude-opus-4-6".to_string(),
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
            cost_usd: Some(0.05),
            latency_ms: Some(2500),
            status: Some(UsageStatus::Success),
        };
        let json = serde_json::to_string(&record).unwrap();
        let parsed: UsageRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.agent_id, "agent-123");
        assert_eq!(parsed.circle_id, Some("circle-456".to_string()));
        assert_eq!(parsed.total_tokens, 1500);
        assert_eq!(parsed.status, Some(UsageStatus::Success));
    }

    #[test]
    fn usage_record_minimal_fields() {
        let record = UsageRecord {
            agent_id: "agent-123".to_string(),
            circle_id: None,
            model: "gpt-4o".to_string(),
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            cost_usd: None,
            latency_ms: None,
            status: None,
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(!json.contains("circle_id"));
        assert!(!json.contains("cost_usd"));
        let parsed: UsageRecord = serde_json::from_str(&json).unwrap();
        assert!(parsed.circle_id.is_none());
        assert!(parsed.cost_usd.is_none());
    }

    #[test]
    fn budget_status_over_quota() {
        let status = BudgetStatus {
            allowed: false,
            hourly_used: 150_000,
            hourly_quota: 100_000,
            daily_used: 2_000_000,
            daily_quota: 1_000_000,
        };
        assert!(!status.allowed);
        assert!(status.hourly_used > status.hourly_quota);
        assert!(status.daily_used > status.daily_quota);
    }
}
