use sera_types::versioning::BuildIdentity;

#[test]
fn build_identity_serde_roundtrip() {
    let identity = BuildIdentity {
        version: "2.0.0".to_string(),
        commit: [0x1au8; 20],
        build_time: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        signer_fingerprint: [0xbbu8; 32],
        constitution_hash: [0xccu8; 32],
    };

    let json = serde_json::to_string(&identity).unwrap();
    let decoded: BuildIdentity = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded.version, identity.version);
    assert_eq!(decoded.commit, identity.commit);
    assert_eq!(decoded.build_time, identity.build_time);
    assert_eq!(decoded.signer_fingerprint, identity.signer_fingerprint);
    assert_eq!(decoded.constitution_hash, identity.constitution_hash);
}
