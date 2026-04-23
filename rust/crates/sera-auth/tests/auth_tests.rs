//! Integration tests for sera-auth Phase 0 extensions (P0-7).

use chrono::Utc;
use sera_auth::{
    authz::{Action, Resource},
    capability::{CapabilityToken, CapabilityTokenError},
    casbin_adapter::CasbinAuthzAdapter,
};
use sera_types::AgentCapability;
use sera_types::evolution::{BlastRadius, ChangeArtifactId};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_token(
    scopes: impl IntoIterator<Item = BlastRadius>,
    max_proposals: u32,
) -> CapabilityToken {
    CapabilityToken {
        id: "agent-test".to_string(),
        scopes: scopes.into_iter().collect(),
        expires_at: Utc::now() + chrono::Duration::hours(1),
        max_proposals,
        signature: [0u8; 64],
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
    let token = make_token(
        [BlastRadius::AgentMemory, BlastRadius::SingleHookConfig],
        10,
    );
    let narrowed = token
        .narrow([BlastRadius::SingleHookConfig].into_iter().collect())
        .expect("narrow to SingleHookConfig only should succeed");

    assert!(narrowed.has(BlastRadius::SingleHookConfig));
    assert!(!narrowed.has(BlastRadius::AgentMemory));
}

// ---------------------------------------------------------------------------
// 2. capability_token_narrowing_widening_denied
// ---------------------------------------------------------------------------

#[test]
fn capability_token_narrowing_widening_denied() {
    let token = make_token([BlastRadius::SingleHookConfig], 10);
    let result = token.narrow(
        [BlastRadius::AgentMemory, BlastRadius::SingleHookConfig]
            .into_iter()
            .collect(),
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
        [BlastRadius::SingleHookConfig, BlastRadius::AgentMemory],
        10,
    );
    assert!(token.has(BlastRadius::SingleHookConfig));
    assert!(token.has(BlastRadius::AgentMemory));
    assert!(!token.has(BlastRadius::GatewayCore));
    assert!(!token.has(BlastRadius::RuntimeCrate));
}

// ---------------------------------------------------------------------------
// 4. capability_token_proposal_limit_enforced
// ---------------------------------------------------------------------------

#[test]
fn capability_token_proposal_limit_enforced() {
    let token = make_token([BlastRadius::SingleHookConfig], 2);

    assert!(token.consume_proposal(0).is_ok(), "used=0 should succeed");
    assert!(token.consume_proposal(1).is_ok(), "used=1 should succeed");

    let err = token.consume_proposal(2).expect_err("used=2 must fail");
    assert!(
        matches!(
            err,
            CapabilityTokenError::ProposalLimitExhausted { limit: 2 }
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
    let scopes: std::collections::HashSet<BlastRadius> =
        [BlastRadius::SingleToolPolicy, BlastRadius::AgentMemory]
            .into_iter()
            .collect();

    let token = CapabilityToken {
        id: "agent-serde-test".to_string(),
        scopes: scopes.clone(),
        expires_at: Utc::now() + chrono::Duration::minutes(15),
        max_proposals: 10,
        signature: [0xABu8; 64],
    };

    let json = serde_json::to_string(&token).expect("serialize CapabilityToken");
    let parsed: CapabilityToken =
        serde_json::from_str(&json).expect("deserialize CapabilityToken");

    assert_eq!(parsed.id, token.id);
    assert_eq!(parsed.scopes, scopes);
    assert_eq!(parsed.max_proposals, 10);
    assert_eq!(parsed.signature, [0xABu8; 64]);
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
