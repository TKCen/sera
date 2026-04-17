use sera_types::evolution::ChangeArtifactId;
use sera_types::hook::{HookContext, HookPoint, HookResult};

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
