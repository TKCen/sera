use serde_json::json;
use sera_config::layer_merge::{LayeredManifestSet, ManifestLayer};

#[test]
fn later_layer_wins_on_conflict() {
    let mut set = LayeredManifestSet::new();

    let mut base = ManifestLayer::new("base");
    base.values.insert("key".to_string(), json!("base_value"));

    let mut overlay = ManifestLayer::new("overlay");
    overlay.values.insert("key".to_string(), json!("overlay_value"));

    set.push(base);
    set.push(overlay);

    let merged = set.merge();
    assert_eq!(merged.get("key").unwrap(), &json!("overlay_value"));
}

#[test]
fn non_overlapping_keys_are_all_present() {
    let mut set = LayeredManifestSet::new();

    let mut layer_a = ManifestLayer::new("a");
    layer_a.values.insert("key_a".to_string(), json!(1));

    let mut layer_b = ManifestLayer::new("b");
    layer_b.values.insert("key_b".to_string(), json!(2));

    set.push(layer_a);
    set.push(layer_b);

    let merged = set.merge();
    assert_eq!(merged.get("key_a").unwrap(), &json!(1));
    assert_eq!(merged.get("key_b").unwrap(), &json!(2));
}

#[test]
fn layer_names_in_push_order() {
    let mut set = LayeredManifestSet::new();
    set.push(ManifestLayer::new("first"));
    set.push(ManifestLayer::new("second"));
    set.push(ManifestLayer::new("third"));

    assert_eq!(set.layer_names(), vec!["first", "second", "third"]);
}
