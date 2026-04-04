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
}
