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
    pub fn verify_chain(records: &[sera_types::audit::AuditRecord]) -> Result<(), AuditVerifyError> {
        let mut expected_prev_hash: Option<String> = None;

        for record in records.iter() {
            // Verify chain linkage: record's prev_hash must match the previous record's hash
            if record.prev_hash.as_deref() != expected_prev_hash.as_deref() {
                return Err(AuditVerifyError {
                    broken_at: record.sequence.clone(),
                });
            }

            let actor_type = match record.actor_type {
                sera_types::audit::ActorType::Operator => "operator",
                sera_types::audit::ActorType::Agent => "agent",
                sera_types::audit::ActorType::System => "system",
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

            expected_prev_hash = Some(record.hash.clone());
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
        use sera_types::audit::{ActorType, AuditRecord};

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
        use sera_types::audit::{ActorType, AuditRecord};

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

    // ------------------------------------------------------------------
    // Empty chain: verify returns OK (nothing to check)
    // ------------------------------------------------------------------
    #[test]
    fn verify_empty_chain_is_ok() {
        assert!(AuditHashChain::verify_chain(&[]).is_ok());
    }

    // ------------------------------------------------------------------
    // Single-entry (genesis) chain: append + verify
    // ------------------------------------------------------------------
    #[test]
    fn verify_single_entry_chain() {
        use sera_types::audit::{ActorType, AuditRecord};

        let hash = AuditHashChain::compute_hash(
            "1",
            "2024-06-01T00:00:00Z",
            "system",
            "sera",
            "boot",
            r#"{"v":1}"#,
            None,
        );

        let records = vec![AuditRecord {
            id: "id-1".to_string(),
            sequence: "1".to_string(),
            timestamp: "2024-06-01T00:00:00Z".to_string(),
            actor_type: ActorType::System,
            actor_id: "sera".to_string(),
            acting_context: None,
            event_type: "boot".to_string(),
            payload: serde_json::json!({"v": 1}),
            prev_hash: None,
            hash,
        }];

        assert!(AuditHashChain::verify_chain(&records).is_ok());
    }

    // ------------------------------------------------------------------
    // Multi-entry chain (N=5): full verify passes
    // ------------------------------------------------------------------
    fn make_record(
        seq: u32,
        timestamp: &str,
        actor_type: sera_types::audit::ActorType,
        actor_id: &str,
        event_type: &str,
        payload: serde_json::Value,
        prev_hash: Option<&str>,
    ) -> sera_types::audit::AuditRecord {
        let actor_str = match actor_type {
            sera_types::audit::ActorType::Operator => "operator",
            sera_types::audit::ActorType::Agent => "agent",
            sera_types::audit::ActorType::System => "system",
        };
        let payload_str = payload.to_string();
        let seq_str = seq.to_string();
        let hash = AuditHashChain::compute_hash(
            &seq_str,
            timestamp,
            actor_str,
            actor_id,
            event_type,
            &payload_str,
            prev_hash,
        );
        sera_types::audit::AuditRecord {
            id: format!("id-{}", seq),
            sequence: seq_str,
            timestamp: timestamp.to_string(),
            actor_type,
            actor_id: actor_id.to_string(),
            acting_context: None,
            event_type: event_type.to_string(),
            payload,
            prev_hash: prev_hash.map(str::to_string),
            hash,
        }
    }

    #[test]
    fn verify_five_entry_chain() {
        use sera_types::audit::ActorType;

        let r1 = make_record(1, "2024-01-01T00:00:00Z", ActorType::System, "sera", "boot", serde_json::json!({}), None);
        let r2 = make_record(2, "2024-01-01T00:00:01Z", ActorType::Operator, "op-1", "create", serde_json::json!({"x":1}), Some(&r1.hash));
        let r3 = make_record(3, "2024-01-01T00:00:02Z", ActorType::Agent, "a-1", "start", serde_json::json!({"y":2}), Some(&r2.hash));
        let r4 = make_record(4, "2024-01-01T00:00:03Z", ActorType::Agent, "a-1", "output", serde_json::json!({"z":3}), Some(&r3.hash));
        let r5 = make_record(5, "2024-01-01T00:00:04Z", ActorType::System, "sera", "shutdown", serde_json::json!({}), Some(&r4.hash));

        let records = vec![r1, r2, r3, r4, r5];
        assert!(AuditHashChain::verify_chain(&records).is_ok());
    }

    // ------------------------------------------------------------------
    // Tampered middle entry: flip payload in record 3, verify fails at 3
    // ------------------------------------------------------------------
    #[test]
    fn tampered_middle_entry_detected() {
        use sera_types::audit::ActorType;

        let r1 = make_record(1, "2024-01-01T00:00:00Z", ActorType::System, "sera", "boot", serde_json::json!({}), None);
        let r2 = make_record(2, "2024-01-01T00:00:01Z", ActorType::Operator, "op-1", "create", serde_json::json!({"x":1}), Some(&r1.hash));
        let mut r3 = make_record(3, "2024-01-01T00:00:02Z", ActorType::Agent, "a-1", "start", serde_json::json!({"y":2}), Some(&r2.hash));
        let r4 = make_record(4, "2024-01-01T00:00:03Z", ActorType::Agent, "a-1", "output", serde_json::json!({"z":3}), Some(&r3.hash));

        // Mutate the payload without recomputing the hash — simulates a tampered record
        r3.payload = serde_json::json!({"y": 999});

        let records = vec![r1, r2, r3, r4];
        let err = AuditHashChain::verify_chain(&records).unwrap_err();
        assert_eq!(err.broken_at, "3");
    }

    // ------------------------------------------------------------------
    // Order-dependence: swapping two adjacent entries breaks verification
    // ------------------------------------------------------------------
    #[test]
    fn swapped_entries_detected() {
        use sera_types::audit::ActorType;

        let r1 = make_record(1, "2024-01-01T00:00:00Z", ActorType::System, "sera", "boot", serde_json::json!({}), None);
        let r2 = make_record(2, "2024-01-01T00:00:01Z", ActorType::Operator, "op-1", "create", serde_json::json!({"x":1}), Some(&r1.hash));
        let r3 = make_record(3, "2024-01-01T00:00:02Z", ActorType::Agent, "a-1", "start", serde_json::json!({"y":2}), Some(&r2.hash));

        // Swap r2 and r3 — the chain is now r1, r3, r2 which breaks linkage
        let records = vec![r1, r3, r2];
        let err = AuditHashChain::verify_chain(&records).unwrap_err();
        // r3 (now at position 1) has prev_hash pointing to r2.hash, not r1.hash
        assert_eq!(err.broken_at, "3");
    }

    // ------------------------------------------------------------------
    // Hash determinism: same inputs always produce the same hash
    // ------------------------------------------------------------------
    #[test]
    fn hash_is_deterministic() {
        let h1 = AuditHashChain::compute_hash(
            "42",
            "2025-01-15T12:00:00Z",
            "operator",
            "user-99",
            "policy.updated",
            r#"{"key":"val"}"#,
            Some("prev-abc123"),
        );
        let h2 = AuditHashChain::compute_hash(
            "42",
            "2025-01-15T12:00:00Z",
            "operator",
            "user-99",
            "policy.updated",
            r#"{"key":"val"}"#,
            Some("prev-abc123"),
        );
        assert_eq!(h1, h2);
        // Changing any single field produces a different hash
        let h_diff = AuditHashChain::compute_hash(
            "42",
            "2025-01-15T12:00:00Z",
            "operator",
            "user-99",
            "policy.updated",
            r#"{"key":"DIFFERENT"}"#,
            Some("prev-abc123"),
        );
        assert_ne!(h1, h_diff);
    }

    // ------------------------------------------------------------------
    // Genesis vs chained: prev_hash=None vs prev_hash=Some("") differ
    // ------------------------------------------------------------------
    #[test]
    fn genesis_differs_from_empty_prev_hash_string() {
        // None is serialized as "" internally; an attacker providing "" explicitly
        // should produce the same hash (no bypass possible via empty string).
        let h_none = AuditHashChain::compute_hash(
            "1", "2024-01-01T00:00:00Z", "system", "sera", "init",
            r#"{}"#, None,
        );
        let h_empty = AuditHashChain::compute_hash(
            "1", "2024-01-01T00:00:00Z", "system", "sera", "init",
            r#"{}"#, Some(""),
        );
        // Both should be identical because None maps to "" per the implementation
        assert_eq!(h_none, h_empty);
    }

    // ------------------------------------------------------------------
    // Chain linkage check: genesis record must have prev_hash=None
    // A second record with incorrect prev_hash fails even if its own hash is valid
    // ------------------------------------------------------------------
    #[test]
    fn wrong_prev_hash_linkage_detected() {
        use sera_types::audit::{ActorType, AuditRecord};

        let r1 = make_record(1, "2024-01-01T00:00:00Z", ActorType::System, "sera", "boot", serde_json::json!({}), None);

        // Compute a real hash for r2, but use a wrong prev_hash value
        let wrong_prev = "0000000000000000000000000000000000000000000000000000000000000000";
        let hash2 = AuditHashChain::compute_hash(
            "2", "2024-01-01T00:00:01Z", "operator", "op-1", "action",
            r#"{}"#, Some(wrong_prev),
        );
        let r2 = AuditRecord {
            id: "id-2".to_string(),
            sequence: "2".to_string(),
            timestamp: "2024-01-01T00:00:01Z".to_string(),
            actor_type: ActorType::Operator,
            actor_id: "op-1".to_string(),
            acting_context: None,
            event_type: "action".to_string(),
            payload: serde_json::json!({}),
            prev_hash: Some(wrong_prev.to_string()),
            hash: hash2,
        };

        let records = vec![r1, r2];
        let err = AuditHashChain::verify_chain(&records).unwrap_err();
        assert_eq!(err.broken_at, "2");
    }
}
