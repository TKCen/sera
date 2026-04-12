use serde_json::json;
use sera_config::version_log::{ChangeArtifactId, ConfigVersionLog};

#[test]
fn version_log_starts_empty() {
    let log = ConfigVersionLog::new();
    assert_eq!(log.version(), 0);
    assert_eq!(
        log.tail_hash(),
        "0000000000000000000000000000000000000000000000000000000000000000"
    );
    assert!(log.entries().is_empty());
}

#[test]
fn version_log_append_increments_version() {
    let mut log = ConfigVersionLog::new();
    log.append(ChangeArtifactId("ca-1".to_string()), None, json!({"a": 1}));
    assert_eq!(log.version(), 1);
    log.append(ChangeArtifactId("ca-2".to_string()), None, json!({"b": 2}));
    assert_eq!(log.version(), 2);
}

#[test]
fn version_log_chain_verifies_after_appends() {
    let mut log = ConfigVersionLog::new();
    log.append(ChangeArtifactId("ca-1".to_string()), None, json!({"x": 1}));
    log.append(ChangeArtifactId("ca-2".to_string()), None, json!({"x": 2}));
    log.append(ChangeArtifactId("ca-3".to_string()), None, json!({"x": 3}));
    assert!(log.verify_chain().is_ok());
}

#[test]
fn version_log_prev_hash_chain_is_linked() {
    let mut log = ConfigVersionLog::new();
    log.append(ChangeArtifactId("ca-1".to_string()), None, json!(1));
    log.append(ChangeArtifactId("ca-2".to_string()), None, json!(2));

    let entries = log.entries();
    // entry[1].prev_hash must equal entry[0].this_hash
    assert_eq!(entries[1].prev_hash, entries[0].this_hash);
}

#[test]
fn version_log_genesis_prev_hash_is_zero() {
    let mut log = ConfigVersionLog::new();
    log.append(ChangeArtifactId("ca-1".to_string()), None, json!("genesis"));

    let entries = log.entries();
    assert_eq!(
        entries[0].prev_hash,
        "0000000000000000000000000000000000000000000000000000000000000000"
    );
}
