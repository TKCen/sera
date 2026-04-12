//! Integration tests for sera-auth Phase 0 extensions (P0-7).

use std::collections::HashSet;

use chrono::Utc;
use sera_auth::{
    authz::{Action, Resource},
    capability::{CapabilityToken, CapabilityTokenError},
    casbin_adapter::CasbinAuthzAdapter,
};
use sera_types::evolution::{AgentCapability, BlastRadius, ChangeArtifactId};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_token(
    caps: impl IntoIterator<Item = AgentCapability>,
    max_proposals: Option<u32>,
) -> CapabilityToken {
    CapabilityToken {
        token_id: uuid::Uuid::new_v4(),
        agent_id: "agent-test".to_string(),
        capabilities: caps.into_iter().collect(),
        blast_radius: None,
        proposals_consumed: 0,
        max_proposals,
        revocation_check_required: false,
        issued_at: Utc::now(),
        expires_at: None,
    }
}

const BASIC_MODEL: &str = r#"[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = r.sub == p.sub && r.obj == p.obj && r.act == p.act
"#;

// ---------------------------------------------------------------------------
// 1. capability_token_narrowing_removes_capability
// ---------------------------------------------------------------------------

#[test]
fn capability_token_narrowing_removes_capability() {
    let token = make_token([AgentCapability::MetaChange, AgentCapability::CodeChange], None);
    let narrowed = token
        .narrow(
            [AgentCapability::CodeChange].into_iter().collect(),
            None,
        )
        .expect("narrow to CodeChange only should succeed");

    assert!(narrowed.has(AgentCapability::CodeChange));
    assert!(!narrowed.has(AgentCapability::MetaChange));
}

// ---------------------------------------------------------------------------
// 2. capability_token_narrowing_widening_denied
// ---------------------------------------------------------------------------

#[test]
fn capability_token_narrowing_widening_denied() {
    let token = make_token([AgentCapability::CodeChange], None);
    let result = token.narrow(
        [AgentCapability::MetaChange, AgentCapability::CodeChange]
            .into_iter()
            .collect(),
        None,
    );
    assert!(
        matches!(result, Err(CapabilityTokenError::WideningAttempt)),
        "expected WideningAttempt, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// 3. capability_token_has_returns_correct_results
// ---------------------------------------------------------------------------

#[test]
fn capability_token_has_returns_correct_results() {
    let token = make_token(
        [AgentCapability::CodeChange, AgentCapability::ConfigRead],
        None,
    );
    assert!(token.has(AgentCapability::CodeChange));
    assert!(token.has(AgentCapability::ConfigRead));
    assert!(!token.has(AgentCapability::MetaChange));
    assert!(!token.has(AgentCapability::MetaApprover));
    assert!(!token.has(AgentCapability::ConfigPropose));
}

// ---------------------------------------------------------------------------
// 4. capability_token_proposal_limit_enforced
// ---------------------------------------------------------------------------

#[test]
fn capability_token_proposal_limit_enforced() {
    let mut token = make_token([AgentCapability::CodeChange], Some(2));

    assert!(token.consume_proposal().is_ok(), "first proposal should succeed");
    assert!(token.consume_proposal().is_ok(), "second proposal should succeed");

    let err = token.consume_proposal().expect_err("third proposal must fail");
    assert!(
        matches!(
            err,
            CapabilityTokenError::ProposalLimitExhausted { limit: 2, consumed: 2 }
        ),
        "expected ProposalLimitExhausted, got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 5. agent_capability_enum_exhaustive_serde
// ---------------------------------------------------------------------------

#[test]
fn agent_capability_enum_exhaustive_serde() {
    let all = [
        AgentCapability::MetaChange,
        AgentCapability::CodeChange,
        AgentCapability::MetaApprover,
        AgentCapability::ConfigRead,
        AgentCapability::ConfigPropose,
    ];

    for cap in &all {
        let json = serde_json::to_string(cap).expect("serialize AgentCapability");
        let parsed: AgentCapability =
            serde_json::from_str(&json).expect("deserialize AgentCapability");
        assert_eq!(&parsed, cap, "roundtrip failed for {cap:?}");
    }
}

// ---------------------------------------------------------------------------
// 6. propose_change_action_exists
// ---------------------------------------------------------------------------

#[test]
fn propose_change_action_exists() {
    let action = Action::ProposeChange(BlastRadius::AgentMemory);
    let json = serde_json::to_string(&action).expect("serialize ProposeChange");
    let parsed: Action = serde_json::from_str(&json).expect("deserialize ProposeChange");
    assert!(matches!(parsed, Action::ProposeChange(BlastRadius::AgentMemory)));
}

// ---------------------------------------------------------------------------
// 7. casbin_rbac_basic_allow_deny
// ---------------------------------------------------------------------------

#[tokio::test]
async fn casbin_rbac_basic_allow_deny() {
    let policy = "p, alice, data1, read\np, bob, data2, write\n";
    let adapter = CasbinAuthzAdapter::from_strings(BASIC_MODEL, policy)
        .await
        .expect("CasbinAuthzAdapter::from_strings");

    assert!(
        adapter.enforce("alice", "data1", "read").await.unwrap(),
        "alice should be allowed to read data1"
    );
    assert!(
        !adapter.enforce("alice", "data2", "write").await.unwrap(),
        "alice should NOT be allowed to write data2"
    );
    assert!(
        adapter.enforce("bob", "data2", "write").await.unwrap(),
        "bob should be allowed to write data2"
    );
    assert!(
        !adapter.enforce("bob", "data1", "read").await.unwrap(),
        "bob should NOT be allowed to read data1"
    );
}

// ---------------------------------------------------------------------------
// 8 & 9. argon2_password_verify_positive / negative
// ---------------------------------------------------------------------------

#[cfg(feature = "basic-auth")]
#[test]
fn argon2_password_verify_positive() {
    use sera_auth::api_key::{hash_key, ApiKeyValidator, StoredApiKey};

    let raw = "super-secret-key-correct";
    let hash = hash_key(raw);
    let stored = vec![StoredApiKey {
        key_hash_argon2: hash,
        operator_id: "op-1".to_string(),
        key_id: "key-id-1".to_string(),
    }];
    assert!(
        ApiKeyValidator::validate(raw, &stored).is_ok(),
        "correct key should validate"
    );
}

#[cfg(feature = "basic-auth")]
#[test]
fn argon2_password_verify_negative() {
    use sera_auth::api_key::{hash_key, ApiKeyValidator, StoredApiKey};

    let hash = hash_key("correct-key");
    let stored = vec![StoredApiKey {
        key_hash_argon2: hash,
        operator_id: "op-1".to_string(),
        key_id: "key-id-1".to_string(),
    }];
    assert!(
        ApiKeyValidator::validate("wrong-key", &stored).is_err(),
        "wrong key should fail"
    );
}

// ---------------------------------------------------------------------------
// 10. plaintext_comparison_path_absent
// ---------------------------------------------------------------------------

#[test]
fn plaintext_comparison_path_absent() {
    let src = include_str!("../src/api_key.rs");
    // Split the forbidden pattern so this file doesn't itself trigger the check.
    let forbidden = ["key_hash", " =="].concat();
    assert!(
        !src.contains(&forbidden),
        "plaintext comparison found in api_key.rs — must use argon2 verification"
    );
}

// ---------------------------------------------------------------------------
// 11. capability_token_serde_roundtrip
// ---------------------------------------------------------------------------

#[test]
fn capability_token_serde_roundtrip() {
    let mut caps = HashSet::new();
    caps.insert(AgentCapability::CodeChange);
    caps.insert(AgentCapability::ConfigRead);

    let token = CapabilityToken {
        token_id: uuid::Uuid::new_v4(),
        agent_id: "agent-serde-test".to_string(),
        capabilities: caps.clone(),
        blast_radius: Some(BlastRadius::SingleToolPolicy),
        proposals_consumed: 3,
        max_proposals: Some(10),
        revocation_check_required: true,
        issued_at: Utc::now(),
        expires_at: None,
    };

    let json = serde_json::to_string(&token).expect("serialize CapabilityToken");
    let parsed: CapabilityToken =
        serde_json::from_str(&json).expect("deserialize CapabilityToken");

    assert_eq!(parsed.token_id, token.token_id);
    assert_eq!(parsed.agent_id, token.agent_id);
    assert_eq!(parsed.capabilities, caps);
    assert_eq!(parsed.blast_radius, Some(BlastRadius::SingleToolPolicy));
    assert_eq!(parsed.proposals_consumed, 3);
    assert_eq!(parsed.max_proposals, Some(10));
    assert!(parsed.revocation_check_required);
}

// ---------------------------------------------------------------------------
// 12. change_artifact_resource_exists
// ---------------------------------------------------------------------------

#[test]
fn change_artifact_resource_exists() {
    let artifact_id = ChangeArtifactId { hash: [0xAB; 32] };
    let resource = Resource::ChangeArtifact(artifact_id);
    let json = serde_json::to_string(&resource).expect("serialize Resource::ChangeArtifact");
    let parsed: Resource =
        serde_json::from_str(&json).expect("deserialize Resource::ChangeArtifact");
    assert!(matches!(parsed, Resource::ChangeArtifact(_)));
}
