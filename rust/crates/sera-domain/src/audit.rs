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
}
