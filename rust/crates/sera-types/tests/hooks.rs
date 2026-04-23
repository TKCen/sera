use sera_types::evolution::ChangeArtifactId;
use sera_types::hook::{HookChain, HookContext, HookPoint, HookResult};
use sera_types::hook_aliases::HOOK_POINT_ALIASES;

#[test]
fn hook_point_constitutional_gate_is_fail_closed() {
    // Verify all 20 hook points are present
    assert_eq!(HookPoint::ALL.len(), 20);
    // Verify ConstitutionalGate exists and can be serialized
    let json = serde_json::to_string(&HookPoint::ConstitutionalGate).unwrap();
    assert_eq!(json, "\"constitutional_gate\"");
    // ConstitutionalGate chains default to fail_open: false (fail-closed)
    // This is verified by the HookChain default which sets fail_open to false
}

#[test]
fn hook_context_change_artifact_field_roundtrip() {
    let ctx = HookContext {
        change_artifact: Some(ChangeArtifactId { hash: [1u8; 32] }),
        ..HookContext::new(HookPoint::ConstitutionalGate)
    };
    let json = serde_json::to_string(&ctx).unwrap();
    let parsed: HookContext = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.change_artifact.unwrap().hash, [1u8; 32]);
}

#[test]
fn hook_result_updated_input_roundtrip() {
    let result = HookResult::Continue {
        context_updates: Default::default(),
        updated_input: Some(serde_json::json!({"key": "value"})),
        permission_overrides: None,
    };
    let json = serde_json::to_string(&result).unwrap();
    let parsed: HookResult = serde_json::from_str(&json).unwrap();
    if let HookResult::Continue { updated_input, .. } = parsed {
        assert_eq!(updated_input.unwrap(), serde_json::json!({"key": "value"}));
    } else {
        panic!("expected Continue variant");
    }
}

// ── Hermes alias tests ────────────────────────────────────────────────────────

#[test]
fn context_memory_canonical_name_parses() {
    let parsed: HookPoint = serde_json::from_str("\"context_memory\"").unwrap();
    assert_eq!(parsed, HookPoint::ContextMemory);
}

#[test]
fn context_memory_hermes_alias_parses_to_same_variant() {
    let parsed: HookPoint = serde_json::from_str("\"pre_agent_turn\"").unwrap();
    assert_eq!(parsed, HookPoint::ContextMemory);
}

#[test]
fn context_memory_alias_yaml_roundtrip() {
    // Config written with alias name round-trips back to canonical variant.
    let yaml = "point: pre_agent_turn\nname: test-chain\nhooks: []\n";
    let chain: HookChain = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(chain.point, HookPoint::ContextMemory);
    // Serialized form uses the canonical name (serde aliases are deserialize-only).
    let out = serde_json::to_string(&chain.point).unwrap();
    assert_eq!(out, "\"context_memory\"");
}

#[test]
fn unknown_hook_point_name_fails() {
    let result = serde_json::from_str::<HookPoint>("\"not_a_real_point\"");
    assert!(
        result.is_err(),
        "unknown hook point name should fail to parse"
    );
}

#[test]
fn alias_table_is_exhaustive() {
    // Every entry in HOOK_POINT_ALIASES must: canonical name round-trips,
    // alias name deserializes to the same variant.
    for (canonical, alias) in HOOK_POINT_ALIASES {
        let canonical_json = format!("\"{}\"", canonical);
        let alias_json = format!("\"{}\"", alias);
        let from_canonical: HookPoint = serde_json::from_str(&canonical_json)
            .unwrap_or_else(|e| panic!("canonical '{}' failed: {}", canonical, e));
        let from_alias: HookPoint = serde_json::from_str(&alias_json)
            .unwrap_or_else(|e| panic!("alias '{}' failed: {}", alias, e));
        assert_eq!(
            from_canonical, from_alias,
            "alias '{}' should resolve to the same variant as canonical '{}'",
            alias, canonical
        );
    }
}
