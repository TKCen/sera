//! Exhaustive tests for LaneFailureClass.

use sera_telemetry::lane_failure::LaneFailureClass;

const ALL_VARIANTS: &[LaneFailureClass] = &[
    LaneFailureClass::PromptDelivery,
    LaneFailureClass::TrustGate,
    LaneFailureClass::BranchDivergence,
    LaneFailureClass::Compile,
    LaneFailureClass::Test,
    LaneFailureClass::PluginStartup,
    LaneFailureClass::McpStartup,
    LaneFailureClass::McpHandshake,
    LaneFailureClass::GatewayRouting,
    LaneFailureClass::ToolRuntime,
    LaneFailureClass::WorkspaceMismatch,
    LaneFailureClass::Infra,
    LaneFailureClass::OrphanReaped,
    LaneFailureClass::ConstitutionalViolation,
    LaneFailureClass::KillSwitchActivated,
];

#[test]
fn variant_count_is_15() {
    assert_eq!(ALL_VARIANTS.len(), 15);
}

#[test]
fn constitutional_violation_present() {
    assert!(ALL_VARIANTS.contains(&LaneFailureClass::ConstitutionalViolation));
}

#[test]
fn kill_switch_activated_present() {
    assert!(ALL_VARIANTS.contains(&LaneFailureClass::KillSwitchActivated));
}

#[test]
fn all_variants_have_ocsf_extension_strings() {
    for variant in ALL_VARIANTS {
        let ext = variant.as_ocsf_extension();
        assert!(!ext.is_empty(), "{variant:?} has empty OCSF extension");
    }
}

#[test]
fn full_serde_roundtrip() {
    for variant in ALL_VARIANTS {
        let json = serde_json::to_string(variant).expect("serialize");
        let restored: LaneFailureClass =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(variant, &restored, "roundtrip failed for {variant:?}");
    }
}

#[test]
fn detection_finding_ocsf_extension() {
    // Constitutional violation maps to OCSF Detection Finding extension
    assert_eq!(
        LaneFailureClass::ConstitutionalViolation.as_ocsf_extension(),
        "constitutional_violation"
    );
    assert_eq!(
        LaneFailureClass::KillSwitchActivated.as_ocsf_extension(),
        "kill_switch_activated"
    );
}
