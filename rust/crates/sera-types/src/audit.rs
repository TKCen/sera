//! Audit trail types — Merkle hash-chain event log.

use serde::{Deserialize, Serialize};

/// Actor type for audit events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActorType {
    Operator,
    Agent,
    System,
}

/// An audit event to record (input).
/// Maps from TS: AuditEntry in audit/AuditService.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub actor_type: ActorType,
    pub actor_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acting_context: Option<serde_json::Value>,
    pub event_type: String,
    pub payload: serde_json::Value,
}

/// A persisted audit record with hash chain fields.
/// Maps from TS: AuditRecord
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub id: String,
    pub sequence: String,
    pub timestamp: String,
    pub actor_type: ActorType,
    pub actor_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acting_context: Option<serde_json::Value>,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub prev_hash: Option<String>,
    pub hash: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_type_roundtrip() {
        for actor in [ActorType::Operator, ActorType::Agent, ActorType::System] {
            let json = serde_json::to_string(&actor).unwrap();
            let parsed: ActorType = serde_json::from_str(&json).unwrap();
            assert_eq!(actor, parsed);
        }
    }

    #[test]
    fn audit_entry_serialize() {
        let entry = AuditEntry {
            actor_type: ActorType::Agent,
            actor_id: "sera".to_string(),
            acting_context: None,
            event_type: "agent.started".to_string(),
            payload: serde_json::json!({"instance_id": "inst-1"}),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"actor_type\":\"agent\""));
        assert!(json.contains("agent.started"));
    }

    #[test]
    fn audit_record_with_hash_chain() {
        let record = AuditRecord {
            id: "audit-1".to_string(),
            sequence: "1".to_string(),
            timestamp: "2026-04-05T00:00:00Z".to_string(),
            actor_type: ActorType::Operator,
            actor_id: "op-123".to_string(),
            acting_context: Some(serde_json::json!({"method": "api_key"})),
            event_type: "agent.created".to_string(),
            payload: serde_json::json!({"agent_id": "agent-456"}),
            prev_hash: Some("hash-0".to_string()),
            hash: "hash-1".to_string(),
        };
        let json = serde_json::to_string(&record).unwrap();
        let parsed: AuditRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.sequence, "1");
        assert_eq!(parsed.prev_hash, Some("hash-0".to_string()));
        assert_eq!(parsed.hash, "hash-1");
    }

    #[test]
    fn audit_record_without_context() {
        let record = AuditRecord {
            id: "audit-2".to_string(),
            sequence: "2".to_string(),
            timestamp: "2026-04-05T01:00:00Z".to_string(),
            actor_type: ActorType::System,
            actor_id: "system".to_string(),
            acting_context: None,
            event_type: "system.check".to_string(),
            payload: serde_json::json!({}),
            prev_hash: None,
            hash: "hash-init".to_string(),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(!json.contains("acting_context"));
        // prev_hash serializes as null, not skipped
        assert!(json.contains("\"prev_hash\":null"));
    }

    #[test]
    fn actor_type_all_variants() {
        let types = vec![ActorType::Operator, ActorType::Agent, ActorType::System];
        let expected_names = vec!["operator", "agent", "system"];

        for (actor_type, expected) in types.iter().zip(expected_names.iter()) {
            let json = serde_json::to_string(actor_type).unwrap();
            assert_eq!(json, format!("\"{}\"", expected));
            let parsed: ActorType = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, actor_type);
        }
    }

    #[test]
    fn audit_entry_with_complex_payload() {
        let entry = AuditEntry {
            actor_type: ActorType::Agent,
            actor_id: "agent-789".to_string(),
            acting_context: None,
            event_type: "task.completed".to_string(),
            payload: serde_json::json!({
                "task_id": "task-123",
                "status": "success",
                "duration_ms": 5000,
                "tokens_used": {
                    "prompt": 1000,
                    "completion": 500
                }
            }),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: AuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.actor_id, "agent-789");
        let tokens = parsed.payload["tokens_used"].as_object().unwrap();
        assert_eq!(tokens["prompt"].as_u64(), Some(1000));
    }
}
