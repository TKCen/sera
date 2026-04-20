//! Legacy SHA-256 audit hash chain (migrated from `sera-events::audit`).
//!
//! Used by `sera-gateway::services::audit::AuditService` to validate and
//! continue the `AuditRecord`-based chain stored in Postgres. Kept distinct
//! from the OCSF-backed [`crate::audit`] module because the on-disk format
//! and hash input layout differ.

use sha2::{Digest, Sha256};
use thiserror::Error;

use sera_errors::{SeraError, SeraErrorCode};

#[derive(Debug, Error)]
#[error("Audit chain broken at sequence {broken_at}")]
pub struct AuditVerifyError {
    pub broken_at: String,
}

impl From<AuditVerifyError> for SeraError {
    fn from(err: AuditVerifyError) -> Self {
        SeraError::with_source(SeraErrorCode::InvalidInput, err.to_string(), err)
    }
}

pub struct AuditHashChain;

impl AuditHashChain {
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

    pub fn verify_chain(records: &[sera_types::audit::AuditRecord]) -> Result<(), AuditVerifyError> {
        let mut expected_prev_hash: Option<String> = None;

        for record in records.iter() {
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
        assert_eq!(hash.len(), 64);
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
                hash: hash2,
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
                hash: "tampered_hash".to_string(),
            },
        ];

        let result = AuditHashChain::verify_chain(&records);
        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(e.broken_at, "2");
        }
    }

    #[test]
    fn verify_empty_chain_is_ok() {
        assert!(AuditHashChain::verify_chain(&[]).is_ok());
    }

    #[test]
    fn audit_verify_error_display() {
        let e = AuditVerifyError { broken_at: "7".to_string() };
        assert_eq!(e.to_string(), "Audit chain broken at sequence 7");
    }

    #[test]
    fn audit_verify_error_maps_to_invalid_input() {
        let e: SeraError = AuditVerifyError { broken_at: "5".to_string() }.into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
    }
}
