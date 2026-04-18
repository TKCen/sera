use serde_json::json;
use sera_config::schema_registry::{SchemaRegistry, SchemaError};
use sera_types::config_manifest::ResourceKind;

#[test]
fn schema_registry_validate_rejects_invalid_payload() {
    let mut registry = SchemaRegistry::new();

    // Schema: object with required string field "name"
    let schema = json!({
        "type": "object",
        "required": ["name"],
        "properties": {
            "name": { "type": "string" }
        }
    });

    registry.register(ResourceKind::Agent, schema).unwrap();

    // Valid payload — should pass
    let valid = json!({ "name": "my-agent" });
    assert!(registry.validate(&ResourceKind::Agent, &valid).is_ok());

    // Invalid payload — missing required "name"
    let invalid = json!({ "provider": "openai" });
    let err = registry.validate(&ResourceKind::Agent, &invalid).unwrap_err();
    assert!(matches!(err, SchemaError::Invalid(_)));
}
