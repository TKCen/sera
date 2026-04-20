//! Integration tests for ConfigWatcher hot-reload.
//!
//! Verifies that:
//! - Writing a valid config file triggers a `ConfigReloaded` event.
//! - Writing an invalid config file triggers a `ConfigReloadFailed` event
//!   and leaves the previous valid config in place.

use std::fs;
use sera_config::{ConfigReloadEvent, ConfigWatcher};
use tempfile::TempDir;

/// Helper: create a temp dir with an initial valid config file, spawn a watcher.
fn setup(initial_yaml: &str) -> (TempDir, std::path::PathBuf, ConfigWatcher, tokio::task::JoinHandle<()>) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("config.yaml");
    fs::write(&path, initial_yaml).expect("write initial config");
    let (watcher, handle) = ConfigWatcher::spawn(path.clone()).expect("spawn watcher");
    (dir, path, watcher, handle)
}

#[tokio::test]
async fn valid_file_emits_config_reloaded() {
    let (_dir, path, watcher, _handle) = setup("key: initial\n");
    let mut rx = watcher.subscribe();

    // Give the watcher a moment to start.
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Overwrite with a new valid config.
    fs::write(&path, "key: updated\n").expect("write updated config");

    let event = tokio::time::timeout(tokio::time::Duration::from_secs(3), rx.recv())
        .await
        .expect("timed out waiting for reload event")
        .expect("channel closed");

    match event {
        ConfigReloadEvent::Reloaded(r) => {
            assert_eq!(r.path, path);
            assert!(!r.version.is_empty(), "version sha256 should be non-empty");
        }
        ConfigReloadEvent::Failed(f) => {
            panic!("expected Reloaded, got Failed: {}", f.reason);
        }
    }

    // Active config should now reflect the new value.
    let config = watcher.config();
    let active = config.read().await;
    assert_eq!(active["key"].as_str(), Some("updated"));
}

#[tokio::test]
async fn invalid_file_emits_config_reload_failed_and_preserves_previous() {
    let (_dir, path, watcher, _handle) = setup("key: good\n");
    let mut rx = watcher.subscribe();

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Write invalid YAML.
    fs::write(&path, "key: [unclosed\n  bad: indent\n").expect("write bad yaml");

    let event = tokio::time::timeout(tokio::time::Duration::from_secs(3), rx.recv())
        .await
        .expect("timed out waiting for reload event")
        .expect("channel closed");

    match event {
        ConfigReloadEvent::Failed(f) => {
            assert_eq!(f.path, path);
            assert!(!f.reason.is_empty(), "reason should be non-empty");
        }
        ConfigReloadEvent::Reloaded(_) => {
            panic!("expected Failed for invalid YAML, got Reloaded");
        }
    }

    // Active config must still be the original good value.
    let config = watcher.config();
    let active = config.read().await;
    assert_eq!(active["key"].as_str(), Some("good"));
}
