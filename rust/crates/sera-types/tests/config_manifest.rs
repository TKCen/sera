use sera_types::config_manifest::{PersonaSpec, ResourceKind, ResourceMetadata};

#[test]
fn resource_kind_has_sandbox_policy_circle_change_artifact() {
    assert_eq!(
        "SandboxPolicy".parse::<ResourceKind>().unwrap(),
        ResourceKind::SandboxPolicy
    );
    assert_eq!(
        "Circle".parse::<ResourceKind>().unwrap(),
        ResourceKind::Circle
    );
    assert_eq!(
        "ChangeArtifact".parse::<ResourceKind>().unwrap(),
        ResourceKind::ChangeArtifact
    );
}

#[test]
fn resource_metadata_shadow_field_defaults_false() {
    let json = r#"{"name":"test"}"#;
    let meta: ResourceMetadata = serde_json::from_str(json).unwrap();
    assert!(!meta.shadow);
}

#[test]
fn persona_spec_mutable_fields_round_trip() {
    let yaml = r#"
immutable_anchor: "You are Sera"
mutable_persona: "Friendly mode"
mutable_token_budget: 5000
"#;
    let spec: PersonaSpec = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(spec.mutable_persona.as_deref(), Some("Friendly mode"));
    assert_eq!(spec.mutable_token_budget, Some(5000));
    // Roundtrip
    let yaml_out = serde_yaml::to_string(&spec).unwrap();
    let parsed: PersonaSpec = serde_yaml::from_str(&yaml_out).unwrap();
    assert_eq!(parsed.mutable_persona, spec.mutable_persona);
    assert_eq!(parsed.mutable_token_budget, spec.mutable_token_budget);
}
