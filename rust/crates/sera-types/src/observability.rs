//! Observability types — trace context, audit entries, run evidence, and cost attribution.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// W3C-compatible trace context for distributed tracing propagation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    pub baggage: HashMap<String, String>,
}

/// Outcome of an audited action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "reason")]
pub enum AuditOutcome {
    Success,
    Failure(String),
    Denied(String),
}

/// A single security-relevant audit event entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub principal_id: String,
    pub action: String,
    pub resource: String,
    pub outcome: AuditOutcome,
    pub metadata: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_ctx: Option<TraceContext>,
}

/// Evidence of a single tool invocation within a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEvidence {
    pub tool_name: String,
    /// SHA-256 or similar hash of the arguments (not stored verbatim for privacy).
    pub arguments_hash: String,
    pub result_summary: String,
    pub duration_ms: u64,
    pub risk_level: crate::tool::RiskLevel,
}

/// Outcome of a completed agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "reason")]
pub enum RunOutcome {
    Completed,
    Failed(String),
    Timeout,
    Cancelled,
}

/// Full evidence bundle for a single agent run — the durable proof of execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofBundle {
    pub run_id: String,
    pub agent_id: String,
    pub session_key: String,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub turn_count: u32,
    pub tool_calls: Vec<ToolCallEvidence>,
    pub total_tokens: crate::runtime::TokenUsage,
    pub outcome: RunOutcome,
}

/// Token usage and cost attribution for a single model call or session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostAttribution {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal_id: Option<String>,
    pub model: String,
    pub tokens: crate::runtime::TokenUsage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd: Option<f64>,
    pub timestamp: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::TokenUsage;
    use crate::tool::RiskLevel;

    fn make_trace_ctx() -> TraceContext {
        TraceContext {
            trace_id: "trace-abc123".to_string(),
            span_id: "span-001".to_string(),
            parent_span_id: Some("span-000".to_string()),
            baggage: HashMap::from([("session".to_string(), "sess-1".to_string())]),
        }
    }

    #[test]
    fn trace_context_construction_and_serde() {
        let ctx = make_trace_ctx();
        let json = serde_json::to_string(&ctx).unwrap();
        let parsed: TraceContext = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.trace_id, "trace-abc123");
        assert_eq!(parsed.span_id, "span-001");
        assert_eq!(parsed.parent_span_id, Some("span-000".to_string()));
        assert_eq!(
            parsed.baggage.get("session").map(String::as_str),
            Some("sess-1")
        );
    }

    #[test]
    fn trace_context_without_parent() {
        let ctx = TraceContext {
            trace_id: "t1".to_string(),
            span_id: "s1".to_string(),
            parent_span_id: None,
            baggage: HashMap::new(),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        assert!(!json.contains("parent_span_id"));
        let parsed: TraceContext = serde_json::from_str(&json).unwrap();
        assert!(parsed.parent_span_id.is_none());
    }

    #[test]
    fn audit_entry_roundtrip() {
        let entry = AuditEntry {
            id: "audit-1".to_string(),
            timestamp: DateTime::parse_from_rfc3339("2026-04-09T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            principal_id: "principal-abc".to_string(),
            action: "tool.execute".to_string(),
            resource: "bash".to_string(),
            outcome: AuditOutcome::Success,
            metadata: HashMap::from([(
                "risk".to_string(),
                serde_json::Value::String("low".to_string()),
            )]),
            trace_ctx: Some(make_trace_ctx()),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: AuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "audit-1");
        assert_eq!(parsed.principal_id, "principal-abc");
        assert_eq!(parsed.action, "tool.execute");
        assert!(matches!(parsed.outcome, AuditOutcome::Success));
        assert!(parsed.trace_ctx.is_some());
    }

    #[test]
    fn audit_outcome_variants() {
        let outcomes = [
            AuditOutcome::Success,
            AuditOutcome::Failure("disk full".to_string()),
            AuditOutcome::Denied("policy violation".to_string()),
        ];
        for outcome in &outcomes {
            let json = serde_json::to_string(outcome).unwrap();
            let _parsed: AuditOutcome = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn proof_bundle_with_tool_evidence() {
        let bundle = ProofBundle {
            run_id: "run-001".to_string(),
            agent_id: "agent-xyz".to_string(),
            session_key: "sess-42".to_string(),
            started_at: DateTime::parse_from_rfc3339("2026-04-09T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            completed_at: Some(
                DateTime::parse_from_rfc3339("2026-04-09T10:01:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            turn_count: 3,
            tool_calls: vec![ToolCallEvidence {
                tool_name: "bash".to_string(),
                arguments_hash: "sha256:abcdef".to_string(),
                result_summary: "exit 0".to_string(),
                duration_ms: 250,
                risk_level: RiskLevel::Read,
            }],
            total_tokens: TokenUsage {
                prompt_tokens: 1000,
                completion_tokens: 500,
                total_tokens: 1500,
            },
            outcome: RunOutcome::Completed,
        };
        let json = serde_json::to_string(&bundle).unwrap();
        let parsed: ProofBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.run_id, "run-001");
        assert_eq!(parsed.turn_count, 3);
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].tool_name, "bash");
        assert_eq!(parsed.total_tokens.total_tokens, 1500);
        assert!(matches!(parsed.outcome, RunOutcome::Completed));
    }

    #[test]
    fn cost_attribution_serde() {
        let cost = CostAttribution {
            agent_id: "agent-1".to_string(),
            session_key: Some("sess-99".to_string()),
            principal_id: Some("user-42".to_string()),
            model: "claude-opus-4-6".to_string(),
            tokens: TokenUsage {
                prompt_tokens: 800,
                completion_tokens: 200,
                total_tokens: 1000,
            },
            estimated_cost_usd: Some(0.03),
            timestamp: DateTime::parse_from_rfc3339("2026-04-09T09:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        };
        let json = serde_json::to_string(&cost).unwrap();
        let parsed: CostAttribution = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.agent_id, "agent-1");
        assert_eq!(parsed.model, "claude-opus-4-6");
        assert_eq!(parsed.tokens.total_tokens, 1000);
        assert_eq!(parsed.estimated_cost_usd, Some(0.03));
    }

    #[test]
    fn cost_attribution_minimal() {
        let cost = CostAttribution {
            agent_id: "agent-2".to_string(),
            session_key: None,
            principal_id: None,
            model: "gpt-4o".to_string(),
            tokens: TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
            },
            estimated_cost_usd: None,
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&cost).unwrap();
        assert!(!json.contains("session_key"));
        assert!(!json.contains("principal_id"));
        assert!(!json.contains("estimated_cost_usd"));
        let parsed: CostAttribution = serde_json::from_str(&json).unwrap();
        assert!(parsed.session_key.is_none());
        assert!(parsed.estimated_cost_usd.is_none());
    }

    #[test]
    fn run_outcome_variants() {
        let outcomes = [
            RunOutcome::Completed,
            RunOutcome::Failed("oom".to_string()),
            RunOutcome::Timeout,
            RunOutcome::Cancelled,
        ];
        for outcome in &outcomes {
            let json = serde_json::to_string(outcome).unwrap();
            let _parsed: RunOutcome = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn run_outcome_json_shapes() {
        // Completed — no content field
        let json = serde_json::to_string(&RunOutcome::Completed).unwrap();
        assert!(json.contains("\"type\":\"completed\""));
        assert!(!json.contains("reason"));

        // Failed — reason field present
        let json = serde_json::to_string(&RunOutcome::Failed("oom killed".to_string())).unwrap();
        assert!(json.contains("\"type\":\"failed\""));
        assert!(json.contains("oom killed"));

        // Timeout — no content field
        let json = serde_json::to_string(&RunOutcome::Timeout).unwrap();
        assert!(json.contains("\"type\":\"timeout\""));

        // Cancelled — no content field
        let json = serde_json::to_string(&RunOutcome::Cancelled).unwrap();
        assert!(json.contains("\"type\":\"cancelled\""));
    }

    #[test]
    fn audit_outcome_json_shapes() {
        // Success — no reason field
        let json = serde_json::to_string(&AuditOutcome::Success).unwrap();
        assert!(json.contains("\"type\":\"success\""));
        assert!(!json.contains("reason"));

        // Failure — reason field present
        let json = serde_json::to_string(&AuditOutcome::Failure("timeout".to_string())).unwrap();
        assert!(json.contains("\"type\":\"failure\""));
        assert!(json.contains("timeout"));

        // Denied — reason field present
        let json = serde_json::to_string(&AuditOutcome::Denied("tier policy".to_string())).unwrap();
        assert!(json.contains("\"type\":\"denied\""));
        assert!(json.contains("tier policy"));
    }

    #[test]
    fn proof_bundle_optional_completed_at_omitted() {
        let bundle = ProofBundle {
            run_id: "run-no-end".to_string(),
            agent_id: "agent-1".to_string(),
            session_key: "sess-1".to_string(),
            started_at: chrono::DateTime::parse_from_rfc3339("2026-04-17T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
            completed_at: None,
            turn_count: 1,
            tool_calls: vec![],
            total_tokens: TokenUsage::default(),
            outcome: RunOutcome::Timeout,
        };
        let json = serde_json::to_string(&bundle).unwrap();
        assert!(!json.contains("completed_at"));
        let parsed: ProofBundle = serde_json::from_str(&json).unwrap();
        assert!(parsed.completed_at.is_none());
    }

    #[test]
    fn tool_call_evidence_serde_roundtrip() {
        let evidence = ToolCallEvidence {
            tool_name: "read_file".to_string(),
            arguments_hash: "sha256:deadbeef".to_string(),
            result_summary: "200 bytes read".to_string(),
            duration_ms: 12,
            risk_level: RiskLevel::Read,
        };
        let json = serde_json::to_string(&evidence).unwrap();
        let parsed: ToolCallEvidence = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tool_name, "read_file");
        assert_eq!(parsed.arguments_hash, "sha256:deadbeef");
        assert_eq!(parsed.duration_ms, 12);
        assert_eq!(parsed.risk_level, RiskLevel::Read);
    }

    #[test]
    fn audit_entry_without_trace_ctx_omits_field() {
        let entry = AuditEntry {
            id: "audit-min".to_string(),
            timestamp: chrono::DateTime::parse_from_rfc3339("2026-04-17T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
            principal_id: "user-1".to_string(),
            action: "session.create".to_string(),
            resource: "session".to_string(),
            outcome: AuditOutcome::Success,
            metadata: HashMap::new(),
            trace_ctx: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("trace_ctx"));
        let parsed: AuditEntry = serde_json::from_str(&json).unwrap();
        assert!(parsed.trace_ctx.is_none());
    }
}
