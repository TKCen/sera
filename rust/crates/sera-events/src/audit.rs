//! Audit hash chain verification.

use sha2::{Digest, Sha256};

use crate::error::AuditVerifyError;

/// Audit hash chain verifier and computer.
pub struct AuditHashChain;

impl AuditHashChain {
    /// Compute a SHA-256 hash for an audit record.
    ///
    /// Concatenates all fields with "|" separator and returns hex string.
    /// If `prev_hash` is None (genesis), uses empty string as prev_hash value.
    pub fn compute_hash(
        sequence: &str,
        timestamp: &str,
        actor_type: &str,
        actor_id: &str,
        event_type: &str,
        payload: &str,
        prev_hash: Option<&str>,
    ) -> String {
        let prev = prev_hash.unwrap_or("");
        let input = format!(
            "{}|{}|{}|{}|{}|{}|{}",
            sequence, timestamp, actor_type, actor_id, event_type, payload, prev
        );

        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Verify the integrity of an audit chain.
    ///
    /// Iterates through records, recomputes each hash, and compares with stored hash.
    /// Returns error with the sequence number of the first broken record.
    pub fn verify_chain(records: &[sera_domain::audit::AuditRecord]) -> Result<(), AuditVerifyError> {
        for record in records.iter() {
            let actor_type = match record.actor_type {
                sera_domain::audit::ActorType::Operator => "operator",
                sera_domain::audit::ActorType::Agent => "agent",
                sera_domain::audit::ActorType::System => "system",
            };

            let payload_str = record.payload.to_string();
            let computed = Self::compute_hash(
                &record.sequence,
                &record.timestamp,
                actor_type,
                &record.actor_id,
                &record.event_type,
                &payload_str,
                record.prev_hash.as_deref(),
            );

            if computed != record.hash {
                return Err(AuditVerifyError {
                    broken_at: record.sequence.clone(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_genesis_hash() {
        let hash = AuditHashChain::compute_hash(
            "1",
            "2024-01-01T00:00:00Z",
            "system",
            "sera",
            "init",
            r#"{"msg":"genesis"}"#,
            None,
        );
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA-256 hex is 64 chars
    }

    #[test]
    fn compute_chain_hash() {
        let genesis = AuditHashChain::compute_hash(
            "1",
            "2024-01-01T00:00:00Z",
            "system",
            "sera",
            "init",
            r#"{"msg":"genesis"}"#,
            None,
        );

        let second = AuditHashChain::compute_hash(
            "2",
            "2024-01-01T00:00:01Z",
            "agent",
            "agent-1",
            "started",
            r#"{"instance_id":"inst-1"}"#,
            Some(&genesis),
        );

        assert_ne!(genesis, second);
        assert_eq!(second.len(), 64);
    }

    #[test]
    fn verify_valid_chain() {
        use sera_domain::audit::{ActorType, AuditRecord};

        let hash1 = AuditHashChain::compute_hash(
            "1",
            "2024-01-01T00:00:00Z",
            "system",
            "sera",
            "init",
            r#"{"msg":"genesis"}"#,
            None,
        );

        let hash2 = AuditHashChain::compute_hash(
            "2",
            "2024-01-01T00:00:01Z",
            "agent",
            "agent-1",
            "started",
            r#"{"instance_id":"inst-1"}"#,
            Some(&hash1),
        );

        let hash3 = AuditHashChain::compute_hash(
            "3",
            "2024-01-01T00:00:02Z",
            "operator",
            "op-1",
            "created_agent",
            r#"{"agent_id":"agent-1"}"#,
            Some(&hash2),
        );

        let records = vec![
            AuditRecord {
                id: "id-1".to_string(),
                sequence: "1".to_string(),
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                actor_type: ActorType::System,
                actor_id: "sera".to_string(),
                acting_context: None,
                event_type: "init".to_string(),
                payload: serde_json::json!({"msg": "genesis"}),
                prev_hash: None,
                hash: hash1.clone(),
            },
            AuditRecord {
                id: "id-2".to_string(),
                sequence: "2".to_string(),
                timestamp: "2024-01-01T00:00:01Z".to_string(),
                actor_type: ActorType::Agent,
                actor_id: "agent-1".to_string(),
                acting_context: None,
                event_type: "started".to_string(),
                payload: serde_json::json!({"instance_id": "inst-1"}),
                prev_hash: Some(hash1),
                hash: hash2.clone(),
            },
            AuditRecord {
                id: "id-3".to_string(),
                sequence: "3".to_string(),
                timestamp: "2024-01-01T00:00:02Z".to_string(),
                actor_type: ActorType::Operator,
                actor_id: "op-1".to_string(),
                acting_context: None,
                event_type: "created_agent".to_string(),
                payload: serde_json::json!({"agent_id": "agent-1"}),
                prev_hash: Some(hash2),
                hash: hash3,
            },
        ];

        assert!(AuditHashChain::verify_chain(&records).is_ok());
    }

    #[test]
    fn detect_tampered_record() {
        use sera_domain::audit::{ActorType, AuditRecord};

        let hash1 = AuditHashChain::compute_hash(
            "1",
            "2024-01-01T00:00:00Z",
            "system",
            "sera",
            "init",
            r#"{"msg":"genesis"}"#,
            None,
        );

        let records = vec![
            AuditRecord {
                id: "id-1".to_string(),
                sequence: "1".to_string(),
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                actor_type: ActorType::System,
                actor_id: "sera".to_string(),
                acting_context: None,
                event_type: "init".to_string(),
                payload: serde_json::json!({"msg": "genesis"}),
                prev_hash: None,
                hash: hash1.clone(),
            },
            AuditRecord {
                id: "id-2".to_string(),
                sequence: "2".to_string(),
                timestamp: "2024-01-01T00:00:01Z".to_string(),
                actor_type: ActorType::Agent,
                actor_id: "agent-1".to_string(),
                acting_context: None,
                event_type: "started".to_string(),
                payload: serde_json::json!({"instance_id": "inst-1"}),
                prev_hash: Some(hash1),
                hash: "tampered_hash".to_string(), // Intentionally wrong
            },
        ];

        let result = AuditHashChain::verify_chain(&records);
        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(e.broken_at, "2");
        }
    }
}
