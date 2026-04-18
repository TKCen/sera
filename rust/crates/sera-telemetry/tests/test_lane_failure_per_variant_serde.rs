//! Per-variant serde string verification for all 15 LaneFailureClass variants.
//!
//! Each variant must serialize to its exact snake_case JSON string and
//! deserialize back correctly.  This catches any accidental rename mismatch.

use sera_telemetry::lane_failure::LaneFailureClass;

/// (variant, expected JSON string including quotes)
const SERDE_CASES: &[(LaneFailureClass, &str)] = &[
    (LaneFailureClass::PromptDelivery, r#""prompt_delivery""#),
    (LaneFailureClass::TrustGate, r#""trust_gate""#),
    (LaneFailureClass::BranchDivergence, r#""branch_divergence""#),
    (LaneFailureClass::Compile, r#""compile""#),
    (LaneFailureClass::Test, r#""test""#),
    (LaneFailureClass::PluginStartup, r#""plugin_startup""#),
    (LaneFailureClass::McpStartup, r#""mcp_startup""#),
    (LaneFailureClass::McpHandshake, r#""mcp_handshake""#),
    (LaneFailureClass::GatewayRouting, r#""gateway_routing""#),
    (LaneFailureClass::ToolRuntime, r#""tool_runtime""#),
    (LaneFailureClass::WorkspaceMismatch, r#""workspace_mismatch""#),
    (LaneFailureClass::Infra, r#""infra""#),
    (LaneFailureClass::OrphanReaped, r#""orphan_reaped""#),
    (LaneFailureClass::ConstitutionalViolation, r#""constitutional_violation""#),
    (LaneFailureClass::KillSwitchActivated, r#""kill_switch_activated""#),
];

#[test]
fn every_variant_serializes_to_exact_snake_case_string() {
    for (variant, expected_json) in SERDE_CASES {
        let json = serde_json::to_string(variant)
            .unwrap_or_else(|e| panic!("serialize failed for {variant:?}: {e}"));
        assert_eq!(
            json, *expected_json,
            "wrong JSON for {variant:?}: got {json}, want {expected_json}"
        );
    }
}

#[test]
fn every_variant_deserializes_from_snake_case_string() {
    for (expected_variant, json) in SERDE_CASES {
        let got: LaneFailureClass = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("deserialize failed for {json}: {e}"));
        assert_eq!(
            &got, expected_variant,
            "wrong variant deserialized from {json}"
        );
    }
}

#[test]
fn serde_extension_string_matches_as_ocsf_extension() {
    // The JSON representation (without quotes) must equal as_ocsf_extension().
    for (variant, json_with_quotes) in SERDE_CASES {
        let stripped = json_with_quotes.trim_matches('"');
        assert_eq!(
            variant.as_ocsf_extension(),
            stripped,
            "as_ocsf_extension() disagrees with serde output for {variant:?}"
        );
    }
}

#[test]
fn all_ocsf_extension_strings_are_unique() {
    let mut seen = std::collections::HashSet::new();
    for (variant, _) in SERDE_CASES {
        let ext = variant.as_ocsf_extension();
        assert!(
            seen.insert(ext),
            "duplicate ocsf_extension string '{ext}' for {variant:?}"
        );
    }
}

#[test]
fn lane_failure_class_debug_contains_variant_name() {
    // Debug output must contain at least the PascalCase variant name.
    let cases: &[(LaneFailureClass, &str)] = &[
        (LaneFailureClass::PromptDelivery, "PromptDelivery"),
        (LaneFailureClass::KillSwitchActivated, "KillSwitchActivated"),
        (LaneFailureClass::ConstitutionalViolation, "ConstitutionalViolation"),
        (LaneFailureClass::Infra, "Infra"),
    ];
    for (variant, name) in cases {
        let debug = format!("{variant:?}");
        assert!(
            debug.contains(name),
            "Debug output '{debug}' does not contain '{name}'"
        );
    }
}

#[test]
fn lane_failure_class_hash_is_consistent() {
    use std::collections::HashMap;
    // Build a frequency map using LaneFailureClass as key (requires Hash + Eq).
    let mut counts: HashMap<LaneFailureClass, u32> = HashMap::new();
    for (variant, _) in SERDE_CASES {
        *counts.entry(*variant).or_insert(0) += 1;
    }
    // Each variant appears exactly once in SERDE_CASES.
    for count in counts.values() {
        assert_eq!(*count, 1);
    }
    assert_eq!(counts.len(), 15);
}

#[test]
fn lane_failure_class_clone_eq() {
    for (variant, _) in SERDE_CASES {
        let cloned = *variant; // Copy
        assert_eq!(variant, &cloned);
    }
}

#[test]
fn unknown_string_deserializes_to_error() {
    let result: Result<LaneFailureClass, _> = serde_json::from_str(r#""not_a_real_variant""#);
    assert!(result.is_err(), "expected error for unknown variant string");
}
