use serde_json::json;
use sera_config::env_override::apply_env_overrides;
use sera_config::layer_merge::{LayeredManifestSet, ManifestLayer};

#[test]
fn env_override_higher_precedence_than_base() {
    // Set a SERA_ env var for this test.
    // SAFETY: single-threaded test; no other thread reads this var.
    unsafe { std::env::set_var("SERA_TEST_OVERRIDE_KEY", "env_value") };

    let mut set = LayeredManifestSet::new();
    let mut base_layer = ManifestLayer::new("base");
    base_layer.values.insert(
        "test_override_key".to_string(),
        json!("base_value"),
    );
    set.push(base_layer);

    apply_env_overrides(&mut set);

    let merged = set.merge();
    assert_eq!(merged.get("test_override_key").unwrap(), &json!("env_value"));

    unsafe { std::env::remove_var("SERA_TEST_OVERRIDE_KEY") };
}

#[test]
fn layer_merge_base_value_survives_when_no_env_override() {
    // Ensure no SERA_UNIQUE_BASE_KEY exists in env.
    unsafe { std::env::remove_var("SERA_UNIQUE_BASE_KEY") };

    let mut set = LayeredManifestSet::new();
    let mut base_layer = ManifestLayer::new("base");
    base_layer.values.insert("unique_base_key".to_string(), json!("base_value"));
    set.push(base_layer);

    apply_env_overrides(&mut set);

    let merged = set.merge();
    assert_eq!(merged.get("unique_base_key").unwrap(), &json!("base_value"));
}
